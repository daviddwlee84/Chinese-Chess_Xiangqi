//! Minimal browser-WebRTC plumbing for the Phase 0 spike. Pure
//! callbacks-and-promises wrappers around `web_sys` — no `Future`-based
//! API, no signal integration; that lives in `lan_echo.rs`.
//!
//! Production code in Phase 3 will live in `clients/chess-web/src/transport/webrtc.rs`
//! with proper `Transport` impl, lifecycle management, error types, etc.
//! This file is deliberately bare-bones so we can validate the underlying
//! browser API does what we think it does on real iOS / Safari before
//! committing to a polished design.

// === module skeleton — section bodies filled in via edits below ===

// SECTION: imports
use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Array, Reflect};
use leptos::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit,
    RtcDataChannelState, RtcIceGatheringState, RtcPeerConnection, RtcPeerConnectionIceEvent,
    RtcSdpType, RtcSessionDescriptionInit, RtcSignalingState,
};
// SECTION: PeerSession struct

/// Live RTC state owned by the page. The page reads `state` / `messages`
/// reactively and mutates `pc` / `dc` as the handshake progresses.
///
/// Closures (`_keepalive`) are stored here so they don't get GC'd by
/// wasm-bindgen — RTC events are async and the browser will call them
/// long after the calling function returns.
///
/// The signal fields are owned by the page; `PeerSession` only re-stores
/// them so future debug helpers can `session.state.set(...)`. Live
/// readers go through the reactive signals the page already holds.
#[allow(dead_code)]
pub struct PeerSession {
    pub pc: RtcPeerConnection,
    pub dc: Rc<RefCell<Option<RtcDataChannel>>>,
    pub state: WriteSignal<RtcConn>,
    pub messages: WriteSignal<Vec<String>>,
    /// Diagnostic counters: opened-at, ICE-completed-at (perf.now() ms).
    pub gather_started_ms: f64,
    pub gather_done_ms: WriteSignal<Option<f64>>,
    pub channel_opened_ms: WriteSignal<Option<f64>>,
    pub ice_state: WriteSignal<String>,
    pub signaling_state: WriteSignal<String>,
    /// Wasm-bindgen closures we need to keep alive for the connection's
    /// lifetime. Drop the `PeerSession` and they all get freed.
    _keepalive: Vec<Closure<dyn FnMut(JsValue)>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RtcConn {
    /// Brand new — no offer / answer yet.
    Idle,
    /// Local description set, gathering ICE.
    Gathering,
    /// ICE done; SDP ready to be exchanged.
    Ready,
    /// DataChannel open; messages flowing.
    Connected,
    /// Failed at some point.
    Failed,
}
// SECTION: open_host

/// Bag of `WriteSignal`s the page hands to `open_host` / `open_joiner` so
/// the RTC plumbing can update the diag panel reactively. Bundled into a
/// struct because there are six of them — too many positional args.
#[derive(Clone, Copy)]
pub struct OpenSetters {
    pub state: WriteSignal<RtcConn>,
    pub messages: WriteSignal<Vec<String>>,
    pub gather_done_ms: WriteSignal<Option<f64>>,
    pub channel_opened_ms: WriteSignal<Option<f64>>,
    pub ice_state: WriteSignal<String>,
    pub signaling_state: WriteSignal<String>,
}

/// Host side: create the PeerConnection + DataChannel, generate the
/// offer, wait for ICE gathering to complete, return the full SDP blob.
pub async fn open_host(
    setters: OpenSetters,
    ice_mode: IceMode,
) -> Result<(PeerSession, String), JsValue> {
    let now = perf_now();
    let pc = new_peer_connection(ice_mode)?;
    let dc_init = {
        let init = RtcDataChannelInit::new();
        // reliable + ordered — the chess-net wire shape assumes
        // in-order delivery.
        init.set_ordered(true);
        init
    };
    let dc = pc.create_data_channel_with_data_channel_dict("chess-spike", &dc_init);

    let dc_holder: Rc<RefCell<Option<RtcDataChannel>>> = Rc::new(RefCell::new(Some(dc.clone())));
    let mut keepalive: Vec<Closure<dyn FnMut(JsValue)>> = Vec::new();

    install_dc_handlers(
        &dc,
        setters.messages,
        setters.channel_opened_ms,
        setters.state,
        &mut keepalive,
    );
    install_state_logging(&pc, setters.ice_state, setters.signaling_state, &mut keepalive);

    setters.state.set(RtcConn::Gathering);
    let offer = JsFuture::from(pc.create_offer()).await?;
    let offer_desc: RtcSessionDescriptionInit = offer.unchecked_into();
    JsFuture::from(pc.set_local_description(&offer_desc)).await?;
    wait_for_ice_complete(&pc, &mut keepalive).await;
    setters.gather_done_ms.set(Some(perf_now() - now));
    setters.state.set(RtcConn::Ready);
    // Push initial state into the diag panel so the user sees the right
    // values even if no event fires before the first render tick.
    setters.signaling_state.set(format!("{:?}", pc.signaling_state()));
    setters.ice_state.set(format!("{:?}", pc.ice_connection_state()));

    let sdp = local_description_sdp(&pc).unwrap_or_default();
    let session = PeerSession {
        pc,
        dc: dc_holder,
        state: setters.state,
        messages: setters.messages,
        gather_started_ms: now,
        gather_done_ms: setters.gather_done_ms,
        channel_opened_ms: setters.channel_opened_ms,
        ice_state: setters.ice_state,
        signaling_state: setters.signaling_state,
        _keepalive: keepalive,
    };
    Ok((session, encode_sdp("offer", &sdp)))
}
// SECTION: open_joiner

/// Joiner side: take the host's offer, generate an answer, wait for ICE
/// gathering, return the answer SDP for the user to send back.
///
/// Joiner does NOT call `create_data_channel` — it discovers the
/// channel via `pc.ondatachannel`.
pub async fn open_joiner(
    offer_blob: &str,
    setters: OpenSetters,
    ice_mode: IceMode,
) -> Result<(PeerSession, String), JsValue> {
    let now = perf_now();
    let pc = new_peer_connection(ice_mode)?;
    let dc_holder: Rc<RefCell<Option<RtcDataChannel>>> = Rc::new(RefCell::new(None));
    let mut keepalive: Vec<Closure<dyn FnMut(JsValue)>> = Vec::new();

    // Wire `ondatachannel` so we install handlers as soon as host's
    // DataChannel arrives.
    {
        let dc_holder = dc_holder.clone();
        let messages_setter = setters.messages;
        let channel_opened_setter = setters.channel_opened_ms;
        let state_setter = setters.state;
        let mut handlers_buf: Vec<Closure<dyn FnMut(JsValue)>> = Vec::new();
        let cb = Closure::wrap(Box::new(move |ev: JsValue| {
            let ev: RtcDataChannelEvent = ev.unchecked_into();
            let dc = ev.channel();
            install_dc_handlers(
                &dc,
                messages_setter,
                channel_opened_setter,
                state_setter,
                &mut handlers_buf,
            );
            *dc_holder.borrow_mut() = Some(dc);
        }) as Box<dyn FnMut(JsValue)>);
        pc.set_ondatachannel(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);
    }
    install_state_logging(&pc, setters.ice_state, setters.signaling_state, &mut keepalive);

    setters.state.set(RtcConn::Gathering);
    let offer_sdp = decode_offer(offer_blob)?;
    let offer_desc = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
    offer_desc.set_sdp(&offer_sdp);
    JsFuture::from(pc.set_remote_description(&offer_desc)).await?;
    let answer = JsFuture::from(pc.create_answer()).await?;
    let answer_desc: RtcSessionDescriptionInit = answer.unchecked_into();
    JsFuture::from(pc.set_local_description(&answer_desc)).await?;
    wait_for_ice_complete(&pc, &mut keepalive).await;
    setters.gather_done_ms.set(Some(perf_now() - now));
    setters.state.set(RtcConn::Ready);
    setters.signaling_state.set(format!("{:?}", pc.signaling_state()));
    setters.ice_state.set(format!("{:?}", pc.ice_connection_state()));

    let sdp = local_description_sdp(&pc).unwrap_or_default();
    let session = PeerSession {
        pc,
        dc: dc_holder,
        state: setters.state,
        messages: setters.messages,
        gather_started_ms: now,
        gather_done_ms: setters.gather_done_ms,
        channel_opened_ms: setters.channel_opened_ms,
        ice_state: setters.ice_state,
        signaling_state: setters.signaling_state,
        _keepalive: keepalive,
    };
    Ok((session, encode_sdp("answer", &sdp)))
}
// SECTION: accept_answer

/// Host calls this with the joiner's answer SDP to complete the handshake.
///
/// Pre-flights `signalingState` so we surface "page was backgrounded" /
/// "you tapped Start hosting twice" before throwing the inscrutable
/// browser error. Real chrome message in that case:
/// `InvalidStateError: Failed to set remote answer sdp: Called in wrong state: stable`.
pub async fn accept_answer(pc: &RtcPeerConnection, answer_blob: &str) -> Result<(), JsValue> {
    let signaling = pc.signaling_state();
    if signaling != RtcSignalingState::HaveLocalOffer {
        return Err(JsValue::from_str(&format!(
            "PeerConnection signaling state is `{signaling:?}` (expected `HaveLocalOffer`). \
             Common causes: (1) you tapped Start hosting more than once \u{2014} only the latest PC keeps the offer; \
             (2) iOS Safari paused the page (AirDrop / share sheet / app switch) and reset the RTC session. \
             Tap Reset, then redo the handshake without leaving the host page."
        )));
    }
    let answer_sdp = decode_answer(answer_blob)?;
    let answer_desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
    answer_desc.set_sdp(&answer_sdp);
    JsFuture::from(pc.set_remote_description(&answer_desc)).await?;
    Ok(())
}
// SECTION: helpers (sdp encode/decode, ICE wait)

fn perf_now() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now()).unwrap_or(0.0)
}

