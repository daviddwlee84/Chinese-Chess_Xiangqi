//! WebRTC transport for chess-net protocol.
//!
//! Production successor to the Phase 0 spike in
//! `clients/chess-web/src/spike/rtc.rs`. Same underlying plumbing
//! (`web_sys::RtcPeerConnection` + reliable+ordered DataChannel +
//! mDNS-only ICE), but exposes a clean API matching `transport::ws`:
//!
//! * `connect_as_joiner(cfg, offer) -> JoinerHandshake` — the
//!   joiner consumes the host's offer SDP and produces an answer SDP.
//!   Returns the same [`Session`] shape as `transport::ws::connect`,
//!   so `pages/play.rs` and `pages/lobby.rs` work unchanged.
//! * `connect_as_host(cfg) -> HostHandshake` — the host generates an
//!   offer SDP and waits for the joiner's answer. Exposes the raw
//!   PeerConnection + DataChannel handles so Phase 4's
//!   `host_room.rs` can wrap them with multi-peer `Room` routing.
//!
//! Out of scope for Phase 3:
//! * QR generation / camera scanning (Phase 5).
//! * Multi-peer host (`host_room.rs` — Phase 4).
//! * Auto-reconnect / signalling fallback (Approach C — P3 follow-up).
//!
//! Backlog: `backlog/webrtc-lan-pairing.md`. Pitfalls:
//! `pitfalls/webrtc-mdns-lan-ap-isolation.md`.
//!
//! ## Dead-code allowance
//!
//! Phase 4 (`host_room.rs`) and Phase 5 (`pages/lan.rs`) consume
//! the public surface of this module — `connect_as_*`, `OfferBlob`,
//! `AnswerBlob`, `WebRtcConfig`, `IceMode`. Some helpers (the spike's
//! `Mock` sink path, future spectator attach, etc.) remain unused;
//! kept for completeness pending Phase 5 polish.
#![allow(dead_code)]

// === module skeleton — section bodies filled in via edits below ===

// SECTION: imports
use std::cell::RefCell;
use std::rc::Rc;

use chess_net::protocol::{ClientMsg, ServerMsg};
use js_sys::{Array, Reflect};
use leptos::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit,
    RtcDataChannelState, RtcIceConnectionState, RtcIceGatheringState, RtcPeerConnection,
    RtcPeerConnectionState, RtcSdpType, RtcSessionDescriptionInit, RtcSignalingState,
};

use super::{ConnState, Incoming, Session, Transport};
// SECTION: public types (IceMode, WebRtcConfig, OfferBlob, AnswerBlob)

/// `RtcPeerConnection` ICE configuration.
///
/// `LanOnly` is the production default — `iceServers: []`, pure mDNS
/// `<uuid>.local` host candidates only. No external dependencies.
///
/// `WithStun` adds a multi-server STUN list for the `srflx` candidates.
/// Useful as a fallback when LAN P2P fails (e.g. router blocks WebRTC's
/// `<uuid>.local` resolution; see
/// `pitfalls/webrtc-mdns-lan-ap-isolation.md`). Server list ordered for
/// CN reachability — `stun.miwifi.com` and `stun.qq.com` first, Google
/// last (Google STUN is blocked by the GFW from mainland China).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IceMode {
    LanOnly,
    WithStun,
}

/// Knobs for opening a WebRTC session. Defaults to `LanOnly` — the
/// "no external dependency" production case.
#[derive(Copy, Clone, Debug)]
pub struct WebRtcConfig {
    pub ice_mode: IceMode,
}

impl Default for WebRtcConfig {
    fn default() -> Self {
        Self { ice_mode: IceMode::LanOnly }
    }
}

