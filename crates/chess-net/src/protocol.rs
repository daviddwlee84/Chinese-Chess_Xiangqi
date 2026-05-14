//! Wire protocol between `chess-net-server` and clients.
//!
//! Frames are JSON text frames, one message per frame. Tagged enums via
//! `#[serde(tag = "type")]` keep the schema obvious in `wscat`/`jq`.
//!
//! All payload types come straight from `chess-core` and already derive
//! `Serialize/Deserialize`, including the proptest-validated no-leak
//! projection in `PlayerView` (hidden banqi pieces stay opaque on the wire).

use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::{RuleSet, Variant};
use chess_core::view::PlayerView;
use serde::{Deserialize, Serialize};

/// Bumped whenever the wire schema changes incompatibly.
///
/// v1 → v2: added `ServerMsg::Rooms` and `ClientMsg::ListRooms` for the
/// multi-room lobby. Old (v1) clients still work against a v2 server in
/// the default room "main" — the new message variants are additive.
///
/// v2 → v3: added `ServerMsg::{Spectating, ChatHistory, Chat}` and
/// `ClientMsg::Chat` for read-only spectators + in-room chat. v2 clients
/// keep working as players (they never request spectator role; the new
/// chat variants are additive). `RoomSummary` gains a `spectators` field
/// with `#[serde(default)]` so v2 lobby clients deserializing a v3 server
/// snapshot ignore the new column.
///
/// v3 → v4: added `PlayerView.in_check: bool` so clients can render a
/// "將軍 / CHECK" warning without having to recompute it from the board.
/// Wire-compatible — `#[serde(default)]` on the new field means a v3
/// client deserializing a v4 message reads `in_check = false` and a v4
/// client deserializing a v3 message gets the same default. The bump is
/// for documentation, not enforcement; the handshake still uses lenient
/// equality. See ADR-0007.
///
/// v4 → v5: added `hints_allowed: bool` to `RoomSummary`, `Hello`, and
/// `Spectating` so clients can decide whether to mount the AI debug /
/// hint panel for a given room. Set at room creation time from the
/// first joiner's `?hints=1` query parameter, frozen for the room's
/// lifetime (same pattern as `password`). Closes the previous
/// client-only-`?debug=1` cheat hole — net-mode clients now refuse to
/// mount the panel unless the server says hints are allowed in this
/// room. Wire-compatible: `#[serde(default)]` on every new field means
/// v4 clients deserializing v5 messages see `hints_allowed = false`,
/// and v5 clients deserializing v4 messages also see `false`. Local
/// (offline / GitHub Pages) `?debug=1` and `?hints=1` are unaffected
/// — they bypass the server entirely.
pub const PROTOCOL_VERSION: u16 = 5;

/// Server → client.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    /// Sent once per client right after side assignment. Carries the variant
    /// (so the client knows what to render) plus the initial `PlayerView`.
    /// `hints_allowed` (v5+) reflects whether the room was created with
    /// `?hints=1`; clients consult this to decide whether to mount the
    /// AI debug / hint panel.
    Hello {
        protocol: u16,
        observer: Side,
        rules: RuleSet,
        view: PlayerView,
        #[serde(default)]
        hints_allowed: bool,
    },
    /// Sent to every seated client after each committed move.
    Update { view: PlayerView },
    /// Illegal move, room full, malformed message, opponent disconnect, etc.
    /// Non-fatal by default — clients should display and keep the connection.
    Error { message: String },
    /// Pushed on the lobby socket only — initial snapshot on connect, then
    /// again whenever any room's state changes (seat insertion / removal /
    /// game finish / GC). Game sockets never receive this variant.
    Rooms { rooms: Vec<RoomSummary> },
    /// Spectator-side counterpart to `Hello` — no `observer` because
    /// spectators are not seated. Carries the same `rules` + initial `view`
    /// (projected from `Side::RED`'s perspective so banqi hidden tiles stay
    /// hidden). Sent immediately after a `?role=spectator` connection
    /// upgrades successfully. `hints_allowed` (v5+) — see [`Hello`].
    Spectating {
        protocol: u16,
        rules: RuleSet,
        view: PlayerView,
        #[serde(default)]
        hints_allowed: bool,
    },
    /// Pushed once right after `Hello` / `Spectating` with the room's chat
    /// ring buffer (≤50 lines). Empty `lines` for a fresh room.
    ChatHistory { lines: Vec<ChatLine> },
    /// Pushed live to every recipient (seats + spectators) on each new chat
    /// line. `from` is currently always a player's `Side`; system messages
    /// (player joined / left) are deferred — see
    /// `backlog/chess-net-system-messages.md`.
    Chat { line: ChatLine },
}

/// Client → server.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    /// Submit a move. For `Move::Reveal`, send `revealed: None` — the server
    /// fills in the piece identity locally; the wire form stays `None`.
    Move { mv: Move },
    /// Resign — ends the game with `WinReason::Resignation` for the opponent.
    Resign,
    /// Request a rematch. Only valid after the current game has ended
    /// (Won / Drawn). Server resets the game when both sides have requested
    /// and re-sends `Hello` to each. Same observer assignment, same rules,
    /// fresh `GameState`.
    Rematch,
    /// Lobby-only: forces a fresh `Rooms` snapshot to the requester. The
    /// server pushes `Rooms` automatically on state changes; this is the
    /// manual-refresh button (`r` in the TUI lobby).
    ListRooms,
    /// Send a chat line. Server enforces players-only (spectators get
    /// `Error{"spectators cannot chat"}`), trims to ≤256 chars after stripping
    /// control chars, stamps `ts_ms` server-side, and broadcasts the result
    /// to every seat + spectator as `ServerMsg::Chat`.
    Chat { text: String },
}

