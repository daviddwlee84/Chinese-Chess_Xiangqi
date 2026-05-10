//! PWA UI: install banner / button, update toast, offline indicator.
//!
//! All the platform plumbing (service worker registration, capturing
//! `beforeinstallprompt`, posting `SKIP_WAITING`) lives in
//! `public/pwa.js` and exposes a tiny bridge on `window.__pwa`. This
//! module subscribes to the bridge's CustomEvents and renders Leptos
//! UI on top.
//!
//! Why JS for the bridge? `beforeinstallprompt` fires very early —
//! before the WASM bundle finishes loading — and missing it makes the
//! install button impossible to wire up. Doing the listener in JS at
//! `<script defer>` time guarantees we catch it.
//!
//! The signals returned by `use_pwa_state()` are global to the app —
//! call it once near the root and pass via context.

use leptos::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

#[derive(Copy, Clone)]
pub struct PwaState {
    /// `beforeinstallprompt` was captured and the in-page prompt is
    /// available. False on iOS Safari (which has no such event) and
    /// after the user already accepts / dismisses.
    pub install_available: RwSignal<bool>,
    /// Currently running as a standalone PWA (display-mode standalone
    /// or iOS `navigator.standalone`).
    pub standalone: RwSignal<bool>,
    /// Best-effort iOS detection. Drives the manual "Add to Home
    /// Screen" hint since iOS won't fire the install event.
    pub ios: RwSignal<bool>,
    /// Best-effort mobile detection (any touch device); we suppress
    /// the desktop-only banner copy on these.
    pub mobile: RwSignal<bool>,
    /// New service worker is waiting to take over — show the toast.
    pub update_ready: RwSignal<bool>,
    /// `navigator.onLine` mirror. Updates on `online` / `offline`
    /// window events. `true` is the optimistic default — browsers
    /// only fire `offline` when truly disconnected.
    pub online: RwSignal<bool>,
}

impl PwaState {
    /// Initialise signals from `window.__pwa` and wire CustomEvent
    /// listeners. Idempotent: safe to call once at app boot.
    pub fn hydrate() -> Self {
        let state = PwaState {
            install_available: create_rw_signal(false),
            standalone: create_rw_signal(false),
            ios: create_rw_signal(false),
            mobile: create_rw_signal(false),
            update_ready: create_rw_signal(false),
            online: create_rw_signal(true),
        };

        let Some(win) = web_sys::window() else {
            return state;
        };

        // Pull initial state via the `window.__pwa` bridge. If pwa.js
        // hasn't loaded yet (script defer + WASM start order varies by
        // browser), the field reads return false / true as defaults
        // and the listeners below correct them when events fire.
        if let Ok(pwa) = js_sys::Reflect::get(&win, &JsValue::from_str("__pwa")) {
            state.install_available.set(call_bool(&pwa, "canInstall"));
            state.standalone.set(call_bool(&pwa, "isStandalone"));
            state.ios.set(call_bool(&pwa, "isIos"));
            state.mobile.set(call_bool(&pwa, "isMobile"));
        }

        // Initial online — read navigator.onLine.
        state.online.set(win.navigator().on_line());

        let target: &web_sys::EventTarget = win.as_ref();

        attach(target, "pwa:install-available", {
            let sig = state.install_available;
            move || sig.set(true)
        });
        attach(target, "pwa:installed", {
            let installed = state.install_available;
            let standalone = state.standalone;
            move || {
                installed.set(false);
                standalone.set(true);
            }
        });
        attach(target, "pwa:update-ready", {
            let sig = state.update_ready;
            move || sig.set(true)
        });
        attach(target, "online", {
            let sig = state.online;
            move || sig.set(true)
        });
        attach(target, "offline", {
            let sig = state.online;
            move || sig.set(false)
        });

        state
    }
}

fn call_bool(pwa: &JsValue, method: &str) -> bool {
    let Ok(f) = js_sys::Reflect::get(pwa, &JsValue::from_str(method)) else {
        return false;
    };
    let Ok(func) = f.dyn_into::<js_sys::Function>() else {
        return false;
    };
    func.call0(pwa).ok().and_then(|v| v.as_bool()).unwrap_or(false)
}

fn attach<F>(target: &web_sys::EventTarget, event: &str, f: F)
where
    F: FnMut() + 'static,
{
    let cb = Closure::<dyn FnMut()>::new(f);
    let _ = target.add_event_listener_with_callback(event, cb.as_ref().unchecked_ref());
    // Leak so the closure outlives the call. These are app-lifetime
    // listeners — we never detach them.
    cb.forget();
}

fn call_install() {
    if let Some(win) = web_sys::window() {
        if let Ok(pwa) = js_sys::Reflect::get(&win, &JsValue::from_str("__pwa")) {
            if let Ok(f) = js_sys::Reflect::get(&pwa, &JsValue::from_str("install")) {
                if let Ok(func) = f.dyn_into::<js_sys::Function>() {
                    let _ = func.call0(&pwa);
                }
            }
        }
    }
}

