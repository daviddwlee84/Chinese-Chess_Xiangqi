//! Transport-agnostic chess-net pump for the chess-web client side.
//!
//! Today's only implementation is [`ws::WsTransport`] (gloo-net WebSocket
//! against a chess-net server). Phase 3 of `backlog/webrtc-lan-pairing.md`
//! will add a `webrtc::WebRtcTransport` (RtcDataChannel as joiner) so the
//! same play / lobby pages can talk to either a hosted chess-net or a
//! peer's PWA over LAN — gated only on which factory function the route
//! decides to call.
//!
//! Object-safety: [`Transport`] only takes `&self` and returns `bool`;
//! no associated types, no generics. Consumers store
//! `Rc<dyn Transport>` and clone it freely into Leptos closures.

// === module skeleton — section bodies filled in via edits below ===

// SECTION: imports
use std::rc::Rc;

use chess_net::protocol::{ClientMsg, ServerMsg};
use leptos::ReadSignal;
// SECTION: ConnState enum

/// Coarse state of a chess-net transport connection. Pages render different
/// banners + disable inputs based on this.
///
/// Order is significant: implementations should only ever step
/// `Connecting` → `Open` → (`Closed` | `Error`); we never recover from
/// `Closed`/`Error` in the same `Session` (auto-reconnect is a separate
/// follow-up — see `backlog/web-ws-reconnect.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Open,
    Closed,
    Error,
}
// SECTION: Transport trait

/// Bidirectional channel of chess-net protocol frames.
///
/// Object-safe: only one method, `&self` receiver, primitive return.
/// Implementations live in submodules (`transport::ws`, future
/// `transport::webrtc`). Consumers hold `Rc<dyn Transport>` so the
/// concrete transport can change without rippling through the page code.
///
/// `send` returns `false` when the underlying write pump has shut down
/// (e.g. WebSocket closed, DataChannel `closed`). Callers should treat
/// that as "drop this attempt"; the [`Session::state`] signal will move
/// to `Closed` / `Error` shortly after.
pub trait Transport {
    fn send(&self, msg: ClientMsg) -> bool;
}
// SECTION: Session bundle

/// One open chess-net transport plus the two Leptos signals every page
/// needs: the latest `ServerMsg` (latched, not a stream), and the coarse
/// connection state.
///
/// Returned by every transport factory (`ws::connect`, future
/// `webrtc::connect`). Pages destructure / clone its fields freely:
/// `handle` is `Rc<dyn Transport>` so cloning is cheap; `incoming` and
/// `state` are Leptos `ReadSignal`s which are `Copy`.
#[derive(Clone)]
pub struct Session {
    pub handle: Rc<dyn Transport>,
    pub incoming: ReadSignal<Option<ServerMsg>>,
    pub state: ReadSignal<ConnState>,
}

impl Session {
    /// Convenience: `session.send(msg)` instead of `session.handle.send(msg)`.
    /// Currently unused inside chess-web — pages clone `handle` into Leptos
    /// closures rather than holding the whole `Session` — but kept for
    /// straight-line-call sites in tests and future Phase 4 host code.
    #[allow(dead_code)]
    pub fn send(&self, msg: ClientMsg) -> bool {
        self.handle.send(msg)
    }
}
// SECTION: submodules

pub mod ws;
