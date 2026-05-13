//! `/lan/host` and `/lan/join` routes — LAN multiplayer over WebRTC.
//!
//! Phase 5 of `backlog/webrtc-lan-pairing.md`. Pairs two browsers
//! on the same WiFi via the Phase 3 `transport::webrtc` plumbing
//! and the Phase 4 `host_room::HostRoom` authority, then drops the
//! resulting `Session` into the existing `pages/play::PlayConnected`
//! so the actual game UI is unchanged from chess-net mode.
//!
//! No QR / camera scanning yet — Phase 5 v1 uses textareas + the
//! browser clipboard for the offer/answer exchange. QR is
//! deferred (Phase 5.5 if requested).

// === module skeleton — section bodies filled in via edits below ===

// SECTION: imports
use std::cell::RefCell;
use std::rc::Rc;

use chess_core::rules::RuleSet;
use leptos::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlTextAreaElement;

use crate::components::qr::QrCodeView;
use crate::config::WsBase;
use crate::host_room::HostRoom;
use crate::pages::play::PlayConnected;
use crate::transport::webrtc::{
    connect_as_host, connect_as_joiner, wait_for_dc_open, AnswerBlob, HostHandshake, IceMode,
    JoinerHandshake, OfferBlob, WebRtcConfig,
};
use crate::transport::{ConnState, Session};
// SECTION: LanHostPage component