/// `RtcPeerConnection` configuration.
///
/// `LanOnly` (default for the spike) sets `iceServers: []` — pure mDNS /
/// `.local` candidates, no STUN, no TURN. This is the "no external
/// dependency" mode the spike is here to validate.
///
/// `WithStun` adds a Google public STUN server. With STUN the browser
/// also publishes server-reflexive (`srflx`) candidates with the public
/// IP. Useful as a triage tool when the LAN is blocking peer-to-peer
/// `.local` resolution (router AP isolation, guest WiFi, etc.) — if
/// the connection succeeds with STUN but fails without, the router is
/// the culprit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IceMode {
    LanOnly,
    WithStun,
}

fn new_peer_connection(mode: IceMode) -> Result<RtcPeerConnection, JsValue> {
    let cfg = RtcConfiguration::new();
    let servers: Array = Array::new();
    if mode == IceMode::WithStun {
        // Single Google public STUN server. Reachable from most networks
        // including most home / mobile / corporate. Not used by
        // production code — production wants pure LAN; this is a spike
        // diagnostic only.
        let server = js_sys::Object::new();
        Reflect::set(&server, &"urls".into(), &"stun:stun.l.google.com:19302".into()).ok();
        servers.push(&server);
    }
    Reflect::set(&cfg, &"iceServers".into(), &servers).ok();
    RtcPeerConnection::new_with_configuration(&cfg)
}