/// Maximum time we wait for `iceGatheringState == "complete"` before
/// giving up and producing SDP with whatever candidates have arrived.
///
/// Caps the failure mode where one of the configured STUN servers is
/// unreachable (e.g. `stun.l.google.com` from mainland China — blocked
/// by the GFW), which otherwise hangs the whole `connect_as_*` flow
/// indefinitely.
///
/// 5s is generous: pure-LAN gathering completes in 100-200 ms, healthy
/// STUN gathering in <1 s. If we hit the timeout we still produce a
/// valid SDP — there will just be fewer candidates than "ideal".
const ICE_GATHER_TIMEOUT_MS: i32 = 5000;

/// Typed wrapper around the host's offer SDP blob (JSON envelope of
/// `{type:"offer", sdp:"..."}`). Prevents accidentally passing an
/// answer where an offer is expected.
#[derive(Clone, Debug)]
pub struct OfferBlob(pub String);

/// Typed wrapper around the joiner's answer SDP blob.
#[derive(Clone, Debug)]
pub struct AnswerBlob(pub String);

/// Live diagnostic snapshot of the underlying RTCPeerConnection. Exposed
/// to the LAN host/join pages so they can show users WHY pairing is
/// stuck (e.g. "ICE: failed, candidates: 1") instead of just timing out
/// with no insight. Updated by listeners installed in
/// `install_ice_diag_handlers` + the gather-state listener inside
/// `wait_for_ice_complete`.
///
/// The cross-device LAN failure documented in
/// `pitfalls/webrtc-mdns-lan-ap-isolation.md` shows up here as
/// `ice = Checking` flipping to `Disconnected`/`Failed` while the page
/// is staring at the 10 s DC-open wait.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct IceDiag {
    pub ice: RtcIceConnectionState,
    pub conn: RtcPeerConnectionState,
    pub gather: RtcIceGatheringState,
    /// Count of non-null `icecandidate` events fired since handler
    /// installation. A pure-`.local`-only offer typically lands at 1;
    /// healthy STUN adds 1–3 srflx candidates on top.
    pub candidates: u32,
}

impl IceDiag {
    pub fn initial(pc: &RtcPeerConnection) -> Self {
        Self {
            ice: pc.ice_connection_state(),
            conn: pc.connection_state(),
            gather: pc.ice_gathering_state(),
            candidates: 0,
        }
    }
}
// SECTION: WebRtcTransport (impl Transport)

/// `Transport` impl backed by a single `RtcDataChannel`. Shared by both
/// joiner and host sides — the same DataChannel just carries different
/// payloads in each direction (joiner sends `ClientMsg`, receives
/// `ServerMsg`; host the inverse, but Phase 4 wraps it differently
/// rather than reusing this trait directly for the host send path).
pub struct WebRtcTransport {
    /// `Rc<RefCell<...>>` so the DataChannel reference can be late-bound
    /// (joiner side discovers it via `ondatachannel` when the host's
    /// channel arrives, after `connect_as_joiner` returns).
    dc: Rc<RefCell<Option<RtcDataChannel>>>,
}

impl Transport for WebRtcTransport {
    fn send(&self, msg: ClientMsg) -> bool {
        let dc = self.dc.borrow();
        let dc = match dc.as_ref() {
            Some(dc) => dc,
            None => return false,
        };
        if dc.ready_state() != RtcDataChannelState::Open {
            return false;
        }
        let body = match serde_json::to_string(&msg) {
            Ok(s) => s,
            Err(_) => return false,
        };
        dc.send_with_str(&body).is_ok()
    }
}
// SECTION: HostHandshake

