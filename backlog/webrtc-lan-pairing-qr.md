# WebRTC LAN pairing — Phase 5.5: QR generation + camera scan

## Status

- **Phase 5 (textarea-only pairing) shipped 2026-05-13** — see
  `docs/lan-multiplayer-webrtc.md` for the recipe + walkthrough.
- **Phase 5.5 SHIPPED 2026-05-13** — six commits (C1-C6) landed
  end-to-end. QR + camera path works alongside textarea path; `scripts/
  test-lan-pairing.sh` verifies the textarea path via playwright-cli;
  camera path verified manually on real devices.
- Owner: same as Phase 5.
- Estimated effort: **1.5–2 days** (1 day implementation, 0.5 day
  cross-device testing). **Actual: ~3 hours of focused work** —
  helped by the design doc accurately predicting all six commits
  upfront.

## Design

### Capability detection (camera availability)

Run on page mount, both `/lan/host` and `/lan/join`:

```rust
async fn has_camera() -> bool {
    let Some(nav) = web_sys::window().and_then(|w| Some(w.navigator())) else {
        return false;
    };
    let Ok(devs) = JsFuture::from(nav.media_devices().ok()?.enumerate_devices().ok()?).await
    else {
        return false;
    };
    let arr: js_sys::Array = devs.into();
    arr.iter().any(|d| {
        let info: web_sys::MediaDeviceInfo = d.unchecked_into();
        info.kind() == web_sys::MediaDeviceKind::Videoinput
    })
}
```

Three UI modes:

| Camera state | UI shown |
|---|---|
| `has_camera() == true` (untested permission) | QR display + camera button labelled "Scan camera" + textarea (collapsed) |
| `has_camera() == false` | Textarea only (no QR? — see below) + a tooltip "no camera detected; use copy/paste" |
| Permission denied or scan failed mid-flow | Camera button reverts; textarea stays |

**Sub-decision: do we show the offer/answer QR even when there's no
camera?** Yes — the LOCAL camera doesn't matter for *displaying* a
QR. Even a desktop with no webcam should show the QR so a phone-with-
camera friend can scan it. The textarea is the parallel input mode
for the *consume* side. Each screen has BOTH, and the user picks.

### Two-way pairing matrix

The pairing flow has 4 atomic UI moments. Each one has a "show"
side and a "consume" side. Both modes coexist:

| Moment | Show side (this peer) | Consume side (other peer) |
|---|---|---|
| Offer | host renders QR + textarea | joiner scans OR pastes |
| Answer | joiner renders QR + textarea | host scans OR pastes |

**Symmetry**: every screen has identical "QR + Copy + Camera + Paste"
affordances. No screen is QR-only or textarea-only.

### Visual layout (mobile portrait, host page after Open room)

```
┌───────────────────────────────────┐
│  LAN host (WebRTC)                │
│                                   │
│  Status: AwaitingAnswer           │
│                                   │
│  Show this QR to the joiner:      │
│                                   │
│  ┌─────────────────────────────┐  │
│  │                             │  │
│  │      ███ ▀▀█  ███           │  │
│  │      ███▄▀ ▀  ▀█▀ (QR SVG)  │  │
│  │      ▀▀█▀▄▄▀▄▀▄▄            │  │
│  │                             │  │
│  └─────────────────────────────┘  │
│                                   │
│  ▼ Or copy as text                │  ← collapse toggle
│                                   │
│  Once joiner sends back answer:   │
│                                   │
│  ┌─────────────────────────────┐  │
│  │  📷  Scan answer            │  │
│  └─────────────────────────────┘  │
│  ┌─────────────────────────────┐  │
│  │ ▾ Or paste answer text      │  │
│  └─────────────────────────────┘  │
│                                   │
└───────────────────────────────────┘
```

When the user taps "Scan answer", a full-screen camera view
overlays the page (closer to native modal than inline `<video>` —
makes aiming easier).

### Camera modal layout

```
┌───────────────────────────────────┐
│  ✕ Cancel                         │
│                                   │
│   ┌──── camera viewfinder ────┐   │
│   │                           │   │
│   │       [ live video ]      │   │
│   │                           │   │
│   │   ┌──── square frame ──┐  │   │
│   │   │                    │  │   │
│   │   │  point QR inside   │  │   │
│   │   │                    │  │   │
│   │   └────────────────────┘  │   │
│   │                           │   │
│   └───────────────────────────┘   │
│                                   │
│  Hold steady…                     │
│                                   │
└───────────────────────────────────┘
```

On successful decode → modal closes → page state advances as if
user had pasted the text and tapped the next-step button. Failed
decode after 30 s → "couldn't read QR — paste text instead" message
+ modal stays open with a "Try again" button.



## UX flow (final, accounting for camera availability)

### Same-LAN, both have cameras (the happy path)

