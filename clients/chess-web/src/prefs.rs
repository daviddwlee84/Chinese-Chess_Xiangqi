//! User-facing FX preferences, persisted in `localStorage`.
//!
//! Two switches, both default ON:
//! * `fx_confetti` — show end-of-game confetti + VICTORY/DEFEAT/DRAW overlay.
//! * `fx_check_banner` — show the "將軍 / CHECK" badge in the sidebar when
//!   the side-to-move is in check (xiangqi only).
//!
//! Storage keys are stable (`chess.fx.confetti`, `chess.fx.checkBanner`)
//! so a user toggling once carries the choice across sessions. Missing
//! values read as `true`. We persist `"1"` / `"0"` rather than booleans
//! so future debugging via DevTools is obvious.
//!
//! All `web_sys` calls are wrapped in `Option`/`Result` chains so a
//! sandboxed context with no `window` (or storage disabled) silently
//! falls back to in-memory defaults — no panics.
//!
//! This module is wasm32-only; the workspace native check skips it.

use leptos::*;

const KEY_CONFETTI: &str = "chess.fx.confetti";
const KEY_CHECK_BANNER: &str = "chess.fx.checkBanner";

#[derive(Copy, Clone)]
pub struct Prefs {
    pub fx_confetti: RwSignal<bool>,
    pub fx_check_banner: RwSignal<bool>,
}

impl Prefs {
    /// Hydrate from `localStorage` and arm `create_effect` watchers that
    /// persist any future change. Designed to be called once at app
    /// boot and `provide_context`-shared with all routes.
    pub fn hydrate() -> Self {
        let fx_confetti = create_rw_signal(read_bool(KEY_CONFETTI, true));
        let fx_check_banner = create_rw_signal(read_bool(KEY_CHECK_BANNER, true));

        // Persist on change. The closures only run inside the browser, so
        // calling `localStorage` on every flip is safe and cheap.
        create_effect(move |_| {
            let v = fx_confetti.get();
            write_bool(KEY_CONFETTI, v);
        });
        create_effect(move |_| {
            let v = fx_check_banner.get();
            write_bool(KEY_CHECK_BANNER, v);
        });

        Self { fx_confetti, fx_check_banner }
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

fn read_bool(key: &str, default_value: bool) -> bool {
    let Some(storage) = local_storage() else { return default_value };
    match storage.get_item(key) {
        Ok(Some(s)) => s != "0",
        _ => default_value,
    }
}

fn write_bool(key: &str, value: bool) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(key, if value { "1" } else { "0" });
    }
}