/// What `connect_as_host` returns.
///
/// Phase 4 (`host_room.rs`) consumes this: it owns the `Room` state
/// machine, holds the `pc` + `dc` references, and routes incoming
/// `ClientMsg`s through `Room::apply` to produce per-peer `ServerMsg`
/// fan-outs. Phase 3 just exposes the raw handles + the
/// offer/accept-answer dance.
///
/// Note: there's no `Session` here. The host doesn't consume the
/// `Transport` trait directly — it operates one level lower because
/// it needs to tag incoming bytes by `PeerId` and demultiplex
/// outgoing bytes back to specific peers' DataChannels (Phase 4
/// adds spectator channels).
pub struct HostHandshake {
    pub pc: RtcPeerConnection,
    /// Slot for the host-side DataChannel. Populated synchronously
    /// during `connect_as_host` (the host creates the channel before
    /// generating the offer); consumers may `clone()` to drive it.
    pub dc: Rc<RefCell<Option<RtcDataChannel>>>,
    pub offer: OfferBlob,
    /// Reactive view of the joiner's connection state. `Open` once
    /// the DataChannel handshake completes; `Closed`/`Error` on
    /// failure. Phase 4's `host_room.rs` watches this to know when
    /// to emit `Hello` / detect peer disconnect.
    pub state: ReadSignal<ConnState>,
    /// Live ICE / connection / gathering snapshot. The LAN host page
    /// shows this as a status badge and reads it when the DC-open
    /// timeout fires so the error message can name the actual stuck
    /// state instead of a generic "network blocked?".
    pub ice_diag: ReadSignal<IceDiag>,
    /// Wasm-bindgen closures kept alive for the connection's
    /// lifetime. Drop the `HostHandshake` and they all get freed.
    _keepalive: Vec<Closure<dyn FnMut(JsValue)>>,
}

impl HostHandshake {
    /// Apply the joiner's answer SDP to complete the handshake.
    ///
    /// Pre-flights `signalingState` so we surface "the joiner's
    /// answer is stale" / "you accepted twice" before throwing the
    /// browser's cryptic `Called in wrong state` error. See
    /// `pitfalls/...` (the iOS-backgrounding pitfall, when written).
    pub async fn accept_answer(&self, answer: AnswerBlob) -> Result<(), JsValue> {
        let signaling = self.pc.signaling_state();
        if signaling != RtcSignalingState::HaveLocalOffer {
            return Err(JsValue::from_str(&format!(
                "PeerConnection signaling state is `{signaling:?}` (expected `HaveLocalOffer`). \
                 The host's offer may have been replaced (multiple Start hosting taps), \
                 the page was paused (iOS Safari backgrounding tears down RTC), or this \
                 answer was already accepted."
            )));
        }
        let answer_sdp = decode_answer(&answer)?;
        let answer_desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        answer_desc.set_sdp(&answer_sdp);
        JsFuture::from(self.pc.set_remote_description(&answer_desc)).await?;
        Ok(())
    }

    /// Re-encode `pc.local_description()` as an offer blob right now.
    ///
    /// `connect_as_host` returns once `wait_for_ice_complete` resolves,
    /// but the browser may continue adding ICE candidates in the
    /// background (especially trickle-srflx if STUN is enabled and the
    /// initial 5s gather timeout fired). The page's "Copy offer" button
    /// calls this so the textarea / QR reflect those late arrivals
    /// instead of the half-gathered snapshot frozen at handshake time.
    pub fn current_offer(&self) -> OfferBlob {
        let sdp = self.pc.local_description().map(|d| d.sdp()).unwrap_or_default();
        OfferBlob(encode_sdp("offer", &sdp))
    }
}
// SECTION: JoinerHandshake (return value of connect_as_joiner)

/// What `connect_as_joiner` returns.
///
/// `session` is a normal [`Session`] — drop it into `pages/play.rs`
/// or `pages/lobby.rs` exactly where the WebSocket `Session` would
/// have gone.
///
/// `answer` is the SDP blob to deliver back to the host out-of-band
/// (paste into a textarea / scan via QR / whatever the higher-level
/// flow uses).
pub struct JoinerHandshake {
    pub session: Session,
    pub answer: AnswerBlob,
    /// `RtcPeerConnection` reference — kept alive in the page so the
    /// connection survives. Wraps in `Rc<RefCell<...>>` to match the
    /// host side (if a future caller wants to e.g. close it
    /// proactively).
    pub pc: Rc<RtcPeerConnection>,
    /// Same role as [`HostHandshake::ice_diag`] — exposed to the LAN
    /// join page for diagnostic UI / late-candidate-aware Copy.
    pub ice_diag: ReadSignal<IceDiag>,
    _keepalive: Vec<Closure<dyn FnMut(JsValue)>>,
}

