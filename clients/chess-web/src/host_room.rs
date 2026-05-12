//! Host-side room authority for LAN play.
//!
//! Phase 4 of `backlog/webrtc-lan-pairing.md`. Wraps the
//! `chess_net::room::Room` state machine with a multi-peer routing
//! table:
//!
//!   - Host runs the `Room` in-process (no chess-net server involved).
//!   - Host is itself one of the seated players (`PeerSink::Local`):
//!     its `ClientMsg`s flow through `Room::apply` exactly the same
//!     way a remote peer's would, and it receives projected
//!     `ServerMsg`s via a Leptos signal that the play page reads.
//!   - Remote peers (joiner + spectators) connect via WebRTC. Each
//!     `RtcDataChannel` is a `PeerSink::Remote`; incoming JSON-decoded
//!     `ClientMsg`s route through the same `Room::apply` path.
//!
//! The host page consumes the same `Session { handle, incoming, state }`
//! shape that `transport::ws::connect` returns, so the existing
//! `pages/play.rs` works unchanged once Phase 5 wires up the
//! `/lan/host` route to provide the host's `Session` via Leptos
//! context.
//!
//! ## Lifetime + reference-cycle notes
//!
//! Closures wired to `RtcDataChannel.onmessage` capture `Weak<HostRoom>`,
//! not `Rc<HostRoom>`, to avoid a reference cycle between the room
//! and the per-peer keepalive vector. When the page drops its
//! `Rc<HostRoom>`, all `Weak::upgrade` calls inside the closures
//! return `None` and become silent no-ops — the next time the
//! browser GCs the channel, the closure goes too.
//!
//! ## Dead-code allowance
//!
//! Phase 4 ships the API. Phase 5 (`/lan/host` and `/lan/join` pages)
//! is the first consumer; until then every public item triggers a
//! "never used" warning. Suppressed module-wide; the suppression
//! comes off in Phase 5 when the page code starts importing this
//! module.
#![allow(dead_code)]

// === module skeleton — section bodies filled in via edits below ===

// SECTION: imports
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use chess_core::rules::RuleSet;
use chess_net::protocol::{ClientMsg, ServerMsg};
use chess_net::room::{JoinError, Outbound, PeerId, Room};
use leptos::*;

use crate::transport::{ConnState, Session, Transport};

#[cfg(target_arch = "wasm32")]
#[allow(unused_imports)]
use {
    std::rc::Weak,
    wasm_bindgen::closure::Closure,
    wasm_bindgen::{JsCast, JsValue},
    web_sys::{MessageEvent, RtcDataChannel},
};
// SECTION: PeerSink

/// Where to deliver `ServerMsg`s for a given peer.
///
/// `Local` is the host's own play page — `ServerMsg`s become
/// values in a Leptos `WriteSignal<Option<ServerMsg>>` that the
/// page reads via the `Session.incoming` `ReadSignal`.
///
/// `Remote` is a peer connected over WebRTC — `ServerMsg`s are
/// JSON-serialised and written into a `RtcDataChannel`. The
/// non-wasm32 build (workspace `cargo check`) substitutes a
/// `RemoteMock` variant for unit tests since `web_sys` types
/// aren't reachable on native targets.
pub enum PeerSink {
    Local(WriteSignal<Option<ServerMsg>>),
    #[cfg(target_arch = "wasm32")]
    Remote(RtcDataChannel),
    /// Test-only sink that records every delivered message into a
    /// shared `Vec`. Native + wasm32 both compile this so unit
    /// tests can run on either target.
    Mock(Rc<RefCell<Vec<ServerMsg>>>),
}

impl PeerSink {
    fn deliver(&self, msg: ServerMsg) {
        match self {
            PeerSink::Local(set) => set.set(Some(msg)),
            #[cfg(target_arch = "wasm32")]
            PeerSink::Remote(dc) => {
                if let Ok(text) = serde_json::to_string(&msg) {
                    let _ = dc.send_with_str(&text);
                }
            }
            PeerSink::Mock(buf) => buf.borrow_mut().push(msg),
        }
    }
}
// SECTION: HostRoom struct

