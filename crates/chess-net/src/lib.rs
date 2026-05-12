//! `chess-net` — websocket server + wire protocol for chess-core.
//!
//! Two consumer shapes:
//!
//! * Default features (`server`) → axum-based multi-room ws server +
//!   protocol types. This is what the `chess-net-server` binary and any
//!   native client (`chess-tui --connect`) build against.
//! * `default-features = false` → protocol types + transport-agnostic
//!   `room` state machine, no axum / tokio. chess-web (wasm32) consumes
//!   the crate this way so the WASM build does not pull in server-only
//!   deps; a future WebRTC host inside the PWA can drive the `room`
//!   module directly. See `backlog/webrtc-lan-pairing.md`.
//!
//! Routes (server feature):
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
pub mod room;
#[cfg(feature = "server")]
pub mod server;

pub use protocol::{
    variant_label, ChatLine, ClientMsg, RoomStatus, RoomSummary, ServerMsg, PROTOCOL_VERSION,
};
pub use room::{
    parse_hints_param, valid_password, valid_room_id, JoinError, Outbound, PeerId, Room, SeatRole,
    CHAT_HISTORY_CAP, DEFAULT_MAX_SPECTATORS, DEFAULT_ROOM, MAX_CHAT_LEN, MAX_PASSWORD_LEN,
    MAX_ROOM_ID_LEN,
};
#[cfg(feature = "server")]
pub use server::{run, run_with, serve, serve_with, ServeOpts};