fn local_description_sdp(pc: &RtcPeerConnection) -> Option<String> {
    pc.local_description().map(|d| d.sdp())
}

/// Wrap `(type, sdp)` in a JSON envelope so the textarea contents survive
/// copy-paste through AirDrop / clipboard / IM apps that helpfully
/// "normalise" line endings (SDP per RFC 4566 demands CRLF; many
/// transports strip the CR).
///
/// Pretty-printed with 2-space indent so it eyeballs the same as the
/// SDP itself in the spike textareas — line count + size are easy to
/// compare against pure-SDP transmission.
pub fn encode_sdp(kind: &str, sdp: &str) -> String {
    // serde_json would pull a dep just for this; hand-format. The two
    // string fields can be embedded with simple escaping (backslash +
    // quote + control chars) which `escape_json_string` covers.
    format!("{{\n  \"type\": \"{}\",\n  \"sdp\": \"{}\"\n}}", kind, escape_json_string(sdp))
}

/// Inverse of [`encode_sdp`]. Returns `(type, sdp)`. Accepts both raw
/// SDP (back-compat with the first spike build) — if the blob doesn't
/// look like JSON, treat it as a bare SDP string and infer the type
/// from `m=` heuristics; here we just default to whatever the caller
/// expects (`open_joiner` knows it has an offer in hand).
fn decode_sdp_envelope(blob: &str) -> Result<(String, String), JsValue> {
    let trimmed = blob.trim();
    if trimmed.starts_with('{') {
        let parsed = js_sys::JSON::parse(trimmed)
            .map_err(|e| JsValue::from_str(&format!("SDP envelope is not valid JSON: {e:?}")))?;
        let type_v = Reflect::get(&parsed, &"type".into()).ok();
        let sdp_v = Reflect::get(&parsed, &"sdp".into()).ok();
        let kind = type_v.and_then(|v| v.as_string()).unwrap_or_default();
        let sdp = sdp_v.and_then(|v| v.as_string()).unwrap_or_default();
        if kind.is_empty() || sdp.is_empty() {
            return Err(JsValue::from_str("SDP envelope JSON missing `type` or `sdp` field"));
        }
        // Browsers accept LF-only SDP in practice but normalise anyway —
        // copy-paste through AirDrop / SMS / IM may strip CR.
        Ok((kind, normalise_crlf(&sdp)))
    } else {
        // Raw SDP — assume the caller knows the type.
        Err(JsValue::from_str(
            "SDP must be the JSON envelope produced by the host page (this build dropped raw-SDP support — copy the entire textarea contents)",
        ))
    }
}

