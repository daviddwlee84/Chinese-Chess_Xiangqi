//! Transport-agnostic room state machine.
//!
//! Extracted from `server.rs` so the same room logic can be driven by any
//! transport — the existing axum WebSocket server, a future WebRTC
//! DataChannel host running inside a PWA, an in-process test harness, etc.
//! See `backlog/webrtc-lan-pairing.md` Phase 1 for the motivating refactor.
//!
//! The key abstraction is [`PeerId`] — an opaque identifier the transport
//! assigns to each connection. [`Room::apply`] consumes a `(PeerId,
//! ClientMsg)` pair and returns a `Vec<Outbound>` describing exactly which
//! peers should receive which `ServerMsg`s. The transport layer owns the
//! mapping `PeerId -> outbound channel` and fans out accordingly.
//!
//! No `tokio` / `axum` / `futures` dependency — this module compiles for
//! `wasm32-unknown-unknown` so the chess-web PWA can host a room locally
//! over WebRTC DataChannels.

// === module skeleton — section bodies filled in via edits below ===

// SECTION: imports
use std::collections::VecDeque;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_core::state::{GameState, GameStatus, WinReason};
use chess_core::view::PlayerView;

use crate::protocol::{
    variant_label, ChatLine, ClientMsg, RoomStatus, RoomSummary, ServerMsg, PROTOCOL_VERSION,
};
// SECTION: constants

/// Names a server's persistent default room. Kept as a `pub const` so the
/// transport layer can decide whether GC applies (the v1 default room is
/// never GC'd; user-created rooms are).
pub const DEFAULT_ROOM: &str = "main";

/// Hard cap on `?password=` length to keep server log noise bounded.
pub const MAX_PASSWORD_LEN: usize = 64;

/// Hard cap on `:room_id` length; mirrors what the URL extractor accepts.
pub const MAX_ROOM_ID_LEN: usize = 32;

/// Trim chat lines server-side so a noisy client cannot fill the ring.
pub const MAX_CHAT_LEN: usize = 256;

/// Per-room chat ring buffer capacity. Sent verbatim to every new joiner
/// via `ChatHistory`.
pub const CHAT_HISTORY_CAP: usize = 50;

/// Default per-room cap on `?role=spectator` connections. The transport
/// can pick a different value (see `ServeOpts::with_max_spectators`); the
/// `Room` itself takes the cap as an explicit argument so it has no
/// hidden global state.
pub const DEFAULT_MAX_SPECTATORS: usize = 16;
// SECTION: peer + outbound types

/// Opaque per-connection identifier. The `Room` does not allocate these —
/// the transport layer assigns one per accepted connection (e.g. a monotonic
/// counter on the axum server, or a random u64 on the WebRTC host) and is
/// responsible for keeping the `PeerId -> outbound channel` mapping.
///
/// The room only needs `Eq` + `Copy`, never inspects the inner value, and
/// makes no assumptions about uniqueness across rooms — only within a single
/// room's lifetime.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct PeerId(pub u64);

/// Role this peer occupies inside the room.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SeatRole {
    Player(Side),
    Spectator,
}

/// One outbound message the transport must deliver to a specific peer.
///
/// We intentionally pre-project per-peer payloads inside the `Room` (e.g.
/// each seated player gets their own [`PlayerView`] in `Update`) rather
/// than expose a fan-out enum. This keeps the transport glue trivial — it
/// just iterates the returned vec and `send`s each `msg` to `peer` — at
/// the cost of cloning chat lines / non-projected messages once per
/// recipient. Acceptable: rooms cap at 2 seats + a handful of spectators.
///
/// `PartialEq` is for tests only — `ServerMsg` doesn't itself implement
/// `Eq` (some payloads carry `f32` win-rates) so `Outbound` cannot derive
/// `Eq`. Tests use the field-by-field check via `assert!(matches!(..))`
/// patterns.
#[derive(Clone, Debug)]
pub struct Outbound {
    pub peer: PeerId,
    pub msg: ServerMsg,
}

/// Result of attempting to seat a peer in a room.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum JoinError {
    /// Both seats already taken.
    RoomFull,
    /// Spectator slots full (parameter to [`Room::join_spectator`]).
    SpectatorCapReached,
}
// SECTION: room struct