/// One chat line as it appears on the wire and in the per-room ring buffer.
/// `ts_ms` is unix milliseconds set server-side so clients don't have to
/// agree on a clock.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatLine {
    pub from: Side,
    pub text: String,
    pub ts_ms: u64,
}

/// One row in the lobby browser. Mirrors what's interesting to a player
/// deciding whether to join: variant for setting expectations, seat count
/// for "is this room joinable?", `has_password` for the lock icon, and
/// `status` so finished rooms stand out from active games.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomSummary {
    pub id: String,
    pub variant: String,
    pub seats: u8,
    /// Watching-only clients connected via `?role=spectator`. v2 lobby
    /// snapshots omit this field entirely; the `serde(default)` keeps v2
    /// clients deserializing a v3 `RoomSummary` cleanly (they just see 0).
    #[serde(default)]
    pub spectators: u16,
    pub has_password: bool,
    pub status: RoomStatus,
    /// Whether this room was created with `?hints=1` — i.e., AI hints
    /// (the in-game debug panel + PV overlay) are sanctioned by the
    /// room and visible to BOTH players + spectators. Frozen for the
    /// room's lifetime. v4 lobby snapshots omit this field; the
    /// `serde(default)` keeps v4 clients reading v5 summaries cleanly
    /// (treating the column as `false`).
    #[serde(default)]
    pub hints_allowed: bool,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RoomStatus {
    /// Game has not begun (≤1 seat occupied or ongoing with both seats).
    Lobby,
    /// Both seats occupied and the game is `Ongoing`.
    Playing,
    /// `GameStatus::Won` or `GameStatus::Drawn`. The room still exists but
    /// requires a rematch (or for both clients to disconnect → GC) to be
    /// reusable.
    Finished,
}

/// Host's colour preference when creating a room (chess-net WS or
/// WebRTC `/lan/host`). `Random` resolves to a concrete `Side` at room
/// creation time — once seats are assigned the resolution is frozen.
///
/// Only relevant for xiangqi and banqi with `HouseRules::PREASSIGN_COLORS`.
/// For default-rules banqi the seat-to-colour mapping is settled by the
/// first reveal, not by the host preference.
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HostColor {
    #[default]
    Red,
    Black,
    Random,
}

impl HostColor {
    /// Resolve `Random` against the room's banqi seed (or a fresh
    /// thread-local source) so the room creator always lands on a
    /// concrete seat. Same `seed` always returns the same side, so
    /// chess-net's deterministic-banqi-with-seed paths round-trip
    /// identically.
    pub fn resolve(self, seed: Option<u64>) -> Side {
        match self {
            HostColor::Red => Side::RED,
            HostColor::Black => Side::BLACK,
            HostColor::Random => match seed {
                Some(s) => {
                    use rand::{Rng, SeedableRng};
                    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(s.wrapping_add(0xABCD));
                    if rng.gen::<bool>() {
                        Side::RED
                    } else {
                        Side::BLACK
                    }
                }
                None => {
                    use rand::Rng;
                    if rand::thread_rng().gen::<bool>() {
                        Side::RED
                    } else {
                        Side::BLACK
                    }
                }
            },
        }
    }
}

/// Who is allowed to make the very first reveal in banqi when
/// `HouseRules::PREASSIGN_COLORS` is **off**. Has no effect for
/// xiangqi or for banqi with PREASSIGN_COLORS — in those cases the
/// engine's own first-mover rule (Red moves first) applies.
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FirstFlipper {
    /// No pre-commitment — `state.banqi_awaiting_first_flip` stays
    /// `true` until either seat clicks. Deployment layer attributes
    /// the flip to the actual clicker.
    #[default]
    Either,
    /// Host seat must flip first. Deployment layer initialises
    /// `side_to_move = host_seat`; standard "not your turn" guard
    /// applies to the joiner.
    Host,
    /// Joiner seat must flip first. Deployment layer initialises
    /// `side_to_move = joiner_seat`; standard guard applies to the
    /// host.
    Joiner,
}

/// Host-side game-setup preferences communicated through both
/// deployment paths (chess-net WS URL params on first join; chess-web
/// WebRTC offer envelope JSON). Once the room is created these are
/// frozen for its lifetime — subsequent joiners' params are ignored.
///
/// Serde-clean with all-`default` fields so adding it to the offer
/// envelope is forward-compatible (older envelopes deserialise into
/// the default `RoomConfig` with no host preferences).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomConfig {
    #[serde(default)]
    pub host_color: HostColor,
    #[serde(default)]
    pub first_flipper: FirstFlipper,
}

/// Stable variant string for `RoomSummary.variant`. Uses kebab-case so the
/// JSON looks the same as the chess-tui `--style ascii` / `--style cjk`
/// arguments and the `--house` token list.
pub fn variant_label(rules: &RuleSet) -> &'static str {
    match rules.variant {
        Variant::Xiangqi => {
            if rules.xiangqi_allow_self_check {
                "xiangqi"
            } else {
                "xiangqi-strict"
            }
        }
        Variant::Banqi => "banqi",
        Variant::ThreeKingdomBanqi => "three-kingdom",
    }
}