#[component]
pub fn LanHostPage() -> impl IntoView {
    // ── Page state ────────────────────────────────────────────
    // We progress through:
    //   Idle → Generating → AwaitingAnswer → AcceptingAnswer →
    //   WaitingForChannel → Playing
    // Each transition mutates the corresponding signals; the view
    // renders accordingly.
    let (status, set_status) = create_signal::<HostStatus>(HostStatus::Idle);
    let (use_stun, set_use_stun) = create_signal::<bool>(false);
    let (offer_blob, set_offer_blob) = create_signal::<String>(String::new());
    let (answer_input, set_answer_input) = create_signal::<String>(String::new());
    let (error_msg, set_error_msg) = create_signal::<Option<String>>(None);

    // Keep the in-flight HostHandshake alive in the page; closures
    // and accept_answer need a stable reference.
    let handshake: Rc<RefCell<Option<HostHandshake>>> = Rc::new(RefCell::new(None));
    // The HostRoom + its Session aren't constructed until the
    // DataChannel actually opens; on `state == Open` we instantiate
    // both and flip status to `Playing`.
    let host_room: Rc<RefCell<Option<Rc<HostRoom>>>> = Rc::new(RefCell::new(None));
    let (play_session, set_play_session) = create_signal::<Option<Session>>(None);

    // ── Open room (generate offer) ────────────────────────────
    let handshake_for_open = handshake.clone();
    let on_open: Callback<()> = Callback::new(move |_: ()| {
        if !matches!(status.get_untracked(), HostStatus::Idle) {
            return;
        }
        set_error_msg.set(None);
        set_status.set(HostStatus::Generating);
        let cfg = WebRtcConfig {
            ice_mode: if use_stun.get_untracked() { IceMode::WithStun } else { IceMode::LanOnly },
        };
        let handshake_slot = handshake_for_open.clone();
        spawn_local(async move {
            match connect_as_host(cfg).await {
                Ok(hh) => {
                    set_offer_blob.set(hh.offer.0.clone());
                    *handshake_slot.borrow_mut() = Some(hh);
                    set_status.set(HostStatus::AwaitingAnswer);
                }
                Err(e) => {
                    set_error_msg.set(Some(format!("connect_as_host failed: {e:?}")));
                    set_status.set(HostStatus::Idle);
                }
            }
        });
    });

    // ── Accept answer ──────────────────────────────────────────
    let handshake_for_accept = handshake.clone();
    let host_room_slot = host_room.clone();
    let on_accept: Callback<()> = Callback::new(move |_: ()| {
        if !matches!(status.get_untracked(), HostStatus::AwaitingAnswer) {
            return;
        }
        let blob = answer_input.get_untracked();
        if blob.trim().is_empty() {
            set_error_msg.set(Some("paste the joiner's answer SDP first".into()));
            return;
        }
        set_error_msg.set(None);
        set_status.set(HostStatus::AcceptingAnswer);
        let handshake_slot = handshake_for_accept.clone();
        let host_room_slot = host_room_slot.clone();
        spawn_local(async move {
            let hh = match handshake_slot.borrow_mut().take() {
                Some(hh) => hh,
                None => {
                    set_error_msg.set(Some("no handshake in flight".into()));
                    set_status.set(HostStatus::Idle);
                    return;
                }
            };
            if let Err(e) = hh.accept_answer(AnswerBlob(blob)).await {
                set_error_msg.set(Some(format!("accept_answer failed: {e:?}")));
                set_status.set(HostStatus::Idle);
                return;
            }
            // CRITICAL: `accept_answer` resolves as soon as
            // `setRemoteDescription` returns — BEFORE the SCTP
            // handshake completes. If we call `attach_remote_player_dc`
            // immediately, the `dc.send_with_str(...)` for Hello +
            // ChatHistory silently fails (DC ready_state is still
            // "connecting"). Wait for actual DC open first.
            let dc = match hh.dc.borrow().clone() {
                Some(d) => d,
                None => {
                    set_error_msg.set(Some("DataChannel slot is empty".into()));
                    set_status.set(HostStatus::Idle);
                    return;
                }
            };
            if !wait_for_dc_open(&dc, 10_000).await {
                set_error_msg.set(Some(
                    "DataChannel did not open within 10 s — pairing failed (network blocked?)"
                        .into(),
                ));
                set_status.set(HostStatus::Idle);
                return;
            }
            let (room, session) = HostRoom::new(RuleSet::xiangqi(), None, /* hints */ false);
            if let Err(e) = room.attach_remote_player_dc(dc) {
                set_error_msg.set(Some(format!("attach joiner failed: {e:?}")));
                set_status.set(HostStatus::Idle);
                return;
            }
            *host_room_slot.borrow_mut() = Some(room);
            set_play_session.set(Some(session));
            set_status.set(HostStatus::Playing);
        });
    });

    // ── Reset (full page reload) ──────────────────────────────
    let on_reset: Callback<()> = Callback::new(move |_: ()| {
        if let Some(win) = web_sys::window() {
            let _ = win.location().reload();
        }
    });

    view! {
        <div class="lan-page" style="max-width:720px;margin:1.5rem auto;padding:1rem">
            <Show
                when=move || matches!(status.get(), HostStatus::Playing)
                fallback=move || view! {
                    <h1>"LAN host (WebRTC)"</h1>
                    <p class="muted">
                        "iOS hint: do not switch apps after tapping Open room. iOS Safari pauses \
                         WebRTC when the page is backgrounded. If you must AirDrop the offer, \
                         keep this Safari tab in the foreground (split-view works)."
                    </p>
                    <p>
                        <button
                            on:click=move |_| on_open.call(())
                            disabled=move || !matches!(status.get(), HostStatus::Idle)
                        >
                            "1. Open room"
                        </button>
                        <button on:click=move |_| on_reset.call(()) style="margin-left:0.5rem">
                            "Reset"
                        </button>
                        <label style="margin-left:1rem;font-size:13px">
                            <input
                                type="checkbox"
                                prop:checked=move || use_stun.get()
                                disabled=move || !matches!(status.get(), HostStatus::Idle)
                                on:change=move |ev| {
                                    let v = ev.target()
                                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                        .map(|el| el.checked())
                                        .unwrap_or(false);
                                    set_use_stun.set(v);
                                }
                            />
                            " Use STUN (workaround for hostile networks)"
                        </label>
                    </p>
                    <p>"Status: " {move || format!("{:?}", status.get())}</p>
                    <Show when=move || !offer_blob.with(|s| s.is_empty())>
                        <p>"Have the joiner scan this QR — or copy the text and AirDrop / Messages it:"</p>
                        <QrCodeView
                            payload=Signal::derive(move || offer_blob.get())
                            label="LAN host offer SDP"
                        />
                        <p style="margin-top:0.75rem">"Or copy as text:"</p>
                        <textarea
                            rows="6"
                            readonly=true
                            style="width:100%;font-family:monospace;font-size:12px"
                            prop:value=move || offer_blob.get()
                        />
                        <p>
                            <button on:click=move |_| {
                                let blob = offer_blob.get();
                                if let Some(win) = web_sys::window() {
                                    let _ = win.navigator().clipboard().write_text(&blob);
                                }
                            }>
                                "Copy offer"
                            </button>
                            <span style="margin-left:0.5rem;font-size:12px;color:#666">
                                "Bytes: " {move || offer_blob.with(|s| s.len())}
                            </span>
                        </p>
                        <p>"2. Paste the joiner's answer SDP below, then accept:"</p>
                        <textarea
                            rows="6"
                            style="width:100%;font-family:monospace;font-size:12px"
                            on:input=move |ev| {
                                let v = ev.target()
                                    .and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok())
                                    .map(|el| el.value())
                                    .unwrap_or_default();
                                set_answer_input.set(v);
                            }
                        />
                        <button
                            on:click=move |_| on_accept.call(())
                            disabled=move || !matches!(status.get(), HostStatus::AwaitingAnswer)
                        >
                            "3. Accept answer"
                        </button>
                    </Show>
                    {move || error_msg.get().map(|e| view! {
                        <p style="color:#c33;margin-top:1rem">"ERROR: " {e}</p>
                    })}
                }
            >
                {move || play_session.get().map(|s| view! {
                    <PlayConnected
                        ws_base=lan_dummy_ws_base()
                        room="lan-host".to_string()
                        password=None
                        watch_only=false
                        debug_enabled=false
                        hints_requested=false
                        injected_session=s
                        back_link_override="/lan/host".to_string()
                    />
                })}
            </Show>
        </div>
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum HostStatus {
    Idle,
    Generating,
    AwaitingAnswer,
    AcceptingAnswer,
    Playing,
}
// SECTION: LanJoinPage component

#[component]
pub fn LanJoinPage() -> impl IntoView {
    let (status, set_status) = create_signal::<JoinStatus>(JoinStatus::Idle);
    let (use_stun, set_use_stun) = create_signal::<bool>(false);
    let (offer_input, set_offer_input) = create_signal::<String>(String::new());
    let (answer_blob, set_answer_blob) = create_signal::<String>(String::new());
    let (error_msg, set_error_msg) = create_signal::<Option<String>>(None);

    // Joiner side: store the JoinerHandshake so we can pull its
    // session out once the DC opens.
    let handshake: Rc<RefCell<Option<JoinerHandshake>>> = Rc::new(RefCell::new(None));
    let (play_session, set_play_session) = create_signal::<Option<Session>>(None);

    // Holder pattern: spawn_local runs without a Leptos owner context,
    // so a `create_effect` inside it would be GC'd immediately (no
    // subscriptions retained — `state.set(Open)` would fire but no
    // effect would react). Solution: define the holder signals and
    // the effect at component scope (properly owned), and have
    // spawn_local just `set` the holders. The component-scope effect
    // re-runs when the holders change AND when the inner state
    // signal changes.
    let (joiner_state_holder, set_joiner_state_holder) =
        create_signal::<Option<ReadSignal<ConnState>>>(None);
    let session_holder: Rc<RefCell<Option<Session>>> = Rc::new(RefCell::new(None));

    {
        let session_holder = session_holder.clone();
        create_effect(move |_| {
            // Read both signals to register subscriptions:
            //   * joiner_state_holder fires once when handshake completes.
            //   * the inner state signal fires when DC opens.
            if let Some(state_sig) = joiner_state_holder.get() {
                if state_sig.get() == ConnState::Open {
                    if let Some(session) = session_holder.borrow().clone() {
                        set_play_session.set(Some(session));
                        set_status.set(JoinStatus::Playing);
                    }
                }
            }
        });
    }

    let handshake_for_gen = handshake.clone();
    let session_holder_for_gen = session_holder.clone();
    let on_generate: Callback<()> = Callback::new(move |_: ()| {
        if !matches!(status.get_untracked(), JoinStatus::Idle) {
            return;
        }
        let blob = offer_input.get_untracked();
        if blob.trim().is_empty() {
            set_error_msg.set(Some("paste the host's offer SDP first".into()));
            return;
        }
        set_error_msg.set(None);
        set_status.set(JoinStatus::Generating);
        let cfg = WebRtcConfig {
            ice_mode: if use_stun.get_untracked() { IceMode::WithStun } else { IceMode::LanOnly },
        };
        let handshake_slot = handshake_for_gen.clone();
        let session_holder = session_holder_for_gen.clone();
        spawn_local(async move {
            match connect_as_joiner(cfg, OfferBlob(blob)).await {
                Ok(jh) => {
                    set_answer_blob.set(jh.answer.0.clone());
                    let state = jh.session.state;
                    *session_holder.borrow_mut() = Some(jh.session.clone());
                    *handshake_slot.borrow_mut() = Some(jh);
                    set_status.set(JoinStatus::WaitingForOpen);
                    // Triggers the component-scope effect to subscribe
                    // to the state signal. Once DC opens, that effect
                    // flips status → Playing.
                    set_joiner_state_holder.set(Some(state));
                }
                Err(e) => {
                    set_error_msg.set(Some(format!("connect_as_joiner failed: {e:?}")));
                    set_status.set(JoinStatus::Idle);
                }
            }
        });
    });

    let on_reset: Callback<()> = Callback::new(move |_: ()| {
        if let Some(win) = web_sys::window() {
            let _ = win.location().reload();
        }
    });

    view! {
        <div class="lan-page" style="max-width:720px;margin:1.5rem auto;padding:1rem">
            <Show
                when=move || matches!(status.get(), JoinStatus::Playing)
                fallback=move || view! {
                    <h1>"LAN join (WebRTC)"</h1>
                    <p class="muted">
                        "Paste the host's offer SDP, generate an answer, send it back to the host."
                    </p>
                    <p>
                        <button on:click=move |_| on_reset.call(())>"Reset"</button>
                        <label style="margin-left:1rem;font-size:13px">
                            <input
                                type="checkbox"
                                prop:checked=move || use_stun.get()
                                disabled=move || !matches!(status.get(), JoinStatus::Idle)
                                on:change=move |ev| {
                                    let v = ev.target()
                                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                                        .map(|el| el.checked())
                                        .unwrap_or(false);
                                    set_use_stun.set(v);
                                }
                            />
                            " Use STUN (must match host's setting)"
                        </label>
                    </p>
                    <p>"Status: " {move || format!("{:?}", status.get())}</p>
                    <p>"1. Paste host's offer SDP:"</p>
                    <textarea
                        rows="8"
                        style="width:100%;font-family:monospace;font-size:12px"
                        on:input=move |ev| {
                            let v = ev.target()
                                .and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok())
                                .map(|el| el.value())
                                .unwrap_or_default();
                            set_offer_input.set(v);
                        }
                    />
                    <button
                        on:click=move |_| on_generate.call(())
                        disabled=move || !matches!(status.get(), JoinStatus::Idle)
                    >
                        "2. Generate answer"
                    </button>
                    <Show when=move || !answer_blob.with(|s| s.is_empty())>
                        <p>"Show this QR back to the host — or copy + AirDrop the text:"</p>
                        <QrCodeView
                            payload=Signal::derive(move || answer_blob.get())
                            label="LAN joiner answer SDP"
                        />
                        <p style="margin-top:0.75rem">"Or copy as text:"</p>
                        <textarea
                            rows="6"
                            readonly=true
                            style="width:100%;font-family:monospace;font-size:12px"
                            prop:value=move || answer_blob.get()
                        />
                        <p>
                            <button on:click=move |_| {
                                let blob = answer_blob.get();
                                if let Some(win) = web_sys::window() {
                                    let _ = win.navigator().clipboard().write_text(&blob);
                                }
                            }>
                                "Copy answer"
                            </button>
                            <span style="margin-left:0.5rem;font-size:12px;color:#666">
                                "Bytes: " {move || answer_blob.with(|s| s.len())}
                            </span>
                        </p>
                        <p class="muted" style="font-size:13px">
                            "Waiting for host to accept the answer + the DataChannel to open. \
                             Once host taps 'Accept answer' on their side, this page should \
                             flip to the game view."
                        </p>
                    </Show>
                    {move || error_msg.get().map(|e| view! {
                        <p style="color:#c33;margin-top:1rem">"ERROR: " {e}</p>
                    })}
                }
            >
                {move || play_session.get().map(|s| view! {
                    <PlayConnected
                        ws_base=lan_dummy_ws_base()
                        room="lan-join".to_string()
                        password=None
                        watch_only=false
                        debug_enabled=false
                        hints_requested=false
                        injected_session=s
                        back_link_override="/lan/join".to_string()
                    />
                })}
            </Show>
        </div>
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum JoinStatus {
    Idle,
    Generating,
    WaitingForOpen,
    Playing,
}
// SECTION: shared UI helpers (textarea + Copy button + link back)

/// LAN play has no chess-net `WsBase` — but `PlayConnected` still
/// requires one for the OnlineSidebar's "back to lobby" path
/// (which we override anyway via `back_link_override`). Construct
/// a stub `WsBase` whose `base` is empty + `persist_in_urls = false`
/// so any code path that DID happen to use it for URL building
/// produces something obviously LAN-shaped.
fn lan_dummy_ws_base() -> WsBase {
    WsBase { base: String::new(), persist_in_urls: false }
}