/// Authoritative per-room state plus its peer roster.
///
/// Replaces the pre-refactor `server::RoomState`, with two changes:
///
/// 1. Senders are out — peers are tracked by [`PeerId`] only. The transport
///    keeps the `PeerId -> Sender` map separately and consumes the
///    [`Outbound`] vec returned from `apply` / `join_*` / `leave`.
/// 2. Logging side-effects (`eprintln!`) moved to the transport layer
///    so this module compiles for `wasm32-unknown-unknown` without dragging
///    in stderr semantics.
///
/// Locking remains the transport's responsibility (the axum server wraps
/// each `Room` in `Arc<Mutex<...>>`, the WebRTC host can use a `RefCell`
/// because it's single-threaded). The `Room` API is `&mut self` for any
/// state-changing operation.
pub struct Room {
    state: GameState,
    /// (side, peer-id). Empty until first connect; capped at 2.
    seats: Vec<(Side, PeerId)>,
    /// Read-only watchers connected via `?role=spectator`. Cap is enforced
    /// by [`Room::join_spectator`]'s `max` argument, not stored on the
    /// room itself.
    spectators: Vec<PeerId>,
    /// Sides that have requested a rematch since the last reset. Cleared
    /// every time the game resets; mutual consent triggers reset.
    rematch: Vec<Side>,
    /// Friend-list lock. `None` = open. Set by the first joiner from the
    /// `?password=` query string (or equivalent in non-WS transports) and
    /// never mutated afterwards. **Plain text is intentional** — this is
    /// a "type the secret to enter the game with the right friends"
    /// mechanism, not a security boundary.
    password: Option<String>,
    /// AI-hint sanction. `false` = clients must NOT mount the AI debug /
    /// hint panel against this room. Set by the first joiner and frozen
    /// for the room's lifetime (same pattern as `password`). Closes the
    /// previous client-only `?debug=1` cheat hole.
    hints_allowed: bool,
    /// In-memory ring buffer of recent chat lines (cap [`CHAT_HISTORY_CAP`]).
    /// Sent verbatim to every new joiner via `ChatHistory`.
    chat: VecDeque<ChatLine>,
}
// SECTION: room construction + simple accessors

impl Room {
    /// Create a fresh room. `password` and `hints_allowed` are set once and
    /// frozen — the [`Room::join_player`] / [`Room::join_spectator`]
    /// callers do their own password-equality check before insertion (the
    /// outer transport already gates on the URL `?password=` param).
    pub fn new(rules: RuleSet, password: Option<String>, hints_allowed: bool) -> Self {
        Self {
            state: GameState::new(rules),
            seats: Vec::with_capacity(2),
            spectators: Vec::new(),
            rematch: Vec::with_capacity(2),
            password,
            hints_allowed,
            chat: VecDeque::with_capacity(CHAT_HISTORY_CAP),
        }
    }

    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    pub fn hints_allowed(&self) -> bool {
        self.hints_allowed
    }

    pub fn rules(&self) -> &RuleSet {
        &self.state.rules
    }

    pub fn seat_count(&self) -> usize {
        self.seats.len()
    }

    pub fn spectator_count(&self) -> usize {
        self.spectators.len()
    }

    /// `true` when the room has no seated players AND no spectators. The
    /// transport uses this to GC user-created rooms (the v1 default room
    /// `"main"` is exempt).
    pub fn is_empty(&self) -> bool {
        self.seats.is_empty() && self.spectators.is_empty()
    }

    /// Side that would be assigned to the next [`Room::join_player`] call,
    /// or `None` if the room is full. Useful for transports that want to
    /// preview "joinable as Red?" without mutating.
    pub fn next_seat(&self) -> Option<Side> {
        match self.seats.len() {
            0 => Some(Side::RED),
            1 => {
                let taken = self.seats[0].0;
                Some(if taken == Side::RED { Side::BLACK } else { Side::RED })
            }
            _ => None,
        }
    }