```
Host (phone A)                    Joiner (phone B)
─────────────────                 ─────────────────
Tap "Open room"
↓
Offer QR appears
                                  Tap "Scan camera" 
                                  ↓
                                  Camera modal
                                  ↓
                                  Point at host's QR
                                  ↓
                                  (auto-decoded)
                                  ↓
                                  Auto-tap Generate answer
                                  ↓
                                  Answer QR appears
Tap "Scan answer"
↓
Camera modal
↓
Point at joiner's QR
↓
(auto-decoded → auto-tap Accept)
↓
Game starts                       Game starts
```

User-visible taps: 3 (Open room, Scan camera, Scan answer).
Total time on practiced run: ~15 seconds.

### Mixed cameras (laptop host, phone joiner)

Host has no camera; joiner does.

```
Host (laptop)                    Joiner (phone B)
─────────────────                 ─────────────────
Tap "Open room"
↓
Offer QR appears
(camera button hidden / greyed)
                                  Tap "Scan camera"
                                  ↓
                                  Scan host's screen
                                  ↓
                                  Generate answer
                                  ↓
                                  Answer QR appears
                                  ↓
                                  + textarea showing answer text
                                  ↓
                                  Tap "Copy answer"
Switch tab; user shouts          Switch app; AirDrop or text the
"send me the answer text"         answer text to the laptop
↓
Paste text into the textarea
↓
Tap "Accept answer"
↓
Game starts                       Game starts
```

User-visible taps: 5 + 1 cross-device handoff. Total time: ~30 s.

### No cameras (both laptops, or all permissions denied)

Falls back to today's textarea-only flow. No regression — Phase 5
behaviour preserved exactly.

### iOS Safari camera permission flow

First scan triggers Safari's permission prompt: "192.168.31.136
wants to use your camera". User taps Allow. Subsequent scans skip
the prompt for the session.

If denied: scan button shows "Camera blocked" + "open Settings →
Safari → Camera → Allow" hint + the textarea path stays open.



## Tech choices

### QR rendering (host displays offer; joiner displays answer)

Two pure-Rust options, both wasm-friendly:

| Crate | Output | Size | Notes |
|---|---|---|---|
| **`qrcode = "0.14"`** | SVG, PNG, Unicode | ~30 KB | Mature, widely used, supports all error-correction levels. SVG output embeds inline into the page — no canvas needed. **Pick this.** |
| `qrcodegen` | bit-array (caller renders) | ~10 KB | Smaller but caller has to write SVG/canvas drawing code. Not worth the savings. |

Decision: `qrcode = "0.14"` with SVG output. Embed the resulting SVG
string into the page via `view! { <div inner_html=svg_string />}`.

**Capacity check** (offer SDP is ~660 bytes JSON envelope, mostly
non-alphanumeric so byte mode applies):

| QR version | Byte mode capacity at ECC level M |
|---|---|
| v25 | 718 bytes |
| v30 | 1085 bytes |
| v40 (max) | 2331 bytes |

Use **version-auto with ECC level M** (Medium, ~15% recovery). Our
660-byte payload fits in v25 with headroom; the auto-version
selection picks the smallest version that fits the payload.

### QR scanning (joiner scans offer; host scans answer)

Two options:

| Library | Lang | Size | Performance | Notes |
|---|---|---|---|---|
| **`jsQR`** | JS | ~50 KB minified | ~30 ms per frame on a phone | Industry-standard pure-JS decoder. Fed `Uint8ClampedArray` (RGBA pixels) + width + height. |
| `rqrr = "0.7"` | Rust | adds ~80 KB to wasm bundle | ~15 ms per frame (faster) | Pure Rust, but more code to integrate (canvas frame capture loop in Rust). |

**Pick `jsQR` for v1**. Reasons:
- Battle-tested across millions of web QR scanners (no novel bugs).
- The JS-side video frame loop is well-documented.
- Smaller incremental bundle size than `rqrr` (50 KB JS gzips to
  ~17 KB; `rqrr` adds ~30 KB to the wasm gzip).
- Easy to swap to `rqrr` later if performance shows up as an issue.

Bundle via `jsQR.js` as a static file in `clients/chess-web/static/`
+ a `<script>` tag in `index.html`. Trunk's PWA service worker
precaches it automatically (existing pattern).

### Camera capture

Use vanilla `web_sys::HtmlMediaElement` + `MediaStream` API — no
extra crate needed:

```rust
let stream = JsFuture::from(
    nav.media_devices()?.get_user_media_with_constraints(
        MediaStreamConstraints::new()
            .video(&JsValue::from_serde(&serde_json::json!({
                "facingMode": "environment"
            }))?)
    )?
).await?;
let video: HtmlVideoElement = ...;
video.set_src_object(Some(&stream.into()));
```

