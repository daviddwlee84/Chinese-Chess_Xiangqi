//! "Are you sure you want to leave?" tab-close protection.
//!
//! Used by `pages/lan.rs` (Phase 6) to guard the host's `Playing`
//! state — accidentally closing the host tab today silently kills
//! the LAN room (no reconnect, all peers immediately lose the
//! game).
//!
//! ## Browser-quirk gotcha
//!
//! Modern browsers IGNORE the message string you set. They show a
//! generic "Changes you made may not be saved" prompt regardless.
//! All you need to do is set `event.returnValue` to a non-empty
//! string AND call `event.preventDefault()`. Some older browsers
//! also need the listener to *return* the string from the handler,
//! but Chrome/Firefox/Safari today only check returnValue.
//!
//! ## Reactivity contract
//!
//! Caller toggles a `bool` signal (e.g. `status == Playing`) via
//! [`use_beforeunload_guard`]; the guard installs / removes the
//! listener as the signal flips. The guard itself takes care of
//! cleanup on component unmount via `on_cleanup`, so the listener
//! never outlives the page that mounted it (cheap to install
//! again for the next room).

use std::cell::RefCell;
use std::rc::Rc;

use leptos::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::EventTarget;

/// Install a `beforeunload` listener whenever `active.get() == true`,
/// remove it when `active.get() == false` or the calling component
/// unmounts.
///
/// `message` is the legacy-API string — modern browsers ignore the
/// content but still need a non-empty string to trigger the prompt.
pub fn use_beforeunload_guard(active: Signal<bool>, message: &'static str) {
    // `Rc<RefCell<Option<Closure>>>` because we both:
    //   1. Need to read it from the effect to call `add_event_listener`
    //      / `remove_event_listener`.
    //   2. Need to drop it on unmount to release the JS callback.
    let listener: Rc<RefCell<Option<Closure<dyn FnMut(JsValue)>>>> = Rc::new(RefCell::new(None));

    {
        let listener = listener.clone();
        create_effect(move |_| {
            let want_active = active.get();
            let Some(win) = web_sys::window() else {
                return;
            };
            let target: &EventTarget = win.as_ref();

            if want_active {
                if listener.borrow().is_some() {
                    // Already installed.
                    return;
                }
                let cb = Closure::wrap(Box::new(move |ev: JsValue| {
                    // Cast to BeforeUnloadEvent so we can set
                    // returnValue (the cross-browser trigger). Safari
                    // also wants `preventDefault`; modern Chromium
                    // honours both.
                    if let Ok(buev) = ev.dyn_into::<web_sys::BeforeUnloadEvent>() {
                        buev.prevent_default();
                        buev.set_return_value(message);
                    }
                }) as Box<dyn FnMut(JsValue)>);
                target
                    .add_event_listener_with_callback("beforeunload", cb.as_ref().unchecked_ref())
                    .ok();
                *listener.borrow_mut() = Some(cb);
            } else if let Some(cb) = listener.borrow_mut().take() {
                target
                    .remove_event_listener_with_callback(
                        "beforeunload",
                        cb.as_ref().unchecked_ref(),
                    )
                    .ok();
                // Drop the closure when it goes out of scope.
                drop(cb);
            }
        });
    }

    // Belt-and-suspenders cleanup: even if the effect didn't run a
    // final remove pass, drop the listener on unmount.
    on_cleanup(move || {
        if let Some(cb) = listener.borrow_mut().take() {
            if let Some(win) = web_sys::window() {
                let target: &EventTarget = win.as_ref();
                target
                    .remove_event_listener_with_callback(
                        "beforeunload",
                        cb.as_ref().unchecked_ref(),
                    )
                    .ok();
            }
            drop(cb);
        }
    });
}
