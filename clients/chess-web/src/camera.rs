//! Camera capability detection for the LAN-pairing QR scanner
//! (Phase 5.5).
//!
//! Three signals the page-level scanner UI cares about:
//!
//! 1. **`has_camera()`** — does the device expose at least one
//!    video-input device? `false` for desktops with no webcam,
//!    laptops with the camera disabled in privacy settings, or
//!    headless browsers. The scanner button stays hidden when this
//!    is `false` so users without a camera don't see a dead
//!    affordance — the textarea-paste path remains the parallel
//!    input mode (per `backlog/webrtc-lan-pairing-qr.md` design).
//!
//! 2. **`camera_permission()`** — current Permissions API state for
//!    `'camera'`. Returns `Granted` / `Denied` / `Prompt` / `Unknown`.
//!    `Unknown` covers browsers that don't implement
//!    `navigator.permissions.query({name:'camera'})` (notably some
//!    older Safari versions). The scanner UI shows a "Camera blocked
//!    — enable in Settings" hint when `Denied`.
//!
//! 3. **`open_camera_stream(constraints)`** — actually request the
//!    camera stream. Wraps `navigator.mediaDevices.getUserMedia`.
//!    First call triggers the browser permission prompt on iOS
//!    Safari + Android Chrome.
//!
//! ## Privacy / first-call ordering
//!
//! `has_camera()` calls `enumerateDevices()` which on most browsers
//! returns video-input devices WITHOUT requiring camera permission
//! (Chrome/Firefox), but on Safari without permission the devices
//! show up with empty `deviceId` / `label`. We treat any
//! video-input as a "camera exists" signal — labels aren't needed
//! for the scanner (we don't pick which camera, we just use
//! `facingMode: environment` and let the browser choose).
//!
//! ## Dead-code allowance
//!
//! `camera_permission()` and `CameraPermission` are consumed by
//! commit 4's modal UI for the Denied-state hint. Allow until then.
#![allow(dead_code)]

use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{MediaDeviceInfo, MediaDeviceKind, MediaStream, MediaStreamConstraints};

/// Browser-reported camera permission state.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CameraPermission {
    /// User has granted camera access (this origin, this session).
    Granted,
    /// User has denied. Scanner should show "enable in Settings"
    /// hint instead of trying to prompt again.
    Denied,
    /// Permission has not been asked yet. The first
    /// `getUserMedia` call will trigger the OS prompt.
    Prompt,
    /// Browser doesn't implement `navigator.permissions.query` for
    /// `camera`, OR the query failed for some other reason. Treat
    /// like `Prompt` — try the call and let the browser handle it.
    Unknown,
}

/// Returns `true` if the device exposes at least one video-input
/// device.
///
/// On browsers that gate `enumerateDevices()` results behind camera
/// permission (some Safari builds), this may return `false` until
/// the user grants permission once. Caller's UI should call this
/// on mount and re-check after a successful `open_camera_stream`.
pub async fn has_camera() -> bool {
    let Some(win) = web_sys::window() else {
        return false;
    };
    let nav = win.navigator();
    let Ok(media_devices) = nav.media_devices() else {
        return false;
    };
    let Ok(promise) = media_devices.enumerate_devices() else {
        return false;
    };
    let Ok(devs_js) = JsFuture::from(promise).await else {
        return false;
    };
    let arr: Array = devs_js.into();
    arr.iter().any(|dev_js| {
        dev_js
            .dyn_into::<MediaDeviceInfo>()
            .map(|info| info.kind() == MediaDeviceKind::Videoinput)
            .unwrap_or(false)
    })
}

/// Query `navigator.permissions.query({name: 'camera'})` and return
/// a typed enum.
pub async fn camera_permission() -> CameraPermission {
    let Some(win) = web_sys::window() else {
        return CameraPermission::Unknown;
    };
    let nav = win.navigator();
    let Ok(perms) = Reflect::get(&nav, &JsValue::from_str("permissions")) else {
        return CameraPermission::Unknown;
    };
    if perms.is_undefined() || perms.is_null() {
        return CameraPermission::Unknown;
    }
    let descriptor = Object::new();
    if Reflect::set(&descriptor, &JsValue::from_str("name"), &JsValue::from_str("camera")).is_err()
    {
        return CameraPermission::Unknown;
    }
    let query = match Reflect::get(&perms, &JsValue::from_str("query")) {
        Ok(q) if q.is_function() => q,
        _ => return CameraPermission::Unknown,
    };
    let query_fn: js_sys::Function = query.unchecked_into();
    let Ok(promise_js) = query_fn.call1(&perms, &descriptor) else {
        return CameraPermission::Unknown;
    };
    let promise: js_sys::Promise = promise_js.unchecked_into();
    let Ok(status) = JsFuture::from(promise).await else {
        return CameraPermission::Unknown;
    };
    let Ok(state) = Reflect::get(&status, &JsValue::from_str("state")) else {
        return CameraPermission::Unknown;
    };
    match state.as_string().as_deref() {
        Some("granted") => CameraPermission::Granted,
        Some("denied") => CameraPermission::Denied,
        Some("prompt") => CameraPermission::Prompt,
        _ => CameraPermission::Unknown,
    }
}

/// Open a camera `MediaStream` via `getUserMedia`. Requests the
/// back camera on phones (`facingMode: 'environment'`); browser
/// falls back to the user-facing camera if `environment` is
/// unavailable (laptops, desktops with one camera).
///
/// Returns the stream on success. Caller should attach it to a
/// `<video>` via `set_src_object(Some(&stream))`.
///
/// First call triggers the OS camera permission prompt. If the
/// user denies, returns Err and the caller should switch to the
/// "Camera blocked" UI.
pub async fn open_camera_stream() -> Result<MediaStream, JsValue> {
    let win = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let media_devices = win.navigator().media_devices()?;

    let constraints = MediaStreamConstraints::new();
    let video_constraint = Object::new();
    Reflect::set(
        &video_constraint,
        &JsValue::from_str("facingMode"),
        &JsValue::from_str("environment"),
    )
    .ok();
    constraints.set_video(&video_constraint);

    let promise = media_devices.get_user_media_with_constraints(&constraints)?;
    let stream_js = JsFuture::from(promise).await?;
    Ok(stream_js.dyn_into::<MediaStream>()?)
}