impl JoinerHandshake {
    /// Re-encode `pc.local_description()` as an answer blob right now.
    /// See [`HostHandshake::current_offer`] for the rationale.
    pub fn current_answer(&self) -> AnswerBlob {
        let sdp = self.pc.local_description().map(|d| d.sdp()).unwrap_or_default();
        AnswerBlob(encode_sdp("answer", &sdp))
    }
}
// SECTION: connect_as_joiner factory

/// Open a WebRTC session as the joiner.
///
/// Steps:
/// 1. Create `RtcPeerConnection` with `cfg.ice_mode`.
/// 2. Wire `ondatachannel` so we receive the host's DataChannel.
/// 3. Apply the host's offer SDP via `setRemoteDescription`.
/// 4. Generate an answer SDP via `createAnswer` + `setLocalDescription`.
/// 5. Wait up to [`ICE_GATHER_TIMEOUT_MS`] for ICE gathering.
/// 6. Return the answer SDP + a [`Session`] backed by the DataChannel.
pub async fn connect_as_joiner(
    cfg: WebRtcConfig,
    offer: OfferBlob,
) -> Result<JoinerHandshake, JsValue> {
    let pc = new_peer_connection(cfg.ice_mode)?;
    let pc_rc = Rc::new(pc);
    let dc_holder: Rc<RefCell<Option<RtcDataChannel>>> = Rc::new(RefCell::new(None));
    let mut keepalive: Vec<Closure<dyn FnMut(JsValue)>> = Vec::new();

    // `incoming` is a queue (push from DC onmessage, page drains via
    // tick signal). See `transport::Incoming` doc for the rationale.
    let incoming = Incoming::new();
    let (state, set_state) = create_signal(ConnState::Connecting);
    let (ice_diag, set_ice_diag) = create_signal(IceDiag::initial(&pc_rc));
    install_ice_diag_handlers(&pc_rc, set_ice_diag, &mut keepalive);

    // The DataChannel arrives on `ondatachannel` after the host's
    // SDP is applied. Wire it up before we set the remote description
    // so we don't miss the event.
    {
        let dc_holder = dc_holder.clone();
        let incoming_for_dc = incoming.clone();
        let cb = Closure::wrap(Box::new(move |ev: JsValue| {
            let ev: RtcDataChannelEvent = ev.unchecked_into();
            let dc = ev.channel();
            install_dc_handlers_for_joiner(&dc, incoming_for_dc.clone(), set_state);
            *dc_holder.borrow_mut() = Some(dc);
        }) as Box<dyn FnMut(JsValue)>);
        pc_rc.set_ondatachannel(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);
    }

    let offer_sdp = decode_offer(&offer)?;
    let offer_desc = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
    offer_desc.set_sdp(&offer_sdp);
    JsFuture::from(pc_rc.set_remote_description(&offer_desc)).await?;

    let answer = JsFuture::from(pc_rc.create_answer()).await?;
    let answer_desc: RtcSessionDescriptionInit = answer.unchecked_into();
    JsFuture::from(pc_rc.set_local_description(&answer_desc)).await?;
    wait_for_ice_complete(&pc_rc, set_ice_diag, &mut keepalive).await;

    let sdp = pc_rc.local_description().map(|d| d.sdp()).unwrap_or_default();
    let answer_blob = AnswerBlob(encode_sdp("answer", &sdp));

    let handle: Rc<dyn Transport> = Rc::new(WebRtcTransport { dc: dc_holder });
    let session = Session { handle, incoming, state };
    Ok(JoinerHandshake { session, answer: answer_blob, pc: pc_rc, ice_diag, _keepalive: keepalive })
}
// SECTION: connect_as_host factory