`facingMode: "environment"` requests the back camera on phones (better
for QR scanning than the selfie camera). Falls back to user-facing
camera if no environment camera exists (laptops).

Capture frames via `<canvas>`:

```rust
// every animation frame:
ctx.draw_image_with_html_video_element(&video, 0.0, 0.0)?;
let img_data = ctx.get_image_data(0.0, 0.0, w as f64, h as f64)?;
let result = jsQR(img_data.data(), w, h);  // ← JS interop
if let Some(text) = result.data { ... }
```

### Permissions UX

`navigator.permissions.query({ name: 'camera' })` is supported on
Chrome / Firefox / Safari 16+. Use it to detect "denied" before
trying `getUserMedia`, so the UI can show "open Settings" hint
without an extra prompt cycle.

### SDP envelope size — does it still fit?

Phase 5 SDP envelopes measured at 657 bytes (offer) and 656 bytes
(answer) on Chrome localhost. Adding 4 STUN servers may grow the
SDP by ~50–80 bytes per server (srflx candidates). Worst case
(STUN mode + ICE produced 4 srflx candidates) ~1000 bytes. Still
fits in QR v30 byte mode (1085 bytes) — the version-auto logic
handles it.



## Implementation plan

Six commits in dependency order. Each is independently CI-green and
ship-able.

### Commit 1 — `qrcode` crate dep + QR rendering helper (0.5d)

- Add `qrcode = "0.14"` to `clients/chess-web/Cargo.toml`.
- New `clients/chess-web/src/components/qr.rs`:
  ```rust
  #[component]
  pub fn QrCode(payload: Signal<String>, label: String) -> impl IntoView {
      let svg = move || {
          let payload = payload.get();
          if payload.is_empty() { return None; }
          QrCode::new(payload.as_bytes())
              .ok()
              .map(|c| c.render().min_dimensions(200, 200).to_string())
      };
      view! {
          <figure class="qr-card">
              <div inner_html=svg />
              <figcaption>{label}</figcaption>
          </figure>
      }
  }
  ```
- Render the offer + answer QRs in `pages/lan.rs`, side-by-side
  with the existing textareas (textarea kept; QR is added).
- **No camera scanning yet** — this commit only makes QRs visible
  for both peers to point cameras at.
- Verify via `playwright-cli`: SVG renders, payload encodes correctly
  (decode via `jsQR.js` in eval to verify).

### Commit 2 — `static/jsQR.min.js` + JS-Rust interop wrapper (0.25d)

- Drop `jsQR.min.js` (~50 KB) into `clients/chess-web/static/` so
  Trunk copies it into `dist/` automatically.
- Add `<script src="/jsQR.min.js" defer></script>` to `index.html`.
- New `clients/chess-web/src/qr_decode.rs`:
  ```rust
  #[wasm_bindgen]
  extern "C" {
      #[wasm_bindgen(js_name = jsQR)]
      fn js_qr(data: &Uint8ClampedArray, width: u32, height: u32) -> JsValue;
  }
  pub fn decode_rgba(data: &Uint8ClampedArray, w: u32, h: u32) -> Option<String> {
      let result = js_qr(data, w, h);
      if result.is_null() { return None; }
      Reflect::get(&result, &"data".into()).ok().and_then(|v| v.as_string())
  }
  ```
- Verify: write a unit test (`#[wasm_bindgen_test]`) that feeds a
  known QR PNG → decode → matches expected string.

### Commit 3 — Camera capability detection (0.25d)

- New `clients/chess-web/src/camera.rs`:
  ```rust
  pub async fn has_camera() -> bool { ... }
  pub async fn camera_permission() -> CameraPermission { ... }
  ```
- Wire into `pages/lan.rs`: on mount, set `has_camera_signal`
  + `cam_permission_signal`; gate the "Scan camera" button visibility
  + label on these signals.
- Verify: works on Chrome with camera, Chrome without camera (use
  `navigator.mediaDevices.getUserMedia` mocked), Safari iOS.

### Commit 4 — Camera modal + frame loop + scan integration (0.5d)

- New `clients/chess-web/src/components/qr_scanner.rs`:
  ```rust
  #[component]
  pub fn QrScanner(
      open: Signal<bool>,
      on_decode: Callback<String>,
      on_cancel: Callback<()>,
  ) -> impl IntoView { ... }
  ```
- Implements: getUserMedia → `<video>` → requestAnimationFrame loop
  → drawImage to offscreen canvas → getImageData → decode_rgba →
  on hit, call `on_decode(text)` once, stop the loop, close modal.
- Wire into `pages/lan.rs`: when user taps "Scan camera", set
  `scanner_open = true`. On decode callback, fill the corresponding
  textarea (offer / answer) and AUTO-FIRE the next-step action
  (Generate answer / Accept answer).
- 30 s timeout: if no decode, show "couldn't read QR" + retry button.

