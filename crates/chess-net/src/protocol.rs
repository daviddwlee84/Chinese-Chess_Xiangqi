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
use chess_core::rules::RuleSet;
use chess_core::view::PlayerView;
use serde::{Deserialize, Serialize};

/// Bumped whenever the wire schema changes incompatibly.
pub const PROTOCOL_VERSION: u16 = 1;

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
}
