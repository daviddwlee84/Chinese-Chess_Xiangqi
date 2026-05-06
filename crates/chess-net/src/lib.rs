//! `chess-net` — websocket server + wire protocol for chess-core.
//!
//! Multi-room ws server: one binary (`chess-net-server`) hosts many
//! concurrent rooms keyed by string id, each with its own authoritative
//! `GameState`. Routes:
//!
//!   GET /             → upgrade into default room "main" (back-compat, v1)
//!   GET /ws           → upgrade into default room "main" (back-compat, v1)
//!   GET /ws/<room-id> → upgrade into named room (auto-created on first join)
//!   GET /lobby        → subscribe to live `Rooms` pushes
//!   GET /rooms        → JSON snapshot of the room list (curl/debug only)
//!
//! Optional per-room password via `?password=<secret>` query string. The
//! first joiner sets the lock; subsequent joiners with the wrong password
//! get `Error{"bad password"}` and are dropped before `Hello`.
//!
//! Time controls / spectators / reconnect / TLS / mixed-variant rooms are
//! deferred — see `TODO.md`.

pub mod protocol;
pub mod server;

pub use protocol::{
    variant_label, ClientMsg, RoomStatus, RoomSummary, ServerMsg, PROTOCOL_VERSION,
};
pub use server::{run, serve};