    pub fn summary(&self, id: &str) -> RoomSummary {
        let status = match self.state.status {
            GameStatus::Won { .. } | GameStatus::Drawn { .. } => RoomStatus::Finished,
            GameStatus::Ongoing => {
                if self.seats.len() >= 2 {
                    RoomStatus::Playing
                } else {
                    RoomStatus::Lobby
                }
            }
        };
        RoomSummary {
            id: id.to_string(),
            variant: variant_label(&self.state.rules).to_string(),
            seats: self.seats.len() as u8,
            spectators: self.spectators.len() as u16,
            has_password: self.password.is_some(),
            status,
            hints_allowed: self.hints_allowed,
        }
    }

    fn chat_history(&self) -> Vec<ChatLine> {
        self.chat.iter().cloned().collect()
    }

    /// What role is this peer in? `None` = unknown peer (e.g. already
    /// disconnected, or never joined this room).
    fn role_of(&self, peer: PeerId) -> Option<SeatRole> {
        if let Some((side, _)) = self.seats.iter().find(|(_, p)| *p == peer) {
            return Some(SeatRole::Player(*side));
        }
        if self.spectators.contains(&peer) {
            return Some(SeatRole::Spectator);
        }
        None
    }
}
// SECTION: join_player

impl Room {
    /// Seat a peer as a player. Returns the assigned [`Side`] plus the
    /// initial outbound list (Hello + ChatHistory) the transport should
    /// deliver to this peer immediately.
    ///
    /// Errors with [`JoinError::RoomFull`] when both seats are taken; the
    /// transport is expected to send `Error{"room full"}` (or transport
    /// equivalent) and close the connection.
    pub fn join_player(&mut self, peer: PeerId) -> Result<(Side, Vec<Outbound>), JoinError> {
        let side = self.next_seat().ok_or(JoinError::RoomFull)?;
        let view = PlayerView::project(&self.state, side);
        let out = vec![
            Outbound {
                peer,
                msg: ServerMsg::Hello {
                    protocol: PROTOCOL_VERSION,
                    observer: side,
                    rules: self.state.rules.clone(),
                    view,
                    hints_allowed: self.hints_allowed,
                },
            },
            Outbound { peer, msg: ServerMsg::ChatHistory { lines: self.chat_history() } },
        ];
        self.seats.push((side, peer));
        Ok((side, out))
    }
}
// SECTION: join_spectator

impl Room {
    /// Seat a peer as a read-only spectator. `max` is the cap the transport
    /// wants to enforce (typically [`DEFAULT_MAX_SPECTATORS`]); the room
    /// stores no global cap of its own. Returns the initial outbound list
    /// (Spectating + ChatHistory).
    ///
    /// Spectators see the board from RED's POV — `PlayerView::project` is
    /// leak-safe so banqi hidden tiles stay opaque. Three-kingdom would
    /// need a neutral projection later (out of scope).
    pub fn join_spectator(&mut self, peer: PeerId, max: usize) -> Result<Vec<Outbound>, JoinError> {
        if self.spectators.len() >= max {
            return Err(JoinError::SpectatorCapReached);
        }
        let view = PlayerView::project(&self.state, Side::RED);
        let out = vec![
            Outbound {
                peer,
                msg: ServerMsg::Spectating {
                    protocol: PROTOCOL_VERSION,
                    rules: self.state.rules.clone(),
                    view,
                    hints_allowed: self.hints_allowed,
                },
            },
            Outbound { peer, msg: ServerMsg::ChatHistory { lines: self.chat_history() } },
        ];
        self.spectators.push(peer);
        Ok(out)
    }
}
// SECTION: leave

impl Room {
    /// Remove a peer (player or spectator). Returns outbound messages the
    /// transport should fire — for example, mid-game opponent-disconnect
    /// notifications to surviving seats. The peer's own outbound channel
    /// is never targeted (the connection is closing).
    ///
    /// No-op when `peer` isn't recognised (already left, or never joined).
    pub fn leave(&mut self, peer: PeerId) -> Vec<Outbound> {
        let role = match self.role_of(peer) {
            Some(r) => r,
            None => return Vec::new(),
        };
        let mut out = Vec::new();
        match role {
            SeatRole::Player(side) => {
                self.seats.retain(|(_, p)| *p != peer);
                self.rematch.retain(|s| *s != side);
                if matches!(self.state.status, GameStatus::Ongoing) && !self.seats.is_empty() {
                    for (_, surviving) in &self.seats {
                        out.push(Outbound {
                            peer: *surviving,
                            msg: ServerMsg::Error { message: "opponent disconnected".into() },
                        });
                    }
                }
            }
            SeatRole::Spectator => {
                self.spectators.retain(|p| *p != peer);
            }
        }
        out
    }
}
// SECTION: apply (entry point)