/// Open a WebRTC session as the host.
///
/// Steps:
/// 1. Create `RtcPeerConnection` with `cfg.ice_mode`.
/// 2. Create the DataChannel (host always initiates; joiner discovers
///    via `ondatachannel`).
/// 3. Generate an offer SDP via `createOffer` + `setLocalDescription`.
/// 4. Wait up to [`ICE_GATHER_TIMEOUT_MS`] for ICE gathering.
/// 5. Return the offer SDP. Caller delivers it out-of-band, then calls
///    `HostHandshake::accept_answer` with the joiner's response.
///
/// State signal flips to `Open` when the DataChannel handshake
/// completes (on top of `accept_answer` succeeding).
pub async fn connect_as_host(cfg: WebRtcConfig) -> Result<HostHandshake, JsValue> {
    let pc = new_peer_connection(cfg.ice_mode)?;
    let dc_init = {
        let init = RtcDataChannelInit::new();
        // reliable + ordered — the chess-net wire shape assumes
        // in-order delivery.
        init.set_ordered(true);
        init
    };
    let dc = pc.create_data_channel_with_data_channel_dict("chess", &dc_init);
    let dc_holder: Rc<RefCell<Option<RtcDataChannel>>> = Rc::new(RefCell::new(Some(dc.clone())));
    let mut keepalive: Vec<Closure<dyn FnMut(JsValue)>> = Vec::new();

    let (state, set_state) = create_signal(ConnState::Connecting);
    install_dc_state_handlers(&dc, set_state);
    let (ice_diag, set_ice_diag) = create_signal(IceDiag::initial(&pc));
    install_ice_diag_handlers(&pc, set_ice_diag, &mut keepalive);

    let offer = JsFuture::from(pc.create_offer()).await?;
    let offer_desc: RtcSessionDescriptionInit = offer.unchecked_into();
    JsFuture::from(pc.set_local_description(&offer_desc)).await?;
    wait_for_ice_complete(&pc, set_ice_diag, &mut keepalive).await;

    let sdp = pc.local_description().map(|d| d.sdp()).unwrap_or_default();
    let offer_blob = OfferBlob(encode_sdp("offer", &sdp));
    Ok(HostHandshake {
        pc,
        dc: dc_holder,
        offer: offer_blob,
        state,
        ice_diag,
        _keepalive: keepalive,
    })
}
/// Block until `dc.ready_state() == "open"` OR `timeout_ms` elapses.
/// Returns `true` if the channel opened, `false` on timeout.
///
/// Used by the host page after `accept_answer` resolves: that
/// `await` completes as soon as `setRemoteDescription` returns,
/// which is BEFORE the SCTP handshake finishes. Calling
/// `dc.send_with_str(...)` on a still-`Connecting` DataChannel
/// silently fails (browsers return Err but the chess `PeerSink::Remote`
/// path discards the result), so the host's `Hello` + `ChatHistory`
/// fanout to the joiner would be lost. Awaiting this helper before
/// `HostRoom::attach_remote_player_dc` ensures the fanout reaches
/// the joiner.
///
/// Polls every 50 ms via `setTimeout`. Lower-level alternative would
/// be a Promise wrapping `dc.set_onopen`, but we already need the
/// timeout path for hostile-network failure cases, so polling is
/// simpler than two-channel select.
pub async fn wait_for_dc_open(dc: &RtcDataChannel, timeout_ms: u32) -> bool {
    let start = js_sys::Date::now();
    let deadline = start + timeout_ms as f64;
    loop {
        if dc.ready_state() == RtcDataChannelState::Open {
            return true;
        }
        if js_sys::Date::now() > deadline {
            return false;
        }
        let promise = js_sys::Promise::new(&mut |resolve, _reject| {
            if let Some(win) = web_sys::window() {
                let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 50);
            }
        });
        let _ = JsFuture::from(promise).await;
    }
}
// SECTION: SDP envelope encode/decode

