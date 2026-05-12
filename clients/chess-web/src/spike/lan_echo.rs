//! `/spike/lan/host` and `/spike/lan/join` pages — see `super` doc.

// === module skeleton — section bodies filled in via edits below ===

// SECTION: imports
use std::cell::RefCell;
use std::rc::Rc;

use leptos::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlTextAreaElement;

use super::rtc::{accept_answer, dc_send, open_host, open_joiner, PeerSession, RtcConn};
// SECTION: route component (HostPage)

#[component]
pub fn HostPage() -> impl IntoView {
    // Diagnostic + reactive signals shared with the RTC plumbing.
    let (state, set_state) = create_signal(RtcConn::Idle);
    let (messages, set_messages) = create_signal::<Vec<String>>(vec![]);
    let (gather_done_ms, set_gather_done_ms) = create_signal::<Option<f64>>(None);
    let (channel_opened_ms, set_channel_opened_ms) = create_signal::<Option<f64>>(None);
    let (ice_state, set_ice_state) = create_signal::<String>(String::from("idle"));

    let (offer_blob, set_offer_blob) = create_signal::<String>(String::new());
    let (answer_blob, set_answer_blob) = create_signal::<String>(String::new());
    let (error_msg, set_error_msg) = create_signal::<Option<String>>(None);

    // The PeerSession lives in an Rc<RefCell<...>> so we can:
    //   1. own it from `start_host` (writes Some after open_host completes),
    //   2. read the inner DataChannel from the chat input handler,
    //   3. read the PeerConnection from the answer-paste handler.
    let session: Rc<RefCell<Option<PeerSession>>> = Rc::new(RefCell::new(None));

    let session_for_start = session.clone();
    let start_host = move |_| {
        set_error_msg.set(None);
        set_offer_blob.set(String::new());
        let session = session_for_start.clone();
        spawn_local(async move {
            match open_host(
                set_state,
                set_messages,
                set_gather_done_ms,
                set_channel_opened_ms,
                set_ice_state,
            )
            .await
            {
                Ok((s, sdp)) => {
                    set_offer_blob.set(sdp);
                    *session.borrow_mut() = Some(s);
                }
                Err(e) => {
                    set_error_msg.set(Some(format!("open_host failed: {e:?}")));
                }
            }
        });
    };

    let session_for_answer = session.clone();
    let accept_answer_click = move |_| {
        set_error_msg.set(None);
        let blob = answer_blob.get();
        if blob.trim().is_empty() {
            set_error_msg.set(Some("paste the joiner's answer SDP first".into()));
            return;
        }
        let session = session_for_answer.clone();
        spawn_local(async move {
            // Reach into the cell to grab the PeerConnection. Cloning
            // the PC is cheap (it's a JS object handle).
            let pc = session.borrow().as_ref().map(|s| s.pc.clone());
            let pc = match pc {
                Some(pc) => pc,
                None => {
                    set_error_msg.set(Some("start the host first".into()));
                    return;
                }
            };
            if let Err(e) = accept_answer(&pc, &blob).await {
                set_error_msg.set(Some(format!("accept_answer failed: {e:?}")));
            }
        });
    };

    let (chat_input, set_chat_input) = create_signal::<String>(String::new());
    let session_for_chat = session.clone();
    let send_chat = move |_| {
        let text = chat_input.get_untracked();
        if text.trim().is_empty() {
            return;
        }
        let session = session_for_chat.borrow();
        let dc = session.as_ref().and_then(|s| s.dc.borrow().clone());
        if let Some(dc) = dc {
            match dc_send(&dc, &text) {
                Ok(()) => {
                    set_messages.update(|v| v.push(format!("→ me: {text}")));
                    set_chat_input.set(String::new());
                }
                Err(e) => set_error_msg.set(Some(format!("send failed: {e:?}"))),
            }
        } else {
            set_error_msg.set(Some("no data channel yet".into()));
        }
    };

    view! {
        <div class="spike-page">
            <h1>"WebRTC LAN spike — Host"</h1>
            <p class="muted">
                "Phase 0 spike for "
                <code>"backlog/webrtc-lan-pairing.md"</code>
                ". iceServers = []; mDNS-only ICE."
            </p>
            <button on:click=start_host>"1. Start hosting"</button>
            <p>"Offer SDP (send to joiner via AirDrop / Nearby Share / paste):"</p>
            <textarea
                rows="8"
                readonly=true
                style="width:100%;font-family:monospace;font-size:12px"
                prop:value=move || offer_blob.get()
            />
            <p>
                "Offer raw bytes: " {move || offer_blob.with(|s| s.len())}
                <CopyButton text=Signal::derive(move || offer_blob.get()) label="Copy offer"/>
            </p>
            <p>"2. Paste joiner's answer SDP below, then accept:"</p>
            <textarea
                rows="8"
                style="width:100%;font-family:monospace;font-size:12px"
                on:input=move |ev| {
                    let v = ev.target()
                        .and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok())
                        .map(|el| el.value())
                        .unwrap_or_default();
                    set_answer_blob.set(v);
                }
            />
            <button on:click=accept_answer_click>"3. Accept answer"</button>
            <DiagPanel
                state=state
                ice_state=ice_state
                gather_done_ms=gather_done_ms
                channel_opened_ms=channel_opened_ms
                offer_size=Signal::derive(move || offer_blob.with(|s| s.len()))
                error_msg=error_msg
            />
            <ChatBox
                messages=messages
                input=chat_input
                set_input=set_chat_input
                send=send_chat
                state=state
            />
        </div>
    }
}
// SECTION: route component (JoinPage)