impl Room {
    /// Process an incoming `ClientMsg` from `peer`. Returns the outbound
    /// fan-out the transport should deliver. Unknown peers (not seated,
    /// not spectating) get a single `Error` reply addressed to themselves
    /// — the transport is free to log + close, or just discard.
    ///
    /// All seat / spectator gating (e.g. "spectators cannot move") happens
    /// inside this function; the transport does not need to inspect the
    /// message kind.
    pub fn apply(&mut self, peer: PeerId, msg: ClientMsg) -> Vec<Outbound> {
        let role = match self.role_of(peer) {
            Some(r) => r,
            None => {
                return vec![Outbound {
                    peer,
                    msg: ServerMsg::Error { message: "unknown peer".into() },
                }];
            }
        };
        let seat = match role {
            SeatRole::Player(s) => Some(s),
            SeatRole::Spectator => None,
        };
        let mut out = Vec::new();
        match msg {
            ClientMsg::Move { mv } => match seat {
                Some(s) => {
                    if !matches!(self.state.status, GameStatus::Ongoing) {
                        out.push(Outbound {
                            peer,
                            msg: ServerMsg::Error {
                                message: "game is over — press 'n' to request a rematch".into(),
                            },
                        });
                    } else {
                        self.process_move(s, mv, peer, &mut out);
                    }
                }
                None => out.push(Outbound {
                    peer,
                    msg: ServerMsg::Error { message: "spectators cannot move".into() },
                }),
            },
            ClientMsg::Resign => match seat {
                Some(s) => {
                    if !matches!(self.state.status, GameStatus::Ongoing) {
                        out.push(Outbound {
                            peer,
                            msg: ServerMsg::Error { message: "game is over".into() },
                        });
                    } else {
                        let winner = if s == Side::RED { Side::BLACK } else { Side::RED };
                        self.state.status =
                            GameStatus::Won { winner, reason: WinReason::Resignation };
                        self.broadcast_update(&mut out);
                    }
                }
                None => out.push(Outbound {
                    peer,
                    msg: ServerMsg::Error { message: "spectators cannot resign".into() },
                }),
            },
            ClientMsg::Rematch => match seat {
                Some(s) => self.process_rematch(s, peer, &mut out),
                None => out.push(Outbound {
                    peer,
                    msg: ServerMsg::Error { message: "spectators cannot request a rematch".into() },
                }),
            },
            ClientMsg::Chat { text } => match seat {
                Some(s) => self.process_chat(s, text, peer, &mut out),
                None => out.push(Outbound {
                    peer,
                    msg: ServerMsg::Error { message: "spectators cannot chat".into() },
                }),
            },
            ClientMsg::ListRooms => out.push(Outbound {
                peer,
                msg: ServerMsg::Error {
                    message: "ListRooms is a lobby-only message; subscribe via /lobby".into(),
                },
            }),
        }
        out
    }
}
// SECTION: process_move

impl Room {
    fn process_move(&mut self, seat: Side, mv: Move, from: PeerId, out: &mut Vec<Outbound>) {
        if self.state.side_to_move != seat {
            out.push(Outbound {
                peer: from,
                msg: ServerMsg::Error { message: "not your turn".into() },
            });
            return;
        }
        match self.state.make_move(&mv) {
            Ok(()) => {
                self.state.refresh_status();
                self.broadcast_update(out);
            }
            Err(e) => {
                out.push(Outbound {
                    peer: from,
                    msg: ServerMsg::Error { message: format!("illegal move: {e}") },
                });
            }
        }
    }
}
// SECTION: process_resign — inlined into `apply` (3-line state mutation +
// broadcast_update); kept here only as a section marker for the section list
// at the top of the file.