/// Wrap `(type, sdp)` in a JSON envelope so the textarea contents survive
/// copy-paste through clipboard / AirDrop / IM apps that helpfully
/// "normalise" line endings (SDP per RFC 4566 demands CRLF; many
/// transports strip the CR).
///
/// Pretty-printed with 2-space indent so the contents eyeball the same
/// as raw SDP — useful while debugging.
pub fn encode_sdp(kind: &str, sdp: &str) -> String {
    format!("{{\n  \"type\": \"{}\",\n  \"sdp\": \"{}\"\n}}", kind, escape_json_string(sdp))
}

fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn decode_sdp_envelope(blob: &str) -> Result<(String, String), JsValue> {
    let trimmed = blob.trim();
    if !trimmed.starts_with('{') {
        return Err(JsValue::from_str(
            "SDP must be the JSON envelope produced by the host page (raw-SDP is not accepted)",
        ));
    }
    let parsed = js_sys::JSON::parse(trimmed)
        .map_err(|e| JsValue::from_str(&format!("SDP envelope is not valid JSON: {e:?}")))?;
    let kind =
        Reflect::get(&parsed, &"type".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
    let sdp =
        Reflect::get(&parsed, &"sdp".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
    if kind.is_empty() || sdp.is_empty() {
        return Err(JsValue::from_str("SDP envelope JSON missing `type` or `sdp` field"));
    }
    // Browsers accept LF-only SDP in practice but normalise anyway —
    // copy-paste through AirDrop / SMS / IM may strip CR.
    let sdp = sdp.replace('\r', "").replace('\n', "\r\n");
    Ok((kind, sdp))
}

fn decode_offer(blob: &OfferBlob) -> Result<String, JsValue> {
    let (kind, sdp) = decode_sdp_envelope(&blob.0)?;
    if kind != "offer" {
        return Err(JsValue::from_str(&format!("expected offer envelope, got `{kind}`")));
    }
    Ok(sdp)
}

fn decode_answer(blob: &AnswerBlob) -> Result<String, JsValue> {
    let (kind, sdp) = decode_sdp_envelope(&blob.0)?;
    if kind != "answer" {
        return Err(JsValue::from_str(&format!("expected answer envelope, got `{kind}`")));
    }
    Ok(sdp)
}
// SECTION: ICE config + ICE-gather wait helper

fn new_peer_connection(mode: IceMode) -> Result<RtcPeerConnection, JsValue> {
    let cfg = RtcConfiguration::new();
    let servers: Array = Array::new();
    if mode == IceMode::WithStun {
        // Multi-server STUN list, ordered for CN reachability:
        //   - stun.miwifi.com — Xiaomi's own STUN; reachable from CN.
        //   - stun.qq.com     — Tencent's; reachable from CN.
        //   - stun.cloudflare.com — Cloudflare; usually reachable.
        //   - stun.l.google.com — global default; BLOCKED IN CN by GFW.
        // Browsers race them in parallel and use whichever responds
        // first; the GFW-blocked Google entry is harmless because the
        // others answer first.
        for url in [
            "stun:stun.miwifi.com:3478",
            "stun:stun.qq.com:3478",
            "stun:stun.cloudflare.com:3478",
            "stun:stun.l.google.com:19302",
        ] {
            let server = js_sys::Object::new();
            Reflect::set(&server, &"urls".into(), &url.into()).ok();
            servers.push(&server);
        }
    }
    Reflect::set(&cfg, &"iceServers".into(), &servers).ok();
    RtcPeerConnection::new_with_configuration(&cfg)
}

/// Block until `pc.iceGatheringState == "complete"`, OR
/// [`ICE_GATHER_TIMEOUT_MS`] elapses (whichever comes first).
///
/// Also threads gathering-state changes into the [`IceDiag`] signal so
/// the LAN host/join page can render "gathering: gathering → complete"
/// live. Owning this listener here (instead of inside
/// `install_ice_diag_handlers`) keeps the one-shot promise resolution
/// glued to the one place that needs it; `set_onicegatheringstatechange`
/// only takes a single callback so we can't have both jobs install
/// independent listeners.
async fn wait_for_ice_complete(
    pc: &RtcPeerConnection,
    set_diag: WriteSignal<IceDiag>,
    keepalive: &mut Vec<Closure<dyn FnMut(JsValue)>>,
) {
    if pc.ice_gathering_state() == RtcIceGatheringState::Complete {
        set_diag.update(|d| d.gather = RtcIceGatheringState::Complete);
        return;
    }
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let pc_inner = pc.clone();
        let resolve_for_state = resolve.clone();
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            let s = pc_inner.ice_gathering_state();
            web_sys::console::log_2(
                &"[webrtc] iceGatheringState =".into(),
                &format!("{s:?}").into(),
            );
            set_diag.update(|d| d.gather = s);
            if s == RtcIceGatheringState::Complete {
                let _ = resolve_for_state.call0(&JsValue::NULL);
            }
        }) as Box<dyn FnMut(JsValue)>);
        pc.set_onicegatheringstatechange(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);

        // Hard-cap the wait. Late `complete` events still fire but
        // promise resolution is idempotent.
        if let Some(win) = web_sys::window() {
            let resolve_for_timeout = resolve.clone();
            let pc_for_timeout = pc.clone();
            let timeout_cb = Closure::wrap(Box::new(move |_ev: JsValue| {
                let s = pc_for_timeout.ice_gathering_state();
                web_sys::console::warn_2(
                    &"[webrtc] ICE gather timeout — proceeding with whatever candidates we have. Final state:".into(),
                    &format!("{s:?}").into(),
                );
                set_diag.update(|d| d.gather = s);
                let _ = resolve_for_timeout.call0(&JsValue::NULL);
            }) as Box<dyn FnMut(JsValue)>);
            let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
                timeout_cb.as_ref().unchecked_ref(),
                ICE_GATHER_TIMEOUT_MS,
            );
            keepalive.push(timeout_cb);
        }
    });
    let _ = JsFuture::from(promise).await;
}