/// Host-side authoritative room. Owns a `chess_net::room::Room`
/// (the same state machine the chess-net server runs) plus a routing
/// table mapping each peer's [`PeerId`] to a [`PeerSink`].
///
/// All mutable state is in `RefCell`s (single-threaded WASM context).
/// The struct is consumed via `Rc<HostRoom>` because:
///   1. The host's `Transport` impl needs to hold a strong ref so
///      `Transport::send` can route the host's own `ClientMsg`s
///      through `Room::apply`.
///   2. Per-peer DataChannel `onmessage` closures need a `Weak`
///      ref (NOT strong — see the cycle note in the module doc).
pub struct HostRoom {
    room: RefCell<Room>,
    sinks: RefCell<HashMap<PeerId, PeerSink>>,
    /// Monotonic peer-id allocator. The host gets `PeerId(1)`; the
    /// first remote peer is `PeerId(2)`, etc.
    next_peer: Cell<u64>,
    /// `PeerId` for the host's own play UI. Always `PeerId(1)` from
    /// `new()`; surfaced as a getter for clarity at call sites.
    self_peer: PeerId,
    /// Stable label used in `RoomSummary` (the host page shows it
    /// in the lobby-style indicator). Defaults to "lan".
    room_id: String,
}
// SECTION: HostRoom::new + Session bridge

impl HostRoom {
    /// Construct a fresh host room and seat the host as the first
    /// player. Returns:
    ///
    ///   * `Rc<HostRoom>` — handle the host page holds for adding
    ///     remote peers as they pair (`add_remote_player`,
    ///     `add_remote_spectator`).
    ///   * `Session` — drop into the play page exactly where
    ///     `transport::ws::connect`'s return value would go. The
    ///     session's `incoming` signal already has the initial
    ///     `Hello` + `ChatHistory` queued (the host always seats as
    ///     RED, so the page sees `observer: Side::RED` immediately).
    ///
    /// Panics only on the impossible case where `Room::join_player`
    /// fails for the very first joiner (the room is fresh — both
    /// seats are open, no password mismatch is possible because the
    /// password was set on the same line).
    pub fn new(
        rules: RuleSet,
        password: Option<String>,
        hints_allowed: bool,
    ) -> (Rc<Self>, Session) {
        let mut room = Room::new(rules, password, hints_allowed);
        let self_peer = PeerId(1);
        let (incoming, set_incoming) = create_signal::<Option<ServerMsg>>(None);
        let (state, _set_state) = create_signal(ConnState::Open);

        // Insert host's sink BEFORE join_player so the Hello +
        // ChatHistory the room emits land in the host's own signal.
        let mut sinks: HashMap<PeerId, PeerSink> = HashMap::new();
        sinks.insert(self_peer, PeerSink::Local(set_incoming));

        let (_side, outbound) =
            room.join_player(self_peer).expect("fresh room always seats the first joiner");
        for ob in outbound {
            if let Some(sink) = sinks.get(&ob.peer) {
                sink.deliver(ob.msg);
            }
        }

        let host = Rc::new(Self {
            room: RefCell::new(room),
            sinks: RefCell::new(sinks),
            next_peer: Cell::new(2),
            self_peer,
            room_id: "lan".into(),
        });

        let handle: Rc<dyn Transport> =
            Rc::new(HostSelfTransport { host: host.clone(), self_peer });
        let session = Session { handle, incoming, state };
        (host, session)
    }

    /// `PeerId` assigned to the host's own play UI. Stable across
    /// the room's lifetime.
    pub fn self_peer(&self) -> PeerId {
        self.self_peer
    }

    /// Allocate the next `PeerId` for a newly-arriving remote peer.
    fn alloc_peer(&self) -> PeerId {
        let id = self.next_peer.get();
        self.next_peer.set(id + 1);
        PeerId(id)
    }
}
// SECTION: HostSelfTransport (impl Transport for the host's own page)

/// `Transport` impl that routes the host's own `ClientMsg`s through
/// the in-process `Room::apply` instead of a network socket.
struct HostSelfTransport {
    host: Rc<HostRoom>,
    self_peer: PeerId,
}

