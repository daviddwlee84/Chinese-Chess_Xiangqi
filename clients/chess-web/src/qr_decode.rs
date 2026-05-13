//! Rust interop wrapper for [`jsQR`](https://github.com/cozmo/jsQR).
//!
//! `jsQR` is bundled at `dist/jsQR.min.js` (~130 KB minified,
//! ~38 KB gzipped, copied by Trunk from `public/jsQR.min.js`). It's
//! loaded LAZILY via [`ensure_loaded`] the first time the LAN camera
//! scanner mounts — eager loading from `index.html` would (a) waste
//! bandwidth for users who never use LAN pairing and (b) trip on
//! the SPA relative-URL issue (a `<script src="jsQR.min.js">` from
//! `/lan/host` would resolve to `/lan/jsQR.min.js`, which is the
//! SPA fallback HTML, not the JS file).
//!
//! The lazy loader uses an absolute URL built from
//! `window.location.origin` + `/jsQR.min.js`, which works on:
//! - Trunk dev server (asset at root).
//! - Production (chess-net `--static-dir` serving dist at root).
//! - GitHub Pages with `--public-url /Chinese-Chess_Xiangqi/` (a
//!   `<base href="...">` tag would also work, but lazy-load avoids
//!   the leptos_router-vs-base-href conflict).
//!
//! ## Why jsQR (not pure-Rust `rqrr`)
//!
//! See `backlog/webrtc-lan-pairing-qr.md` for the full comparison.
//! Short version: `jsQR` is the industry-standard pure-JS QR
//! decoder, well-tested across millions of web QR scanners. `rqrr`
//! would be ~30 KB additional wasm but adds the canvas-frame-capture
//! loop in Rust (more new code, more new bugs). Easy to swap to
//! `rqrr` later if `jsQR`'s decode latency shows up as a bottleneck.
//!
//! ## API shape
//!
//! `jsQR(data, width, height)`:
//!   * `data`: `Uint8ClampedArray` of RGBA pixels (same layout as
//!     `CanvasRenderingContext2D.getImageData().data`).
//!   * `width` / `height` in pixels.
//!   * Returns `null` if no QR found, else `{ data: string, ... }`.
//!
//! ## Dead-code allowance
//!
//! `is_available` is consumed by the page-level scanner (commit 4)
//! for the "scanner unavailable" fallback. Allow until that wires up.
#![allow(dead_code)]

use js_sys::{Reflect, Uint8ClampedArray};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{HtmlScriptElement, Window};

#[wasm_bindgen]
extern "C" {
    /// Global `jsQR(data, width, height)` symbol from `jsQR.min.js`.
    /// Wrapped in `catch` because calling an undefined symbol throws.
    #[wasm_bindgen(js_name = jsQR, catch)]
    fn js_qr_call(data: &Uint8ClampedArray, width: u32, height: u32) -> Result<JsValue, JsValue>;
}

/// Try to decode a QR code from a raw RGBA pixel buffer.
///
/// Returns:
/// * `Some(text)` — `data` field of jsQR's result object.
/// * `None` — no QR found in this frame, OR `jsQR` isn't loaded
///   (caller should `ensure_loaded()` first).
///
/// Safe to call from a `requestAnimationFrame` loop — `jsQR`
/// itself takes ~30 ms per frame on a phone. Caller may rate-
/// limit if needed (e.g. skip every other frame on slow hardware).
pub fn decode_rgba(data: &Uint8ClampedArray, width: u32, height: u32) -> Option<String> {
    let result = js_qr_call(data, width, height).ok()?;
    if result.is_null() || result.is_undefined() {
        return None;
    }
    let data_field = Reflect::get(&result, &JsValue::from_str("data")).ok()?;
    data_field.as_string()
}

/// `true` if the global `jsQR` symbol is reachable.
pub fn is_available() -> bool {
    let global = js_sys::global();
    Reflect::get(&global, &JsValue::from_str("jsQR")).ok().map(|v| v.is_function()).unwrap_or(false)
}

/// Lazy-load `jsQR.min.js` if it's not already loaded.
///
/// Idempotent: if `is_available() == true` on entry, returns
/// immediately. Otherwise injects a `<script>` tag with an absolute
/// URL (`window.location.origin + "/jsQR.min.js"`) into `<head>`
/// and awaits its `load` event.
///
/// Returns `Err` on:
/// * No `window` (not in a browser — shouldn't happen for chess-web).
/// * Script `error` event (network failure / 404 / CSP block).
/// * `jsQR` symbol still missing after the script reports loaded
///   (would indicate a corrupted bundle — give up).
///
/// 10 s implicit timeout via `script.onerror` — if the script never
/// fires `load` or `error`, the promise hangs and the caller's
/// camera modal will eventually error out the user-visible "couldn't
/// read QR" message. Acceptable for a one-shot pairing flow.
pub async fn ensure_loaded() -> Result<(), JsValue> {
    if is_available() {
        return Ok(());
    }
    let win: Window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let doc = win.document().ok_or_else(|| JsValue::from_str("no document"))?;
    let head = doc.head().ok_or_else(|| JsValue::from_str("no <head>"))?;

    // Build absolute URL so the relative-URL-on-SPA-route trap
    // (`/lan/host` resolving `jsQR.min.js` to `/lan/jsQR.min.js` →
    // SPA fallback HTML) is sidestepped.
    let origin = win.location().origin().map_err(|_| JsValue::from_str("no origin"))?;
    let url = format!("{origin}/jsQR.min.js");

    let script: HtmlScriptElement =
        doc.create_element("script")?.dyn_into::<HtmlScriptElement>()?;
    script.set_src(&url);
    script.set_async(false);

    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let resolve_for_load = resolve.clone();
        let load_cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            let _ = resolve_for_load.call0(&JsValue::NULL);
        }) as Box<dyn FnMut(JsValue)>);
        script.add_event_listener_with_callback("load", load_cb.as_ref().unchecked_ref()).ok();
        load_cb.forget();

        let reject_for_err = reject.clone();
        let err_cb = Closure::wrap(Box::new(move |_ev: JsValue| {
            let _ =
                reject_for_err.call1(&JsValue::NULL, &JsValue::from_str("jsQR script load error"));
        }) as Box<dyn FnMut(JsValue)>);
        script.add_event_listener_with_callback("error", err_cb.as_ref().unchecked_ref()).ok();
        err_cb.forget();
    });

    head.append_child(&script)?;
    JsFuture::from(promise).await?;

    if !is_available() {
        return Err(JsValue::from_str("jsQR script loaded but global symbol not registered"));
    }
    Ok(())
}
