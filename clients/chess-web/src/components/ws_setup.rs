use leptos::*;

use crate::config::{
    append_query_pairs, read_stored_ws_base, write_stored_ws_base, ws_query_pair, WsBase,
};
use crate::routes::{app_href, normalize_ws_base, WsBaseError};

#[component]
pub fn WsSetup(
    title: &'static str,
    next_path: String,
    #[prop(optional)] detail: Option<&'static str>,
    #[prop(default = "")] initial_error: &'static str,
) -> impl IntoView {
    let stored = read_stored_ws_base().unwrap_or_else(|| "wss://".to_string());
    let input = create_rw_signal(stored);
    let error = create_rw_signal(if initial_error.is_empty() {
        None
    } else {
        Some(initial_error.to_string())
    });

    let connect: Callback<()> = {
        let next_path = next_path.clone();
        Callback::new(move |_| {
            let raw = input.get_untracked();
            match normalize_ws_base(&raw) {
                Ok(base) => {
                    write_stored_ws_base(&base);
                    let ws = WsBase { base, persist_in_urls: true };
                    let target = append_query_pairs(&next_path, ws_query_pair(&ws));
                    if let Some(win) = web_sys::window() {
                        let _ = win.location().set_href(&app_href(&target));
                    }
                }
                Err(WsBaseError::Empty) => error.set(Some("Enter a websocket server URL.".into())),
                Err(WsBaseError::BadScheme) => {
                    error.set(Some("Use a full ws:// or wss:// server URL.".into()))
                }
            }
        })
    };

    view! {
        <section class="lobby">
            <a href=app_href("/") rel="external" class="back-link">"← Back to picker"</a>
            <h2>{title}</h2>
            <p class="subtitle">
                {detail.unwrap_or("This static build needs a chess-net websocket server. GitHub Pages uses HTTPS, so remote play should use wss://.")}
            </p>
            <div class="lobby-form">
                <input
                    class="text-input"
                    type="text"
                    placeholder="wss://your-server.example"
                    prop:value=move || input.get()
                    on:input=move |ev| {
                        error.set(None);
                        input.set(event_target_value(&ev));
                    }
                    on:keydown=move |ev| {
                        if ev.key() == "Enter" {
                            connect.call(());
                        }
                    }
                />
                <button class="btn btn-primary" on:click=move |_| connect.call(())>"Connect"</button>
            </div>
            <Show when=move || error.get().is_some()>
                <p class="conn-banner error">{move || error.get().unwrap_or_default()}</p>
            </Show>
        </section>
    }
}