#[component]
pub fn JoinPage() -> impl IntoView {
    let (state, set_state) = create_signal(RtcConn::Idle);
    let (messages, set_messages) = create_signal::<Vec<String>>(vec![]);
    let (gather_done_ms, set_gather_done_ms) = create_signal::<Option<f64>>(None);
    let (channel_opened_ms, set_channel_opened_ms) = create_signal::<Option<f64>>(None);
    let (ice_state, set_ice_state) = create_signal::<String>(String::from("idle"));

    let (offer_blob, set_offer_blob) = create_signal::<String>(String::new());
    let (answer_blob, set_answer_blob) = create_signal::<String>(String::new());
    let (error_msg, set_error_msg) = create_signal::<Option<String>>(None);

    let session: Rc<RefCell<Option<PeerSession>>> = Rc::new(RefCell::new(None));

    let session_for_join = session.clone();
    let do_join = move |_| {
        set_error_msg.set(None);
        set_answer_blob.set(String::new());
        let blob = offer_blob.get();
        if blob.trim().is_empty() {
            set_error_msg.set(Some("paste host offer first".into()));
            return;
        }
        let session = session_for_join.clone();
        spawn_local(async move {
            match open_joiner(
                &blob,
                set_state,
                set_messages,
                set_gather_done_ms,
                set_channel_opened_ms,
                set_ice_state,
            )
            .await
            {
                Ok((s, sdp)) => {
                    set_answer_blob.set(sdp);
                    *session.borrow_mut() = Some(s);
                }
                Err(e) => {
                    set_error_msg.set(Some(format!("open_joiner failed: {e:?}")));
                }
            }
        });
    };

    let (chat_input, set_chat_input) = create_signal::<String>(String::new());
    let session_for_chat = session.clone();
    let send_chat = move |_| {
        let text = chat_input.get_untracked();
        if text.trim().is_empty() {
            return;
        }
        let session = session_for_chat.borrow();
        let dc = session.as_ref().and_then(|s| s.dc.borrow().clone());
        if let Some(dc) = dc {
            match dc_send(&dc, &text) {
                Ok(()) => {
                    set_messages.update(|v| v.push(format!("→ me: {text}")));
                    set_chat_input.set(String::new());
                }
                Err(e) => set_error_msg.set(Some(format!("send failed: {e:?}"))),
            }
        } else {
            set_error_msg.set(Some("no data channel yet".into()));
        }
    };

    view! {
        <div class="spike-page">
            <h1>"WebRTC LAN spike — Join"</h1>
            <p class="muted">
                "Phase 0 spike. Paste host's offer, generate answer, send back."
            </p>
            <p>"1. Paste host's offer SDP:"</p>
            <textarea
                rows="8"
                style="width:100%;font-family:monospace;font-size:12px"
                on:input=move |ev| {
                    let v = ev.target()
                        .and_then(|t| t.dyn_into::<HtmlTextAreaElement>().ok())
                        .map(|el| el.value())
                        .unwrap_or_default();
                    set_offer_blob.set(v);
                }
            />
            <button on:click=do_join>"2. Generate answer"</button>
            <p>"Answer SDP (send back to host):"</p>
            <textarea
                rows="8"
                readonly=true
                style="width:100%;font-family:monospace;font-size:12px"
                prop:value=move || answer_blob.get()
            />
            <p>
                "Answer raw bytes: " {move || answer_blob.with(|s| s.len())}
                <CopyButton text=Signal::derive(move || answer_blob.get()) label="Copy answer"/>
            </p>
            <DiagPanel
                state=state
                ice_state=ice_state
                gather_done_ms=gather_done_ms
                channel_opened_ms=channel_opened_ms
                offer_size=Signal::derive(move || answer_blob.with(|s| s.len()))
                error_msg=error_msg
            />
            <ChatBox
                messages=messages
                input=chat_input
                set_input=set_chat_input
                send=send_chat
                state=state
            />
        </div>
    }
}
// SECTION: shared diag panel

