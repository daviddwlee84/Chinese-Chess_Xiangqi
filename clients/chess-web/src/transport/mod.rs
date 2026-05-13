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
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use chess_net::protocol::{ClientMsg, ServerMsg};
use leptos::{create_signal, ReadSignal, SignalGet, SignalUpdate, WriteSignal};
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
// SECTION: Incoming queue + tick signal

/// FIFO queue of unprocessed `ServerMsg`s plus a Leptos tick signal that
/// fires once per push.
///
/// Why this design rather than `RwSignal<Vec<ServerMsg>>`?
///
/// Earlier shapes either:
///   * Used `WriteSignal<Option<ServerMsg>>` — synchronous double-set
///     (host-room emits `Hello` + `ChatHistory` back-to-back from a
///     fresh `Room::join_player`) silently dropped the first message
///     because Leptos batches synchronous sets and the effect fires once
///     reading the LAST value. Symptom: page stuck on "Awaiting seat
///     assignment".
///   * Used `RwSignal<Vec<ServerMsg>>` with `incoming.set(Vec::new())`
///     at end of the drain effect — STILL had a race: a push that
///     arrived between the start of the drain and the clear got
///     overwritten by the `set(Vec::new())`. Symptom: post-mount
///     messages (joiner's `Hello` arriving via DC after the effect
///     first ran with an empty queue, host's own move's `Update`
///     after the initial drain) silently dropped.
///
/// The fix is to separate the queue (a plain `VecDeque` outside Leptos's
/// reactivity) from the notification (a monotonic `u32` counter signal).
/// Pushers grab `borrow_mut`, append, increment the counter — never
/// truncating. Drainers subscribe to the counter, then drain everything
/// in the queue. New pushes during a drain go onto the same queue (after
/// the borrow is released — JS is single-threaded so the push physically
/// happens after the drain returns), and the next counter increment
/// re-fires the effect to drain them.
///
/// `Incoming` is `Clone` (cheap — `Rc` + signal copies); the queue is
/// shared mutable state but JS's single-threaded event loop guarantees
/// no concurrent borrows in practice.
#[derive(Clone)]
pub struct Incoming {
    queue: Rc<RefCell<VecDeque<ServerMsg>>>,
    tick: ReadSignal<u32>,
    set_tick: WriteSignal<u32>,
}

impl Incoming {
    /// Construct a fresh empty queue with its tick signal.
    ///
    /// Must be called inside a Leptos owner context (any `#[component]`
    /// body, or anywhere `create_signal` is legal).
    pub fn new() -> Self {
        let (tick, set_tick) = create_signal(0u32);
        Self { queue: Rc::new(RefCell::new(VecDeque::new())), tick, set_tick }
    }

    /// Append a message and notify subscribers.
    ///
    /// Safe to call from any non-reactive context (DC `onmessage`
    /// callbacks, WS read-pump tasks, the host's local fanout). The
    /// counter increment uses `wrapping_add` so the value can't
    /// overflow into a panic at u32::MAX (would take >4 billion
    /// messages on a single session — physically impossible for a
    /// chess game).
    pub fn push(&self, msg: ServerMsg) {
        self.queue.borrow_mut().push_back(msg);
        self.set_tick.update(|n| *n = n.wrapping_add(1));
    }

    /// Subscribe to the tick signal, then drain ALL pending messages,
    /// invoking `f` on each in arrival order.
    ///
    /// Intended for use inside `create_effect`:
    /// ```ignore
    /// create_effect(move |_| {
    ///     incoming.drain(|msg| match msg { ServerMsg::Hello { .. } => ... });
    /// });
    /// ```
    ///
    /// The `tick.get()` at the start registers this effect as a
    /// subscriber. Subsequent `push`es will re-trigger the effect.
    /// The drain reads ALL queued messages, so a single re-run after
    /// multiple pushes still processes everything in order.
    pub fn drain<F: FnMut(ServerMsg)>(&self, mut f: F) {
        let _tick = self.tick.get();
        let mut q = self.queue.borrow_mut();
        while let Some(msg) = q.pop_front() {
            f(msg);
        }
    }
}

impl Default for Incoming {
    fn default() -> Self {
        Self::new()
    }
}
// SECTION: Session bundle

/// One open chess-net transport plus the two Leptos signals every page
/// needs: a queue of unprocessed `ServerMsg`s, and the coarse
/// connection state.
///
/// `incoming` is an [`Incoming`] (queue + tick signal pair) — see the
/// `Incoming` doc for why this isn't a plain Leptos signal.
///
/// Returned by every transport factory (`ws::connect`, future
/// `webrtc::connect`). Pages destructure / clone its fields freely:
/// `handle` is `Rc<dyn Transport>` so cloning is cheap; `incoming`
/// is `Clone` (cheap — `Rc` + signal copies); `state` is `ReadSignal`
/// which is `Copy`.
#[derive(Clone)]
pub struct Session {
    pub handle: Rc<dyn Transport>,
    pub incoming: Incoming,
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

pub mod webrtc;
pub mod ws;