// SECTION: process_rematch

impl Room {
    fn process_rematch(&mut self, seat: Side, from: PeerId, out: &mut Vec<Outbound>) {
        if matches!(self.state.status, GameStatus::Ongoing) {
            out.push(Outbound {
                peer: from,
                msg: ServerMsg::Error {
                    message: "Game still in progress — finish or resign first.".into(),
                },
            });
            return;
        }
        if self.seats.len() < 2 {
            out.push(Outbound {
                peer: from,
                msg: ServerMsg::Error {
                    message: "No opponent connected — can't start a rematch.".into(),
                },
            });
            return;
        }
        if !self.rematch.contains(&seat) {
            self.rematch.push(seat);
        }
        let want_seats: Vec<Side> = self.seats.iter().map(|(s, _)| *s).collect();
        let all_ready = want_seats.iter().all(|s| self.rematch.contains(s));
        if all_ready {
            let rules = self.state.rules.clone();
            self.state = GameState::new(rules);
            self.rematch.clear();
            // Re-Hello every seated player from their own POV.
            for (side, peer) in &self.seats {
                let view = PlayerView::project(&self.state, *side);
                out.push(Outbound {
                    peer: *peer,
                    msg: ServerMsg::Hello {
                        protocol: PROTOCOL_VERSION,
                        observer: *side,
                        rules: self.state.rules.clone(),
                        view,
                        hints_allowed: self.hints_allowed,
                    },
                });
            }
            // Re-Spectating every spectator (one shared projection from RED).
            if !self.spectators.is_empty() {
                let view = PlayerView::project(&self.state, Side::RED);
                for peer in &self.spectators {
                    out.push(Outbound {
                        peer: *peer,
                        msg: ServerMsg::Spectating {
                            protocol: PROTOCOL_VERSION,
                            rules: self.state.rules.clone(),
                            view: view.clone(),
                            hints_allowed: self.hints_allowed,
                        },
                    });
                }
            }
        } else {
            // One side has requested; nudge both seats with the appropriate copy.
            for (s, peer) in &self.seats {
                let msg = if *s == seat {
                    ServerMsg::Error {
                        message: "Rematch requested. Waiting for opponent…".into()
                    }
                } else {
                    ServerMsg::Error {
                        message: "Opponent wants a rematch. Press 'n' to accept.".into(),
                    }
                };
                out.push(Outbound { peer: *peer, msg });
            }
        }
    }
}
// SECTION: process_chat

impl Room {
    fn process_chat(
        &mut self,
        from_side: Side,
        raw: String,
        from: PeerId,
        out: &mut Vec<Outbound>,
    ) {
        let cleaned: String = raw
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
            .collect();
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            out.push(Outbound {
                peer: from,
                msg: ServerMsg::Error { message: "chat message is empty".into() },
            });
            return;
        }
        let mut text: String = trimmed.chars().take(MAX_CHAT_LEN).collect();
        text.shrink_to_fit();
        let line = ChatLine { from: from_side, text, ts_ms: now_ms() };
        if self.chat.len() >= CHAT_HISTORY_CAP {
            self.chat.pop_front();
        }
        self.chat.push_back(line.clone());
        // Fan a single Chat message out to every connection (seats +
        // spectators). Used for `Chat` where every recipient should see the
        // same payload — `Update` uses `broadcast_update` instead because
        // seats need per-side projections.
        let msg = ServerMsg::Chat { line };
        for (_, peer) in &self.seats {
            out.push(Outbound { peer: *peer, msg: msg.clone() });
        }
        for peer in &self.spectators {
            out.push(Outbound { peer: *peer, msg: msg.clone() });
        }
    }
}

/// Unix milliseconds source. Native uses `SystemTime`; wasm32 uses
/// `js_sys::Date::now()` because `SystemTime::now()` panics on wasm
/// with "time not implemented on this platform" — discovered when
/// `host_room.rs` started running `Room` in-browser for LAN play and
/// the first chat message panicked the entire fanout (silently
/// dropping the chat broadcast). Returns `0` only on the impossible
/// `SystemTimeError` (CI clock before the unix epoch).
fn now_ms() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        // `Date::now()` returns f64 millis since unix epoch. Cast to
        // u64 — within the next ~285,000,000 years this won't overflow.
        js_sys::Date::now() as u64
    }
}
// SECTION: broadcast helpers