fn call_apply_update() {
    if let Some(win) = web_sys::window() {
        if let Ok(pwa) = js_sys::Reflect::get(&win, &JsValue::from_str("__pwa")) {
            if let Ok(f) = js_sys::Reflect::get(&pwa, &JsValue::from_str("applyUpdate")) {
                if let Ok(func) = f.dyn_into::<js_sys::Function>() {
                    let _ = func.call0(&pwa);
                }
            }
        }
    }
}

// ---- Components ------------------------------------------------------

/// Bottom-corner toast that appears when a new build is precached and
/// waiting. Click to skipWaiting + reload.
#[component]
pub fn PwaUpdateToast() -> impl IntoView {
    let pwa = expect_context::<PwaState>();
    let visible = pwa.update_ready;

    view! {
        <Show when=move || visible.get()>
            <div class="pwa-toast" role="status" aria-live="polite">
                <span class="pwa-toast__msg">"有新版本 — New version available"</span>
                <button
                    class="pwa-toast__btn"
                    on:click=move |_| {
                        call_apply_update();
                    }
                >
                    "重新整理 / Reload"
                </button>
                <button
                    class="pwa-toast__close"
                    aria-label="Dismiss"
                    on:click=move |_| visible.set(false)
                >
                    "✕"
                </button>
            </div>
        </Show>
    }
}

/// Big banner shown above the picker hero. Hidden once installed or
/// after the user dismisses.
#[component]
pub fn PwaInstallBanner() -> impl IntoView {
    let pwa = expect_context::<PwaState>();
    let dismissed = create_rw_signal(false);

    let show_native =
        move || !dismissed.get() && !pwa.standalone.get() && pwa.install_available.get();
    let show_ios_hint = move || {
        !dismissed.get() && !pwa.standalone.get() && pwa.ios.get() && !pwa.install_available.get()
    };

    view! {
        <Show when=show_native>
            <aside class="pwa-banner" role="region" aria-label="Install app">
                <div class="pwa-banner__icon" aria-hidden="true">"帥"</div>
                <div class="pwa-banner__copy">
                    <strong>"安裝到桌面 / Install app"</strong>
                    <p>"安裝後不開瀏覽器也能玩本地對局,連網會自動檢查更新。"</p>
                </div>
                <div class="pwa-banner__actions">
                    <button
                        class="btn btn-primary"
                        on:click=move |_| {
                            call_install();
                            dismissed.set(true);
                        }
                    >"安裝 / Install"</button>
                    <button
                        class="pwa-banner__dismiss"
                        aria-label="Dismiss"
                        on:click=move |_| dismissed.set(true)
                    >"✕"</button>
                </div>
            </aside>
        </Show>
        <Show when=show_ios_hint>
            <aside class="pwa-banner pwa-banner--ios" role="region" aria-label="Install on iOS">
                <div class="pwa-banner__icon" aria-hidden="true">"帥"</div>
                <div class="pwa-banner__copy">
                    <strong>"加到主畫面 / Add to Home Screen"</strong>
                    <p>
                        "在 Safari 點下方分享按鈕 "
                        <span class="pwa-banner__share" aria-hidden="true">"⬆"</span>
                        " → 選『加入主畫面』,即可離線使用本地對局。"
                    </p>
                </div>
                <button
                    class="pwa-banner__dismiss"
                    aria-label="Dismiss"
                    on:click=move |_| dismissed.set(true)
                >"✕"</button>
            </aside>
        </Show>
    }
}

/// Compact button — hides itself unless a native install prompt is
/// available. Designed for the sidebar footer.
#[component]
pub fn PwaInstallButton() -> impl IntoView {
    let pwa = expect_context::<PwaState>();
    let visible = move || !pwa.standalone.get() && pwa.install_available.get();

    view! {
        <Show when=visible>
            <button
                class="btn pwa-install-btn"
                title="Install Chinese Chess as an app on this device"
                on:click=move |_| call_install()
            >
                "📲 安裝到桌面 / Install"
            </button>
        </Show>
    }
}

/// Tiny dot in the corner. Green when online, dim red when offline.
/// Tooltip explains that online play needs a connection.
#[component]
pub fn OfflineIndicator() -> impl IntoView {
    let pwa = expect_context::<PwaState>();
    let online = pwa.online;
    let cls = move || {
        if online.get() {
            "pwa-online-dot pwa-online-dot--on"
        } else {
            "pwa-online-dot pwa-online-dot--off"
        }
    };
    let title = move || {
        if online.get() {
            "Online — multiplayer available"
        } else {
            "Offline — local play still works; lobby/online disabled"
        }
    };

    view! {
        <div class=cls title=title aria-live="polite">
            <span class="pwa-online-dot__pip" aria-hidden="true"></span>
            <span class="pwa-online-dot__label">
                {move || if online.get() { "Online" } else { "Offline" }}
            </span>
        </div>
    }
}
