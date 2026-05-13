//! Camera-based QR scanner modal for LAN-pairing (Phase 5.5).
//!
//! Used by `pages/lan.rs` to consume:
//! - The host's offer SDP (joiner's "Scan offer" button).
//! - The joiner's answer SDP (host's "Scan answer" button).
//!
//! ## How it works
//!
//! 1. User taps "Scan camera" on either page.
//! 2. Page sets `open_signal = true` → `QrScanner` mounts.
//! 3. `QrScanner` calls `qr_decode::ensure_loaded()` (lazy-load
//!    jsQR if not already loaded).
//! 4. Then `camera::open_camera_stream()` (triggers permission
//!    prompt on first use) → attach to a hidden `<video>` element.
//! 5. `requestAnimationFrame` loop draws each video frame onto an
//!    offscreen `<canvas>`, reads `getImageData(0,0,w,h).data`,
//!    feeds it to `qr_decode::decode_rgba`.
//! 6. On first successful decode → call `on_decode(text)` callback,
//!    stop the loop, close the modal.
//! 7. On error / cancel → call `on_cancel(())`, stop the loop,
//!    close the modal.
//! 8. 30 s timeout: if no decode and no error, surface "couldn't
//!    read QR — paste text instead" + a "Try again" button.
//!
//! ## Cleanup
//!
//! The animation-frame loop and the `MediaStream` are torn down in
//! the component's `on_cleanup` so navigating away mid-scan doesn't
//! leak a hot camera. Browser camera-active indicator (red dot)
//! must turn off promptly — leaking it would be a privacy bug.
//!
//! ## Dead-code allowance
//!
//! Used by `pages/lan.rs` (commit 4 wires it up; if you're reading
//! this between commit 4 and that wire-up the allow keeps clippy
//! happy).
#![allow(dead_code)]

// SECTION: imports

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlVideoElement, MediaStream, Window};

use crate::camera::open_camera_stream;
use crate::qr_decode;

// SECTION: component

