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
    RtcSdpType, RtcSessionDescriptionInit,
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

/// Host side: create the PeerConnection + DataChannel, generate the
/// offer, wait for ICE gathering to complete, return the full SDP blob.
///
/// `_label` is the DataChannel label — anything stable; the joiner sees
/// the same name. We default to `"chess-spike"`.
///
/// `state_setter` / `messages_setter` etc. are the page's writable
/// signals; the closures hold them and update reactively as RTC
/// callbacks fire.
pub async fn open_host(
    state_setter: WriteSignal<RtcConn>,
    messages_setter: WriteSignal<Vec<String>>,
    gather_done_setter: WriteSignal<Option<f64>>,
    channel_opened_setter: WriteSignal<Option<f64>>,
    ice_state_setter: WriteSignal<String>,
) -> Result<(PeerSession, String), JsValue> {
    let now = perf_now();
    let pc = new_peer_connection_no_servers()?;
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

    install_dc_handlers(&dc, messages_setter, channel_opened_setter, state_setter, &mut keepalive);
    install_ice_state_logging(&pc, ice_state_setter, &mut keepalive);

    state_setter.set(RtcConn::Gathering);
    let offer = JsFuture::from(pc.create_offer()).await?;
    let offer_desc: RtcSessionDescriptionInit = offer.unchecked_into();
    JsFuture::from(pc.set_local_description(&offer_desc)).await?;
    wait_for_ice_complete(&pc, &mut keepalive).await;
    gather_done_setter.set(Some(perf_now() - now));
    state_setter.set(RtcConn::Ready);

    let sdp = local_description_sdp(&pc).unwrap_or_default();
    let session = PeerSession {
        pc,
        dc: dc_holder,
        state: state_setter,
        messages: messages_setter,
        gather_started_ms: now,
        gather_done_ms: gather_done_setter,
        channel_opened_ms: channel_opened_setter,
        ice_state: ice_state_setter,
        _keepalive: keepalive,
    };
    Ok((session, encode_sdp(&sdp)))
}
// SECTION: open_joiner

/// Joiner side: take the host's offer, generate an answer, wait for ICE
/// gathering, return the answer SDP for the user to send back.
///
/// Joiner does NOT call `create_data_channel` — it discovers the
/// channel via `pc.ondatachannel`.
pub async fn open_joiner(
    offer_blob: &str,
    state_setter: WriteSignal<RtcConn>,
    messages_setter: WriteSignal<Vec<String>>,
    gather_done_setter: WriteSignal<Option<f64>>,
    channel_opened_setter: WriteSignal<Option<f64>>,
    ice_state_setter: WriteSignal<String>,
) -> Result<(PeerSession, String), JsValue> {
    let now = perf_now();
    let pc = new_peer_connection_no_servers()?;
    let dc_holder: Rc<RefCell<Option<RtcDataChannel>>> = Rc::new(RefCell::new(None));
    let mut keepalive: Vec<Closure<dyn FnMut(JsValue)>> = Vec::new();

    // Wire `ondatachannel` so we install handlers as soon as host's
    // DataChannel arrives.
    {
        let dc_holder = dc_holder.clone();
        let messages_setter = messages_setter;
        let channel_opened_setter = channel_opened_setter;
        let state_setter = state_setter;
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
    install_ice_state_logging(&pc, ice_state_setter, &mut keepalive);

    state_setter.set(RtcConn::Gathering);
    let offer_sdp = decode_sdp(offer_blob)?;
    let offer_desc = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
    offer_desc.set_sdp(&offer_sdp);
    JsFuture::from(pc.set_remote_description(&offer_desc)).await?;
    let answer = JsFuture::from(pc.create_answer()).await?;
    let answer_desc: RtcSessionDescriptionInit = answer.unchecked_into();
    JsFuture::from(pc.set_local_description(&answer_desc)).await?;
    wait_for_ice_complete(&pc, &mut keepalive).await;
    gather_done_setter.set(Some(perf_now() - now));
    state_setter.set(RtcConn::Ready);

    let sdp = local_description_sdp(&pc).unwrap_or_default();
    let session = PeerSession {
        pc,
        dc: dc_holder,
        state: state_setter,
        messages: messages_setter,
        gather_started_ms: now,
        gather_done_ms: gather_done_setter,
        channel_opened_ms: channel_opened_setter,
        ice_state: ice_state_setter,
        _keepalive: keepalive,
    };
    Ok((session, encode_sdp(&sdp)))
}
// SECTION: accept_answer

/// Host calls this with the joiner's answer SDP to complete the handshake.
pub async fn accept_answer(pc: &RtcPeerConnection, answer_blob: &str) -> Result<(), JsValue> {
    let answer_sdp = decode_sdp(answer_blob)?;
    let answer_desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
    answer_desc.set_sdp(&answer_sdp);
    JsFuture::from(pc.set_remote_description(&answer_desc)).await?;
    Ok(())
}
// SECTION: helpers (sdp encode/decode, ICE wait)

fn perf_now() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now()).unwrap_or(0.0)
}

/// `RtcPeerConnection` with `iceServers: []` — pure mDNS / .local
/// candidates, no STUN, no TURN. This is the "LAN only" mode the spike
/// is here to validate.
fn new_peer_connection_no_servers() -> Result<RtcPeerConnection, JsValue> {
    let cfg = RtcConfiguration::new();
    let empty: Array = Array::new();
    Reflect::set(&cfg, &"iceServers".into(), &empty).ok();
    RtcPeerConnection::new_with_configuration(&cfg)
}

fn local_description_sdp(pc: &RtcPeerConnection) -> Option<String> {
    pc.local_description().map(|d| d.sdp())
}

/// SDP blobs are textareas in the spike — no compression, just verbatim
/// SDP text. We keep `encode`/`decode` as fns so Phase 5 can swap in
/// deflate+base64 without churning callers. (For the spike, we ALSO
/// want to see the raw text in DevTools to count bytes.)
pub fn encode_sdp(sdp: &str) -> String {
    sdp.to_string()
}

fn decode_sdp(blob: &str) -> Result<String, JsValue> {
    Ok(blob.trim().to_string())
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

/// Mirror `iceConnectionState` into the diagnostic panel so the user can
/// see e.g. `checking → connected → completed` (or `failed`) in real time.
fn install_ice_state_logging(
    pc: &RtcPeerConnection,
    ice_state: WriteSignal<String>,
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
    // Optional: log each ICE candidate as it's gathered (useful for
    // confirming `.local` / mDNS candidates appear).
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