impl Room {
    /// Push one `Update` per recipient: each seated player gets a
    /// per-side-projected `PlayerView`; spectators share a single RED-POV
    /// projection (banqi hidden tiles stay opaque per `PlayerView::project`'s
    /// no-leak invariant).
    fn broadcast_update(&self, out: &mut Vec<Outbound>) {
        for (side, peer) in &self.seats {
            let view = PlayerView::project(&self.state, *side);
            out.push(Outbound { peer: *peer, msg: ServerMsg::Update { view } });
        }
        if !self.spectators.is_empty() {
            let view = PlayerView::project(&self.state, Side::RED);
            for peer in &self.spectators {
                out.push(Outbound { peer: *peer, msg: ServerMsg::Update { view: view.clone() } });
            }
        }
    }
}
// SECTION: validation helpers (room id / password)

/// Cheap pre-flight check on a `:room_id` URL segment. The transport layer
/// is expected to reject + close before constructing a `Room` when this
/// returns `false`.
pub fn valid_room_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ROOM_ID_LEN
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Cheap pre-flight check on a `?password=` query value.
pub fn valid_password(pw: &str) -> bool {
    !pw.is_empty() && pw.len() <= MAX_PASSWORD_LEN && pw.chars().all(|c| !c.is_control())
}

/// Parse the `?hints=` query value (case-insensitive `1` / `true` / `on` /
/// `yes` → `true`). The transport layer extracts the raw string from its
/// URL/handshake; we centralise the string-to-bool mapping here so every
/// transport agrees.
pub fn parse_hints_param(raw: Option<&str>) -> bool {
    matches!(raw, Some(v) if matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "on" | "yes"))
}
// SECTION: tests

#[cfg(test)]
mod tests {
    //! Direct `Room` driver tests — no sockets, no tokio, no axum. These
    //! complement the existing `tests/server_smoke.rs` etc. (which exercise
    //! the same logic through the WS transport) and run instantly.

    use super::*;
    use chess_core::rules::RuleSet;

    fn xiangqi_room() -> Room {
        Room::new(RuleSet::xiangqi(), None, false)
    }

    #[test]
    fn join_player_emits_hello_then_chat_history_for_red_first() {
        let mut room = xiangqi_room();
        let red = PeerId(1);
        let (side, out) = room.join_player(red).unwrap();
        assert_eq!(side, Side::RED);
        assert_eq!(out.len(), 2);
        assert!(matches!(out[0].msg, ServerMsg::Hello { observer: Side::RED, .. }));
        assert!(matches!(out[1].msg, ServerMsg::ChatHistory { ref lines } if lines.is_empty()));
    }

    #[test]
    fn second_join_gets_black_third_gets_room_full() {
        let mut room = xiangqi_room();
        let (s1, _) = room.join_player(PeerId(1)).unwrap();
        let (s2, _) = room.join_player(PeerId(2)).unwrap();
        assert_eq!(s1, Side::RED);
        assert_eq!(s2, Side::BLACK);
        assert!(matches!(room.join_player(PeerId(3)), Err(JoinError::RoomFull)));
    }

    #[test]
    fn spectator_cap_enforced() {
        let mut room = xiangqi_room();
        room.join_spectator(PeerId(10), 2).unwrap();
        room.join_spectator(PeerId(11), 2).unwrap();
        assert!(matches!(room.join_spectator(PeerId(12), 2), Err(JoinError::SpectatorCapReached)));
    }