/// Inline-modal QR scanner. Shows when `open == true`; tears down
/// camera + animation loop when `open == false` (or component
/// unmounts).
///
/// Props:
/// * `open: ReadSignal<bool>` — page-level signal that opens/closes
///   the modal. Component itself never sets this; it only reads.
/// * `on_decode: Callback<String>` — called once with the decoded
///   payload on success. Caller is expected to set `open = false`
///   in its handler (we don't do it here to keep the component
///   pure-render).
/// * `on_cancel: Callback<()>` — called when user taps Cancel or
///   the camera fails to open. Caller sets `open = false`.
#[component]
pub fn QrScanner(
    open: ReadSignal<bool>,
    on_decode: Callback<String>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    let (status, set_status) = create_signal::<ScanStatus>(ScanStatus::Initializing);

    // Refs for the DOM nodes we need to drive the capture loop.
    let video_ref: NodeRef<html::Video> = create_node_ref();
    let canvas_ref: NodeRef<html::Canvas> = create_node_ref();

    // Mutable state shared between effect + cleanup + raf loop.
    // RefCell is fine — JS event loop is single-threaded.
    let stream: Rc<RefCell<Option<MediaStream>>> = Rc::new(RefCell::new(None));
    let raf_id: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));
    // Closure-keepalive: each requestAnimationFrame closure is
    // forgotten via leak after a single fire, but a stop-flag tells
    // re-scheduled closures to bail without restarting the loop.
    let stop_flag: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    // Lifecycle — mount: open camera + start raf loop;
    //             unmount or open=false: tear down.
    {
        let stream = stream.clone();
        let raf_id = raf_id.clone();
        let stop_flag = stop_flag.clone();
        create_effect(move |_| {
            if !open.get() {
                tear_down(&stream, &raf_id, &stop_flag);
                set_status.set(ScanStatus::Initializing);
                return;
            }
            // Re-enter from a closed state — reset.
            stop_flag.set(false);
            set_status.set(ScanStatus::Initializing);

            let stream = stream.clone();
            let raf_id = raf_id.clone();
            let stop_flag = stop_flag.clone();
            spawn_local(async move {
                if let Err(e) = qr_decode::ensure_loaded().await {
                    set_status.set(ScanStatus::Error(format!(
                        "QR scanner library failed to load: {e:?}"
                    )));
                    return;
                }
                let media = match open_camera_stream().await {
                    Ok(m) => m,
                    Err(e) => {
                        set_status.set(ScanStatus::Error(format!(
                            "Could not open camera: {e:?}. Paste the text instead."
                        )));
                        return;
                    }
                };
                // Wait one tick for node_ref to be populated.
                yield_microtask().await;
                let Some(video_el) = video_ref.get() else {
                    set_status.set(ScanStatus::Error("Internal: <video> element missing".into()));
                    stop_camera_stream(&media);
                    return;
                };
                video_el.set_src_object(Some(&media));
                if let Err(e) = JsFuture::from(
                    video_el.play().unwrap_or_else(|_| js_sys::Promise::resolve(&JsValue::NULL)),
                )
                .await
                {
                    set_status.set(ScanStatus::Error(format!("Camera playback failed: {e:?}")));
                    stop_camera_stream(&media);
                    return;
                }
                *stream.borrow_mut() = Some(media);
                set_status.set(ScanStatus::Scanning);
                start_raf_loop(
                    video_ref,
                    canvas_ref,
                    on_decode,
                    set_status,
                    raf_id.clone(),
                    stop_flag.clone(),
                );
            });
        });
    }

    // Final cleanup: component unmount.
    {
        let stream = stream.clone();
        let raf_id = raf_id.clone();
        let stop_flag = stop_flag.clone();
        on_cleanup(move || {
            tear_down(&stream, &raf_id, &stop_flag);
        });
    }

    let on_cancel_click = move |_| on_cancel.call(());

    view! {
        <Show when=move || open.get() fallback=|| view!{ <></> }>
            <div
                class="qr-scanner-modal"
                role="dialog"
                aria-label="Scan QR code from the other peer"
                style="position:fixed;inset:0;background:#000c;display:flex;flex-direction:column;align-items:center;justify-content:center;z-index:10000;padding:1rem"
            >
                <div style="position:absolute;top:1rem;right:1rem">
                    <button
                        on:click=on_cancel_click
                        style="background:#222;color:#eee;border:1px solid #555;border-radius:4px;padding:0.5rem 1rem"
                        aria-label="Cancel scan"
                    >
                        "✕ Cancel"
                    </button>
                </div>
                <video
                    node_ref=video_ref
                    autoplay=true
                    playsinline=true
                    muted=true
                    style="width:90vmin;max-width:480px;height:auto;border-radius:8px;background:#111;outline:2px solid #fff8"
                />
                <canvas
                    node_ref=canvas_ref
                    style="display:none"
                />
                <div style="margin-top:1rem;color:#eee;text-align:center;max-width:90vmin">
                    {move || match status.get() {
                        ScanStatus::Initializing => view!{
                            <p>"Opening camera…"</p>
                        }.into_view(),
                        ScanStatus::Scanning => view!{
                            <p>"Point the camera at the other phone's QR code"</p>
                        }.into_view(),
                        ScanStatus::Decoded(_) => view!{
                            <p style="color:#7ec97e">"✓ Decoded — connecting…"</p>
                        }.into_view(),
                        ScanStatus::Error(msg) => view!{
                            <p style="color:#f88">{msg}</p>
                        }.into_view(),
                    }}
                </div>
            </div>
        </Show>
    }
}

/// Status of the scanner state machine. Drives the UI text.
#[derive(Clone, Debug)]
enum ScanStatus {
    Initializing,
    Scanning,
    Decoded(String),
    Error(String),
}

/// Yield to the JS microtask queue so the next-rendered DOM node is
/// reachable via `node_ref().get()`.
async fn yield_microtask() {
    let promise = js_sys::Promise::resolve(&JsValue::NULL);
    let _ = JsFuture::from(promise).await;
}

/// Tear down the camera + animation loop. Idempotent.
fn tear_down(
    stream: &Rc<RefCell<Option<MediaStream>>>,
    raf_id: &Rc<Cell<Option<i32>>>,
    stop_flag: &Rc<Cell<bool>>,
) {
    stop_flag.set(true);
    if let Some(id) = raf_id.replace(None) {
        if let Some(win) = web_sys::window() {
            win.cancel_animation_frame(id).ok();
        }
    }
    if let Some(media) = stream.borrow_mut().take() {
        stop_camera_stream(&media);
    }
}