impl Transport for HostSelfTransport {
    fn send(&self, msg: ClientMsg) -> bool {
        self.host.handle_self_send(self.self_peer, msg);
        true
    }
}
// SECTION: add_remote_player / add_remote_spectator

impl HostRoom {
    /// Insert a remote peer with a player-side seat.
    ///
    /// `sink` should normally be `PeerSink::Remote(dc)` for production
    /// (the page just got a `RtcDataChannel` from
    /// `HostHandshake::accept_answer` succeeding); tests pass
    /// `PeerSink::Mock(...)` instead.
    ///
    /// Wires the DataChannel's `onmessage` to route incoming
    /// `ClientMsg`s through `Room::apply` (production only — the
    /// `dc_for_handlers` arg passes the channel through to the
    /// handler installer; tests skip it). On error (room full),
    /// the sink is removed and the error returned.
    pub fn add_remote_player(self: &Rc<Self>, sink: PeerSink) -> Result<PeerId, JoinError> {
        let peer = self.alloc_peer();
        self.sinks.borrow_mut().insert(peer, sink);
        let outbound = match self.room.borrow_mut().join_player(peer) {
            Ok((_side, ob)) => ob,
            Err(e) => {
                self.sinks.borrow_mut().remove(&peer);
                return Err(e);
            }
        };
        self.fanout(outbound);
        Ok(peer)
    }

    /// Insert a remote peer as a read-only spectator. `max` is the
    /// per-room spectator cap (the v1 plan says 4).
    pub fn add_remote_spectator(
        self: &Rc<Self>,
        sink: PeerSink,
        max: usize,
    ) -> Result<PeerId, JoinError> {
        let peer = self.alloc_peer();
        self.sinks.borrow_mut().insert(peer, sink);
        let outbound = match self.room.borrow_mut().join_spectator(peer, max) {
            Ok(ob) => ob,
            Err(e) => {
                self.sinks.borrow_mut().remove(&peer);
                return Err(e);
            }
        };
        self.fanout(outbound);
        Ok(peer)
    }

    /// Wasm-only convenience: take a freshly-opened `RtcDataChannel`
    /// from a completed `HostHandshake`, install onmessage routing
    /// to `Room::apply`, and seat the peer as a player.
    #[cfg(target_arch = "wasm32")]
    pub fn attach_remote_player_dc(
        self: &Rc<Self>,
        dc: RtcDataChannel,
    ) -> Result<PeerId, JoinError> {
        let peer = self.add_remote_player(PeerSink::Remote(dc.clone()))?;
        install_remote_dc_handlers(self, peer, &dc);
        Ok(peer)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn attach_remote_spectator_dc(
        self: &Rc<Self>,
        dc: RtcDataChannel,
        max: usize,
    ) -> Result<PeerId, JoinError> {
        let peer = self.add_remote_spectator(PeerSink::Remote(dc.clone()), max)?;
        install_remote_dc_handlers(self, peer, &dc);
        Ok(peer)
    }
}
// SECTION: handle_self_send / handle_remote_msg / fanout

impl HostRoom {
    /// Route the host's own `ClientMsg` (from the play page's
    /// `Transport::send`) through the room.
    fn handle_self_send(&self, peer: PeerId, msg: ClientMsg) {
        let outbound = self.room.borrow_mut().apply(peer, msg);
        self.fanout(outbound);
    }

    /// Route a remote peer's incoming `ClientMsg` through the room.
    /// Called from the per-DataChannel `onmessage` closure.
    fn handle_remote_msg(&self, peer: PeerId, msg: ClientMsg) {
        let outbound = self.room.borrow_mut().apply(peer, msg);
        self.fanout(outbound);
    }

    /// Deliver each `Outbound` to its target peer's sink. Outbounds
    /// for unknown peers (e.g. peer disconnected mid-broadcast) are
    /// silently dropped.
    fn fanout(&self, outbound: Vec<Outbound>) {
        let sinks = self.sinks.borrow();
        for ob in outbound {
            if let Some(sink) = sinks.get(&ob.peer) {
                sink.deliver(ob.msg);
            }
        }
    }