fn normalise_crlf(s: &str) -> String {
    // Drop existing \r so we don't double them, then expand \n → \r\n.
    let stripped = s.replace('\r', "");
    stripped.replace('\n', "\r\n")
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

/// Decode an offer blob — caller asserts `kind == "offer"` because
/// `open_joiner` is the only consumer.
pub fn decode_offer(blob: &str) -> Result<String, JsValue> {
    let (kind, sdp) = decode_sdp_envelope(blob)?;
    if kind != "offer" {
        return Err(JsValue::from_str(&format!("expected offer envelope, got `{kind}`")));
    }
    Ok(sdp)
}

/// Decode an answer blob — `accept_answer`'s only consumer.
pub fn decode_answer(blob: &str) -> Result<String, JsValue> {
    let (kind, sdp) = decode_sdp_envelope(blob)?;
    if kind != "answer" {
        return Err(JsValue::from_str(&format!("expected answer envelope, got `{kind}`")));
    }
    Ok(sdp)
}

/// Block until `pc.iceGatheringState == "complete"`. Browsers gather ICE
/// asynchronously; for trickle-less SDP exchange (i.e. the QR/text path
/// the spike is testing) we MUST wait.
async fn wait_for_ice_complete(
    pc: &RtcPeerConnection,
    keepalive: &mut Vec<Closure<dyn FnMut(JsValue)>>,
) {
    if pc.ice_gathering_state() == RtcIceGatheringState::Complete {
        return;
    }
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let pc_inner = pc.clone();
        let resolve = resolve;
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            if pc_inner.ice_gathering_state() == RtcIceGatheringState::Complete {
                let _ = resolve.call0(&JsValue::NULL);
            }
        }) as Box<dyn FnMut(JsValue)>);
        pc.set_onicegatheringstatechange(Some(cb.as_ref().unchecked_ref()));
        // Keep the closure alive for as long as the connection lives.
        // The vec is moved into the PeerSession so it lives until the
        // session is dropped.
        keepalive.push(cb);
    });
    let _ = JsFuture::from(promise).await;
}