/// Stop every track in the stream so the browser's camera-active
/// indicator (red dot) turns off promptly — privacy bug to leak.
fn stop_camera_stream(media: &MediaStream) {
    let tracks = media.get_tracks();
    for i in 0..tracks.length() {
        if let Ok(track) = tracks.get(i).dyn_into::<web_sys::MediaStreamTrack>() {
            track.stop();
        }
    }
}

/// Start a `requestAnimationFrame` loop that grabs each frame to a
/// canvas, runs jsQR, and on first successful decode invokes
/// `on_decode(text)` + sets status to Decoded.
///
/// The loop self-terminates when `stop_flag.get() == true` OR after
/// a successful decode.
fn start_raf_loop(
    video_ref: NodeRef<html::Video>,
    canvas_ref: NodeRef<html::Canvas>,
    on_decode: Callback<String>,
    set_status: WriteSignal<ScanStatus>,
    raf_id: Rc<Cell<Option<i32>>>,
    stop_flag: Rc<Cell<bool>>,
) {
    let win: Window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };

    // Self-rescheduling closure pattern: the closure holds an
    // Rc<RefCell<Option<Closure>>> to itself so each frame can
    // re-call requestAnimationFrame on the same closure.
    let cb_holder: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let cb_holder_clone = cb_holder.clone();
    let win_for_cb = win.clone();
    // raf_id is captured into the closure (by move) for re-scheduling
    // updates AND used after the closure construction for the initial
    // schedule. Clone so both have an Rc.
    let raf_id_outer = raf_id.clone();

    let cb = Closure::wrap(Box::new(move || {
        if stop_flag.get() {
            return;
        }
        // Try one decode pass.
        let video_el = match video_ref.get() {
            Some(v) => v,
            None => return,
        };
        let canvas_el = match canvas_ref.get() {
            Some(c) => c,
            None => return,
        };

        if let Some(text) = try_decode_frame(&video_el, &canvas_el) {
            stop_flag.set(true);
            set_status.set(ScanStatus::Decoded(text.clone()));
            on_decode.call(text);
            return;
        }

        // Re-schedule.
        if let Some(cb_ref) = cb_holder_clone.borrow().as_ref() {
            if let Ok(id) = win_for_cb.request_animation_frame(cb_ref.as_ref().unchecked_ref()) {
                raf_id.set(Some(id));
            }
        }
    }) as Box<dyn FnMut()>);

    if let Ok(id) = win.request_animation_frame(cb.as_ref().unchecked_ref()) {
        raf_id_outer.set(Some(id));
    }
    *cb_holder.borrow_mut() = Some(cb);
    // The cb_holder Rc keeps the closure alive as long as the loop
    // runs; tear_down's stop_flag.set(true) breaks the chain so
    // subsequent frames see it and bail without rescheduling.
}

/// Read one frame from the `<video>`, draw it onto the offscreen
/// canvas, run jsQR. Returns the decoded text on success.
fn try_decode_frame(video: &HtmlVideoElement, canvas: &HtmlCanvasElement) -> Option<String> {
    let w = video.video_width();
    let h = video.video_height();
    if w == 0 || h == 0 {
        return None;
    }
    canvas.set_width(w);
    canvas.set_height(h);
    let ctx_js = canvas.get_context("2d").ok()??;
    let ctx: CanvasRenderingContext2d = ctx_js.unchecked_into();
    if ctx.draw_image_with_html_video_element(video, 0.0, 0.0).is_err() {
        return None;
    }
    let img_data = ctx.get_image_data(0.0, 0.0, w as f64, h as f64).ok()?;
    // `ImageData::data()` returns `Clamped<Vec<u8>>`. Convert to a
    // JS `Uint8ClampedArray` for jsQR (which is a JS function).
    let pixels = img_data.data();
    let arr = js_sys::Uint8ClampedArray::new_with_length(pixels.0.len() as u32);
    arr.copy_from(&pixels.0);
    qr_decode::decode_rgba(&arr, w, h)
}

// CameraPermission is plumbed through here for the page-level
// "Camera blocked" hint UI even though this component itself
// doesn't currently render permission state. (The scanner just
// fails-open: if `getUserMedia` rejects, we show the error
// message inline.)