/// Install reactive bridges for `oniceconnectionstatechange`,
/// `onconnectionstatechange`, and `onicecandidate` — pushes each event
/// into the `IceDiag` write signal and `console.log`s the transition.
///
/// The `onicegatheringstatechange` listener is NOT installed here —
/// `wait_for_ice_complete` owns it (only one callback slot per event,
/// and its promise resolution + diag-update have to share that slot).
///
/// Closures are pushed into `keepalive` so they outlive the call;
/// dropping the Handshake drops them.
fn install_ice_diag_handlers(
    pc: &RtcPeerConnection,
    set_diag: WriteSignal<IceDiag>,
    keepalive: &mut Vec<Closure<dyn FnMut(JsValue)>>,
) {
    {
        let pc_inner = pc.clone();
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            let s = pc_inner.ice_connection_state();
            web_sys::console::log_2(
                &"[webrtc] iceConnectionState =".into(),
                &format!("{s:?}").into(),
            );
            set_diag.update(|d| d.ice = s);
        }) as Box<dyn FnMut(JsValue)>);
        pc.set_oniceconnectionstatechange(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);
    }
    {
        let pc_inner = pc.clone();
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            let s = pc_inner.connection_state();
            web_sys::console::log_2(&"[webrtc] connectionState =".into(), &format!("{s:?}").into());
            set_diag.update(|d| d.conn = s);
        }) as Box<dyn FnMut(JsValue)>);
        pc.set_onconnectionstatechange(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);
    }
    {
        // The event also fires once with a null candidate when
        // gathering ends — we ignore that branch and count only
        // real candidates. `RtcPeerConnectionIceEvent.candidate()`
        // returns Option<RtcIceCandidate>.
        let cb = Closure::wrap(Box::new(move |ev: JsValue| {
            let ev: web_sys::RtcPeerConnectionIceEvent = ev.unchecked_into();
            if ev.candidate().is_some() {
                set_diag.update(|d| d.candidates = d.candidates.saturating_add(1));
            }
        }) as Box<dyn FnMut(JsValue)>);
        pc.set_onicecandidate(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);
    }
}
// SECTION: DataChannel handler installation