/// Wire DataChannel `onopen` / `onmessage` / `onclose` to the page's
/// signals. Used by both host and joiner branches.
fn install_dc_handlers(
    dc: &RtcDataChannel,
    messages: WriteSignal<Vec<String>>,
    channel_opened_at: WriteSignal<Option<f64>>,
    state: WriteSignal<RtcConn>,
    keepalive: &mut Vec<Closure<dyn FnMut(JsValue)>>,
) {
    {
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            channel_opened_at.set(Some(perf_now()));
            state.set(RtcConn::Connected);
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onopen(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);
    }
    {
        let cb = Closure::wrap(Box::new(move |ev: JsValue| {
            let ev: MessageEvent = ev.unchecked_into();
            if let Some(text) = ev.data().as_string() {
                messages.update(|v| v.push(format!("← peer: {text}")));
            }
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onmessage(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);
    }
    {
        let dc_for_state = dc.clone();
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            if dc_for_state.ready_state() == RtcDataChannelState::Closed {
                state.set(RtcConn::Failed);
                messages.update(|v| v.push("[dc closed]".to_string()));
            }
        }) as Box<dyn FnMut(JsValue)>);
        dc.set_onclose(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);
    }
}

/// Mirror the four RTC state machines into the diagnostic panel:
///
/// - `iceConnectionState` — the legacy "is the peer reachable?" view
/// - `signalingState` — what stage of offer/answer we're in
///   (`stable` ↔ `have-local-offer` ↔ `stable` after answer)
/// - `iceGatheringState` — `new` → `gathering` → `complete`
///
/// The fourth (`connectionState` aka the unified peer-connection state)
/// is sampled in [`PeerSession::connection_state_str`] on demand because
/// `onconnectionstatechange` isn't always fired the same way across
/// browsers (Safari was late to it). The page polls it after each user
/// action instead.
fn install_state_logging(
    pc: &RtcPeerConnection,
    ice_state: WriteSignal<String>,
    signaling_state: WriteSignal<String>,
    keepalive: &mut Vec<Closure<dyn FnMut(JsValue)>>,
) {
    {
        let pc_inner = pc.clone();
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            let s = format!("{:?}", pc_inner.ice_connection_state());
            ice_state.set(s);
        }) as Box<dyn FnMut(JsValue)>);
        pc.set_oniceconnectionstatechange(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);
    }
    {
        let pc_inner = pc.clone();
        let cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            let s = format!("{:?}", pc_inner.signaling_state());
            signaling_state.set(s);
        }) as Box<dyn FnMut(JsValue)>);
        pc.set_onsignalingstatechange(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);
    }
    // Optional: log each ICE candidate as it's gathered (useful for
    // confirming `.local` / mDNS candidates appear in the JS console).
    {
        let cb = Closure::wrap(Box::new(move |ev: JsValue| {
            let ev: RtcPeerConnectionIceEvent = ev.unchecked_into();
            if let Some(c) = ev.candidate() {
                let cand_str = c.candidate();
                web_sys::console::log_2(&"[spike] ICE candidate:".into(), &cand_str.into());
            }
        }) as Box<dyn FnMut(JsValue)>);
        pc.set_onicecandidate(Some(cb.as_ref().unchecked_ref()));
        keepalive.push(cb);
    }
}

/// Public helper for the page's chat input — write straight into the
/// DataChannel if it's open.
pub fn dc_send(dc: &RtcDataChannel, text: &str) -> Result<(), JsValue> {
    if dc.ready_state() != RtcDataChannelState::Open {
        return Err(JsValue::from_str("data channel not open"));
    }
    dc.send_with_str(text)
}