    #[test]
    fn move_from_wrong_seat_returns_error_only_to_sender() {
        let mut room = xiangqi_room();
        let red = PeerId(1);
        let black = PeerId(2);
        room.join_player(red).unwrap();
        room.join_player(black).unwrap();
        // Black tries to move first — red is to-move at start. The
        // side-to-move guard runs BEFORE make_move legality, so we can
        // hand in any `Move` value: process_move bails with
        // "not your turn" without inspecting it.
        let bogus = chess_core::moves::Move::Step {
            from: chess_core::coord::Square(0),
            to: chess_core::coord::Square(1),
        };
        let out = room.apply(black, ClientMsg::Move { mv: bogus });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].peer, black);
        assert!(matches!(
            &out[0].msg,
            ServerMsg::Error { message } if message.contains("not your turn")
        ));
    }

    #[test]
    fn legal_move_from_red_broadcasts_update_to_both_seats() {
        let mut room = xiangqi_room();
        let red = PeerId(1);
        let black = PeerId(2);
        room.join_player(red).unwrap();
        room.join_player(black).unwrap();
        let view = PlayerView::project(&GameState::new(RuleSet::xiangqi()), Side::RED);
        let mv = view
            .legal_moves
            .iter()
            .find(|m| matches!(m, chess_core::moves::Move::Step { .. }))
            .cloned()
            .expect("xiangqi opening has step moves");
        let out = room.apply(red, ClientMsg::Move { mv });
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|o| matches!(o.msg, ServerMsg::Update { .. })));
        assert_eq!(out.iter().filter(|o| o.peer == red).count(), 1);
        assert_eq!(out.iter().filter(|o| o.peer == black).count(), 1);
    }

    #[test]
    fn resign_marks_won_and_broadcasts_to_both_seats() {
        let mut room = xiangqi_room();
        let red = PeerId(1);
        let black = PeerId(2);
        room.join_player(red).unwrap();
        room.join_player(black).unwrap();
        let out = room.apply(red, ClientMsg::Resign);
        assert_eq!(out.len(), 2);
        for o in &out {
            match &o.msg {
                ServerMsg::Update { view } => {
                    assert!(matches!(
                        view.status,
                        GameStatus::Won { winner: Side::BLACK, reason: WinReason::Resignation }
                    ));
                }
                other => panic!("expected Update, got {other:?}"),
            }
        }
    }

    #[test]
    fn chat_from_player_fans_out_to_seats_and_spectators() {
        let mut room = xiangqi_room();
        let red = PeerId(1);
        let black = PeerId(2);
        let watcher = PeerId(3);
        room.join_player(red).unwrap();
        room.join_player(black).unwrap();
        room.join_spectator(watcher, 4).unwrap();
        let out = room.apply(red, ClientMsg::Chat { text: "hello".into() });
        assert_eq!(out.len(), 3);
        for o in &out {
            assert!(matches!(&o.msg, ServerMsg::Chat { line } if line.text == "hello"));
        }
    }

    #[test]
    fn chat_from_spectator_rejected_only_to_sender() {
        let mut room = xiangqi_room();
        let red = PeerId(1);
        let watcher = PeerId(2);
        room.join_player(red).unwrap();
        room.join_spectator(watcher, 4).unwrap();
        let out = room.apply(watcher, ClientMsg::Chat { text: "hi".into() });
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].peer, watcher);
        assert!(matches!(&out[0].msg, ServerMsg::Error { .. }));
    }

    #[test]
    fn chat_history_capped_at_50() {
        let mut room = xiangqi_room();
        let red = PeerId(1);
        room.join_player(red).unwrap();
        for i in 0..(CHAT_HISTORY_CAP + 5) {
            let _ = room.apply(red, ClientMsg::Chat { text: format!("msg{i}") });
        }
        let history = room.chat_history();
        assert_eq!(history.len(), CHAT_HISTORY_CAP);
        // Oldest 5 should have been dropped; first surviving line is "msg5".
        assert_eq!(history[0].text, "msg5");
    }

    #[test]
    fn rematch_requires_both_seats_then_resets_state() {
        let mut room = xiangqi_room();
        let red = PeerId(1);
        let black = PeerId(2);
        room.join_player(red).unwrap();
        room.join_player(black).unwrap();
        let _ = room.apply(red, ClientMsg::Resign);
        let out = room.apply(red, ClientMsg::Rematch);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|o| matches!(o.msg, ServerMsg::Error { .. })));
        let out = room.apply(black, ClientMsg::Rematch);
        assert_eq!(out.len(), 2);
        for o in &out {
            assert!(matches!(o.msg, ServerMsg::Hello { .. }));
        }
        assert!(matches!(room.state.status, GameStatus::Ongoing));
    }

    #[test]
    fn leave_during_ongoing_notifies_surviving_seat() {
        let mut room = xiangqi_room();
        let red = PeerId(1);
        let black = PeerId(2);
        room.join_player(red).unwrap();
        room.join_player(black).unwrap();
        let out = room.leave(red);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].peer, black);
        assert!(matches!(
            &out[0].msg,
            ServerMsg::Error { message } if message.contains("opponent disconnected")
        ));
    }

    #[test]
    fn leave_after_game_over_does_not_notify() {
        let mut room = xiangqi_room();
        let red = PeerId(1);
        let black = PeerId(2);
        room.join_player(red).unwrap();
        room.join_player(black).unwrap();
        let _ = room.apply(red, ClientMsg::Resign);
        let out = room.leave(red);
        assert!(out.is_empty(), "no opponent-disconnect notice after game ended");
    }

    #[test]
    fn unknown_peer_apply_returns_unknown_peer_error() {
        let mut room = xiangqi_room();
        let stranger = PeerId(99);
        let out = room.apply(stranger, ClientMsg::Resign);
        assert_eq!(out.len(), 1);
        assert!(matches!(
            &out[0].msg,
            ServerMsg::Error { message } if message.contains("unknown peer")
        ));
    }

    #[test]
    fn list_rooms_on_game_socket_is_polite_error() {
        let mut room = xiangqi_room();
        let red = PeerId(1);
        room.join_player(red).unwrap();
        let out = room.apply(red, ClientMsg::ListRooms);
        assert_eq!(out.len(), 1);
        assert!(matches!(
            &out[0].msg,
            ServerMsg::Error { message } if message.contains("lobby-only")
        ));
    }

    #[test]
    fn summary_reflects_seats_spectators_status() {
        let mut room = xiangqi_room();
        let s = room.summary("foo");
        assert_eq!(s.id, "foo");
        assert_eq!(s.seats, 0);
        assert_eq!(s.spectators, 0);
        assert_eq!(s.status, RoomStatus::Lobby);
        room.join_player(PeerId(1)).unwrap();
        room.join_player(PeerId(2)).unwrap();
        room.join_spectator(PeerId(3), 4).unwrap();
        let s = room.summary("foo");
        assert_eq!(s.seats, 2);
        assert_eq!(s.spectators, 1);
        assert_eq!(s.status, RoomStatus::Playing);
        let _ = room.apply(PeerId(1), ClientMsg::Resign);
        assert_eq!(room.summary("foo").status, RoomStatus::Finished);
    }

    #[test]
    fn validation_helpers() {
        assert!(valid_room_id("foo"));
        assert!(valid_room_id("a-b_c-1"));
        assert!(!valid_room_id(""));
        assert!(!valid_room_id("with space"));
        assert!(!valid_room_id(&"a".repeat(MAX_ROOM_ID_LEN + 1)));

        assert!(valid_password("hunter2"));
        assert!(!valid_password(""));
        assert!(!valid_password(&"a".repeat(MAX_PASSWORD_LEN + 1)));
        assert!(!valid_password("a\nb")); // control char rejected

        assert!(parse_hints_param(Some("1")));
        assert!(parse_hints_param(Some("TRUE")));
        assert!(!parse_hints_param(Some("0")));
        assert!(!parse_hints_param(None));
    }

    #[test]
    fn next_seat_and_is_empty() {
        let mut room = xiangqi_room();
        assert!(room.is_empty());
        assert_eq!(room.next_seat(), Some(Side::RED));
        room.join_player(PeerId(1)).unwrap();
        assert_eq!(room.next_seat(), Some(Side::BLACK));
        assert!(!room.is_empty());
        room.join_player(PeerId(2)).unwrap();
        assert_eq!(room.next_seat(), None);
        room.leave(PeerId(1));
        room.leave(PeerId(2));
        room.join_spectator(PeerId(3), 4).unwrap();
        assert!(!room.is_empty());
        room.leave(PeerId(3));
        assert!(room.is_empty());
    }
}