### Commit 5 — Permission-denied UX + accessibility polish (0.25d)

- "Camera blocked" badge with platform-specific hint text.
- ARIA labels on the camera button + modal.
- Reduced-motion-friendly modal animations.
- Test on iOS Safari with camera denied (Settings → Safari → Camera
  → Deny scenario).

### Commit 6 — End-to-end playwright-cli test + docs update (0.25d)

- Extend `scripts/test-lan-pairing.sh` (or create it from the
  inline ops we used) to drive the QR-based flow:
  - Tab 0 (host) opens room → snapshot QR SVG → extract payload
    via `eval`.
  - Decode the payload with `qrcode` Rust → confirm round-trip.
  - Tab 1 (joiner) injects the offer payload directly via `eval`
    (skip the camera; test the underlying state machine).
- Update `docs/lan-multiplayer-webrtc.md` "Future work" section
  with shipped status + new subsection "QR pairing flow".
- Move the entry from TODO.md `## P2` to `## Done`.



## Open questions

- **Layout: side-by-side QR+textarea, or tabs?** Side-by-side wastes
  screen space on mobile portrait but is the most discoverable. Tabs
  ("📷 Camera" / "📋 Text") save space but add a click. Lean
  side-by-side with the textarea collapsed-by-default; user can
  toggle.

- **Auto-fire next-step on scan, or require explicit "Accept"
  confirmation?** Auto-fire is faster but loses the "review what
  I scanned" moment. Lean auto-fire with a 1-second visual
  confirmation ("✓ Decoded — connecting…") so user can hit Cancel
  if it's the wrong QR.

- **Test injection bypass for playwright-cli?** Camera frame capture
  isn't easy to fake in playwright. Need either:
  (a) An `eval` hook that bypasses the camera and feeds payloads
      directly into the page's state machine.
  (b) A `--debug-mode` URL flag that exposes a hidden textbox just
      for tests.
  Decision: (a) — write `window.__lan_debug_inject_offer(text)` /
  `__lan_debug_inject_answer(text)` Rust-exported functions gated
  on debug-cfg.

- **Should we add a "share via NFC" button on Android Chrome?**
  Web NFC API works only on Android Chrome (≠ iOS). Could be a
  Phase 5.6 polish if user demand emerges. Skip for v1.

- **QR contrast / size for camera robustness**: render at 280×280
  CSS pixels minimum (large enough for a phone camera at arm's
  length). Use ECC level M (15% damage tolerance). High contrast
  black-on-white. Test on real phones with the QR shown on:
  - Phone screen (small, bright) ✓ expected to work
  - Laptop screen (large, may have glare) ⚠️ test
  - Printed paper (no backlight) ⚠️ test (covers the "asynchronous
    pairing via printout" use case if anyone wants it)

- **Lighting failure mode**: if the camera can't focus or the
  scene is dark, jsQR returns null indefinitely. Add a "torch"
  toggle if `MediaStreamTrack.getCapabilities().torch == true`
  (Android Chrome only; iOS Safari doesn't expose torch). Phase
  5.6 polish, skip for v1.



## Out of scope (v1; track in Phase 6 or later)

- **Web NFC tap-to-pair** — Android Chrome only, excludes iOS.
  Track as Phase 5.6 if iOS users get NFC support.
- **Pure-Rust QR decoder (`rqrr`)** — performance optimization;
  swap for jsQR only if real-device testing shows decode latency
  > 100 ms.
- **Torch toggle in low light** — Android Chrome only; iOS Safari
  doesn't expose `MediaStreamTrack.torch`. Track as Phase 5.6.
- **Multi-frame QR** for >2 KB SDPs — current ~660 byte payloads
  fit in a single QR, no need for chunked encoding.
- **Spectator slot UI** — Phase 6 polish, separate concern.
- **`beforeunload` confirm on host tab close** — Phase 6 polish.
- **Pre-paired persistent peers** — technically infeasible (every
  WebRTC session needs fresh SDP + ICE candidates).

## Related

- [`backlog/webrtc-lan-pairing.md`](webrtc-lan-pairing.md) — parent
  feature; Phase 0–5 design history.
- [`docs/lan-multiplayer-webrtc.md`](../docs/lan-multiplayer-webrtc.md)
  — long-form recipe; gets a "QR pairing" subsection added in
  Commit 6.
- [`pitfalls/ios-safari-svg-click-no-tap.md`](../pitfalls/ios-safari-svg-click-no-tap.md)
  — relevant if camera modal close button uses SVG.
- [`pitfalls/wasm32-systemtime-now-panics.md`](../pitfalls/wasm32-systemtime-now-panics.md)
  — same audit applies to any new code path that runs in-browser.
- TODO entry: `[L] WebRTC LAN pairing — Phase 5.5 + 6 polish` in
  `## P2`.


