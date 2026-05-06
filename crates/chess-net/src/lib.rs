//! `chess-net` — websocket server + wire protocol for chess-core.
//!
//! MVP: a single binary (`chess-net-server`) hosts one room with one game,
//! accepts the first two ws clients (Red then Black), validates moves
//! against `state.legal_moves()`, and broadcasts per-side `PlayerView` after
//! every committed move. Lobby / matchmaking / time controls / takeback /
//! reconnect / TLS are deferred — see `TODO.md`.

pub mod protocol;
pub mod server;

pub use protocol::{ClientMsg, ServerMsg, PROTOCOL_VERSION};
pub use server::{run, serve};