    /// Stable label used in `RoomSummary` (the host page may show
    /// the room state in a sidebar widget).
    pub fn room_id(&self) -> &str {
        &self.room_id
    }

    /// Snapshot of the underlying room's [`chess_net::RoomSummary`].
    /// Useful for the host page's sidebar / future lobby UI.
    pub fn summary(&self) -> chess_net::RoomSummary {
        self.room.borrow().summary(&self.room_id)
    }
}
// SECTION: peer disconnect handling

impl HostRoom {
    /// Remove a peer from the routing table + room. Returns the
    /// outbound list `Room::leave` produces (e.g. mid-game
    /// "opponent disconnected" notice to the surviving seat). The
    /// caller is responsible for invoking this from a DataChannel
    /// `onclose` event — production wires it via
    /// `install_remote_dc_handlers`.
    pub fn drop_peer(&self, peer: PeerId) {
        let outbound = self.room.borrow_mut().leave(peer);
        self.fanout(outbound);
        self.sinks.borrow_mut().remove(&peer);
    }
}
// SECTION: install_remote_dc_handlers (Weak<HostRoom> closures)

/// Wire a remote peer's `RtcDataChannel.onmessage` to route incoming
/// `ClientMsg` JSON through `Room::apply`, and `onclose` to drop the
/// peer.
///
/// Closures capture `Weak<HostRoom>` (NOT `Rc`) so dropping the
/// page's strong references actually frees the room — without
/// `Weak`, the closures-in-keepalive-vec → `Rc<HostRoom>` cycle
/// would leak forever. Once the room drops, `Weak::upgrade` returns
/// `None` and any in-flight callback is a silent no-op.
///
/// Closures are `cb.forget()`-leaked: the only way to cancel them
/// would be `dc.set_onmessage(None)`, but the room dropping already
/// makes them harmless. For Phase 4 this is acceptable; Phase 5+
/// could add explicit teardown if memory pressure ever shows up.
#[cfg(target_arch = "wasm32")]
fn install_remote_dc_handlers(host: &Rc<HostRoom>, peer: PeerId, dc: &RtcDataChannel) {
    {
        let weak = Rc::downgrade(host);
        let cb = Closure::wrap(Box::new(move |ev: JsValue| {
            let ev: MessageEvent = ev.unchecked_into();
            let text = match ev.data().as_string() {
                Some(t) => t,
                None => return,
            };
            let msg: ClientMsg = match serde_json::from_str(&text) {
                Ok(m) => m,
                Err(_) => return,
            };
            if let Some(host) = weak.upgrade() {
                host.handle_remote_msg(peer, msg);
            }
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onmessage(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
    {
        let weak = Rc::downgrade(host);
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            if let Some(host) = weak.upgrade() {
                host.drop_peer(peer);
            }
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onclose(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
}
// SECTION: tests
//
// HostRoom is wasm32-only because of the Leptos `WriteSignal` /
// `ReadSignal` dependency on the host's `Session` bridge. Native
// `cargo test --workspace` therefore cannot exercise this module
// directly; the meaningful test coverage lives elsewhere:
//
// * `chess-net::room::tests` (in the workspace) has 17 unit tests
//   covering every `Room::apply` branch (move / resign / chat /
//   rematch / leave / spectator gates / ...). HostRoom is a thin
//   routing wrapper on top of that; the moment a `ClientMsg`
//   reaches `Room::apply`, behaviour is identical to the chess-net
//   server. So the room-side correctness is well-covered.
//
// * The Phase 5 `/lan/host` and `/lan/join` pages exercise the
//   wasm-specific glue (DataChannel routing, Local-sink Hello
//   delivery, peer disconnect notifications) end-to-end on real
//   devices.
//
// If we ever need native unit coverage of HostRoom itself, the
// cleanest split is to move the pure-routing `handle_remote_msg` /
// `fanout` / `drop_peer` methods + `PeerSink::Mock` into a
// `host_room/router.rs` submodule that has no Leptos dependency,
// leaving the `Session` bridge in a wasm32-only `host_room/
// session.rs`. Deferred until a routing-bug-without-an-obvious-fix
// shows up.
