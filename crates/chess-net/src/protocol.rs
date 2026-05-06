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
pub const PROTOCOL_VERSION: u16 = 2;

/// Server → client.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    /// Sent once per client right after side assignment. Carries the variant
    /// (so the client knows what to render) plus the initial `PlayerView`.
    Hello { protocol: u16, observer: Side, rules: RuleSet, view: PlayerView },
    /// Sent to every seated client after each committed move.
    Update { view: PlayerView },
    /// Illegal move, room full, malformed message, opponent disconnect, etc.
    /// Non-fatal by default — clients should display and keep the connection.
    Error { message: String },
    /// Pushed on the lobby socket only — initial snapshot on connect, then
    /// again whenever any room's state changes (seat insertion / removal /
    /// game finish / GC). Game sockets never receive this variant.
    Rooms { rooms: Vec<RoomSummary> },
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
    pub has_password: bool,
    pub status: RoomStatus,
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
