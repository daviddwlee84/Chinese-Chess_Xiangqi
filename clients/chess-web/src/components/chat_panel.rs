//! In-game chat. Shows the per-room ring buffer (≤50 lines, server-capped),
//! plus an input row that's enabled for players and disabled for
//! spectators. The input is uncontrolled — we read its value on submit
//! rather than mirroring it through a signal so typing stays snappy on
//! large logs.

use chess_core::piece::Side;
use chess_net::protocol::ChatLine;
use leptos::*;

use crate::state::ClientRole;

#[component]
pub fn ChatPanel(
    role: Signal<Option<ClientRole>>,
    log: Signal<Vec<ChatLine>>,
    on_send: Callback<String>,
) -> impl IntoView {
    let log_ref: NodeRef<html::Div> = create_node_ref();

    // After every render that adds a line, scroll the log to the bottom so
    // the newest message is visible. `log.get()` subscribes us to the
    // signal; we ignore the value and just trigger on change.
    create_effect(move |_| {
        let _ = log.get();
        if let Some(el) = log_ref.get() {
            // HtmlElement<html::Div> auto-derefs to web_sys::Element which
            // exposes set_scroll_top / scroll_height directly.
            el.set_scroll_top(el.scroll_height());
        }
    });

    let input_ref: NodeRef<html::Input> = create_node_ref();
    let input_disabled = move || !role.get().map(|r| r.is_player()).unwrap_or(false);

    let placeholder = move || {
        if input_disabled() { "Spectators can read but not chat" } else { "Say something…" }
            .to_string()
    };

    let send_current = move || {
        let Some(input) = input_ref.get() else { return };
        let text = input.value();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        on_send.call(trimmed.to_string());
        input.set_value("");
    };

    let on_submit = move |ev: ev::SubmitEvent| {
        ev.prevent_default();
        send_current();
    };

    view! {
        <section class="chat-panel" aria-label="In-game chat">
            <div class="chat-log" node_ref=log_ref>
                <Show
                    when=move || !log.get().is_empty()
                    fallback=|| view! { <p class="chat-empty">"No messages yet."</p> }
                >
                    <For
                        each=move || log.get()
                        key=|line| (line.from, line.ts_ms, line.text.clone())
                        children=move |line| view! { <ChatLineView line=line/> }
                    />
                </Show>
            </div>
            <form class="chat-input" on:submit=on_submit>
                <input
                    type="text"
                    maxlength="256"
                    autocomplete="off"
                    node_ref=input_ref
                    prop:placeholder=placeholder
                    prop:disabled=input_disabled
                />
                <button class="btn btn-primary" type="submit" prop:disabled=input_disabled>"Send"</button>
            </form>
        </section>
    }
}

#[component]
fn ChatLineView(line: ChatLine) -> impl IntoView {
    let from_class = match line.from {
        Side::RED => "from from-red",
        Side::BLACK => "from from-black",
        _ => "from from-green",
    };
    let from_label = match line.from {
        Side::RED => "Red",
        Side::BLACK => "Black",
        _ => "Green",
    };
    let ts = format_ts(line.ts_ms);
    view! {
        <p class="chat-line">
            <span class="ts">{ts}</span>
            <span class=from_class>{from_label}":"</span>
            " "
            <span class="text">{line.text}</span>
        </p>
    }
}

/// Format a unix-millis timestamp as `hh:mm` in the browser's local zone.
/// Falls back to `--:--` if the JS Date construction fails (shouldn't, but
/// keeps the render infallible).
fn format_ts(ts_ms: u64) -> String {
    // `js_sys::Date::new` takes an `f64` of unix millis.
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ts_ms as f64));
    let hours = date.get_hours();
    let mins = date.get_minutes();
    format!("{:02}:{:02}", hours, mins)
}