/// Joiner-side handlers: incoming bytes deserialize as `ServerMsg`,
/// `onopen` flips the state signal to `Open`, `onclose` to `Closed`.
fn install_dc_handlers_for_joiner(
    dc: &RtcDataChannel,
    incoming: Incoming,
    state: WriteSignal<ConnState>,
) {
    {
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            state.set(ConnState::Open);
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onopen(Some(cb.as_ref().unchecked_ref()));
        // Closures here intentionally leak — they have to outlive the
        // PeerConnection. The browser only fires `onopen` once + a
        // bounded number of `onmessage`/`onclose` events; in steady
        // state of an open connection the closures are kept alive by
        // the JS side via the listener registration. The page's
        // `JoinerHandshake._keepalive` keeps the gathering closures
        // alive separately.
        cb.forget();
    }
    {
        let incoming_for_msg = incoming.clone();
        let cb = Closure::wrap(Box::new(move |ev: JsValue| {
            let ev: MessageEvent = ev.unchecked_into();
            if let Some(text) = ev.data().as_string() {
                if let Ok(msg) = serde_json::from_str::<ServerMsg>(&text) {
                    incoming_for_msg.push(msg);
                }
            }
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onmessage(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
    {
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            state.set(ConnState::Closed);
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onclose(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }

    // Defensive: if the DataChannel is ALREADY open by the time we
    // set the handlers (fast LAN path — the SCTP handshake can
    // complete in <1 ms, faster than `ondatachannel` → handler
    // installation Rust closures), the `onopen` event has already
    // fired and we'd never see it. Manually flip the state signal
    // so the page can transition. Browsers don't re-fire `onopen`.
    if dc.ready_state() == RtcDataChannelState::Open {
        state.set(ConnState::Open);
    }
}

/// Host-side state handlers — flip `state` to `Open`/`Closed` based on
/// DataChannel events. `onmessage` is wired separately by Phase 4's
/// `host_room.rs` because incoming bytes for the host are
/// `ClientMsg`, not `ServerMsg`, and they need to be tagged with a
/// `PeerId` before going to `Room::apply`.
fn install_dc_state_handlers(dc: &RtcDataChannel, state: WriteSignal<ConnState>) {
    {
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            state.set(ConnState::Open);
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onopen(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
    {
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            state.set(ConnState::Closed);
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onclose(Some(cb.as_ref().unchecked_ref()));
        cb.forget();
    }
    // Defensive (see `install_dc_handlers_for_joiner` for the
    // rationale): cover the race where the DC opens before the
    // handler is installed.
    if dc.ready_state() == RtcDataChannelState::Open {
        state.set(ConnState::Open);
    }
}

// SECTION: tests
//
// The async factories — `connect_as_joiner` / `connect_as_host` —
// require a live `RtcPeerConnection` and are exercised end-to-end via
// the spike pages on real devices. Mocking `RtcPeerConnection` for
// unit tests has historically been more pain than insight.
//
// The pure-string SDP envelope codec is small enough that
// `pages/play.rs` exercising it through `connect_as_joiner` (which it
// will once Phase 4 wires this up) is adequate coverage. Phase 5 may
// add a `wasm_bindgen_test` harness once the page-level QR code
// paths land.