#[component]
fn DiagPanel(
    state: ReadSignal<RtcConn>,
    ice_state: ReadSignal<String>,
    gather_done_ms: ReadSignal<Option<f64>>,
    channel_opened_ms: ReadSignal<Option<f64>>,
    #[prop(into)] offer_size: Signal<usize>,
    error_msg: ReadSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <div class="diag" style="margin-top:1rem;padding:0.75rem;border:1px solid #888;font-family:monospace;font-size:12px">
            <p>"State: " {move || format!("{:?}", state.get())}</p>
            <p>"ICE state: " {move || ice_state.get()}</p>
            <p>"ICE gather done at: " {move || gather_done_ms.get().map(|m| format!("{m:.0} ms")).unwrap_or_else(|| "—".into())}</p>
            <p>"DataChannel opened at: " {move || channel_opened_ms.get().map(|m| format!("{m:.0} ms")).unwrap_or_else(|| "—".into())}</p>
            <p>"Local SDP bytes: " {move || offer_size.get()}</p>
            {move || error_msg.get().map(|e| view! { <p style="color:#c33">"ERROR: " {e}</p> })}
        </div>
    }
}

#[component]
fn ChatBox(
    messages: ReadSignal<Vec<String>>,
    input: ReadSignal<String>,
    set_input: WriteSignal<String>,
    #[prop(into)] send: Callback<()>,
    state: ReadSignal<RtcConn>,
) -> impl IntoView {
    let send_click = move |_| send.call(());
    view! {
        <div class="chat" style="margin-top:1rem">
            <h3>"Echo channel"</h3>
            <ul style="font-family:monospace;font-size:12px;border:1px solid #888;height:160px;overflow:auto;padding:0.5rem">
                <For each=move || messages.get() key=|m| m.clone() let:m>
                    <li>{m}</li>
                </For>
            </ul>
            <input
                type="text"
                prop:value=move || input.get()
                disabled=move || state.get() != RtcConn::Connected
                on:input=move |ev| {
                    let v = ev.target()
                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                        .map(|el| el.value())
                        .unwrap_or_default();
                    set_input.set(v);
                }
                style="width:80%;font-family:monospace"
            />
            <button on:click=send_click disabled=move || state.get() != RtcConn::Connected>
                "Send"
            </button>
        </div>
    }
}
// SECTION: helpers — reserved for future spike-only utilities. Kept as
// a marker so the section list at the top of the file matches the
// section comments below.

/// "Copy to clipboard" button + status flash. Long-press-select on
/// mobile textareas is fiddly; this gives the spike a one-tap path.
#[component]
fn CopyButton(#[prop(into)] text: Signal<String>, label: &'static str) -> impl IntoView {
    let (status, set_status) = create_signal::<&'static str>("");
    let click = move |_| {
        let blob = text.get();
        if blob.is_empty() {
            set_status.set("(nothing to copy)");
            return;
        }
        let win = match web_sys::window() {
            Some(w) => w,
            None => return,
        };
        let clipboard = win.navigator().clipboard();
        let promise = clipboard.write_text(&blob);
        spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(promise).await {
                Ok(_) => set_status.set("copied ✓"),
                Err(_) => set_status.set("copy failed (long-press to select)"),
            }
        });
    };
    view! {
        <span style="margin-left:0.5rem">
            <button on:click=click>{label}</button>
            <span style="margin-left:0.5rem;font-size:12px;color:#393">
                {move || status.get()}
            </span>
        </span>
    }
}
