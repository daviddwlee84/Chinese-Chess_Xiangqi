# LAN host/join pairing on GitHub Pages — fix jsQR + ICE diagnostics

## Context

Two bugs reported on `https://daviddwlee84.github.io/Chinese-Chess_Xiangqi/lan/host` (LAN WebRTC pairing, Phase 5/5.5):

1. **Copy-text flow reaches "Accept answer" then errors out with**
   `ERROR: DataChannel did not open within 10 s — pairing failed (network blocked?)`.
   Offer/answer SDPs both have only `.local` mDNS host candidates; toggling
   "Use STUN" doesn't help — error persists, and offer generation visibly slows
   (5 s ICE-gather timeout firing because one of the STUN servers is unreachable).

2. **QR scanner on iOS Safari fails immediately with**
   `JsValue("jsQR script load error")` — the user can't even open the camera flow.

### Root causes

**Issue 2 (clear code bug, the priority fix).**
`clients/chess-web/src/qr_decode.rs:109-110` builds the script URL as:

```rust
let origin = win.location().origin()?;
let url = format!("{origin}/jsQR.min.js");
```

On the deployed GitHub Pages target the app lives at `https://daviddwlee84.github.io/Chinese-Chess_Xiangqi/...`. `origin` is just `https://daviddwlee84.github.io` (no path), so the constructed URL `{origin}/jsQR.min.js` resolves to a 404 — the asset is actually at `/Chinese-Chess_Xiangqi/jsQR.min.js`. The script `error` event fires and the loader's promise rejects with `"jsQR script load error"`. The doc comment at lines 12–18 explicitly claims this works on GitHub Pages subpaths, but the implementation doesn't actually consume the base path. The PWA service-worker side handles base paths via `__BASE_PATH__` substitution (`scripts/build-pwa.sh`), and the Leptos router consumes `routes::base_path()` (`src/routes.rs:30`); the jsQR loader just never got plumbed through.

**Issue 1 (not directly fixable in code — it's a network/router-level WebRTC quirk).**
This is the documented pitfall in `pitfalls/webrtc-mdns-lan-ap-isolation.md`: modern browsers obfuscate LAN host candidates as `<uuid>.local` mDNS names, and on certain consumer routers (Xiaomi AX9000 confirmed) WebRTC's per-session dynamic `.local` resolution fails even though general mDNS (AirPlay, Mi IoT) works. STUN srflx fallback also doesn't rescue NAT hairpin on the same network (user's STUN-on test confirms this). The known workaround is iPhone-hotspot pairing (Approach A in the pitfall doc); a proper code fix requires a chess-net signalling endpoint (Approach C, backlog `webrtc-lan-pairing.md`).

What we can do in code: make the failure mode *legible* to the user / future debuggers so they don't have to read pitfall docs to know what's happening — surface the live ICE / connection state in the page, log state transitions to the JS console, include the final state in the timeout error, and re-snapshot the SDP at copy time so late-arriving candidates aren't lost.

### Intended outcome

- iOS users can use the QR scanner camera flow on the GH Pages deploy (issue 2 fully fixed).
- When LAN pairing fails (issue 1 — networks where mDNS can't resolve), the user sees what went wrong (e.g. "ICE state: disconnected — try iPhone hotspot or enable STUN") instead of a generic "network blocked?", and the offer/answer text/QR always reflect the latest candidates rather than the half-gathered snapshot. Root-cause network fix (signalling broker) is explicitly deferred to a future Phase 5 follow-up.

---

## Commit 1 — Issue 2: jsQR loader respects base path

**Critical files:**

- `clients/chess-web/src/qr_decode.rs` (lines 98–141, especially 109–110)
- `clients/chess-web/index.html` (comment lines 55–61 — currently lies that the loader works on GH Pages)

**Change:**

In `qr_decode.rs::ensure_loaded()`, replace:

```rust
let origin = win.location().origin().map_err(|_| JsValue::from_str("no origin"))?;
let url = format!("{origin}/jsQR.min.js");
```

with:

```rust
use crate::routes::base_path;

let origin = win.location().origin().map_err(|_| JsValue::from_str("no origin"))?;
let base = base_path();   // "" on server deploy, "/Chinese-Chess_Xiangqi" on GH Pages
let url = format!("{origin}{base}/jsQR.min.js");
```

`base_path()` is the existing helper from `routes.rs:30`, baked at build time from `CHESS_WEB_BASE_PATH` env var (set by `Makefile` line 72's `build-web-static` recipe). It returns `""` for `make build-web` (root deploy) and `/Chinese-Chess_Xiangqi` for `make build-web-static WEB_BASE=Chinese-Chess_Xiangqi`. The string already has its leading `/` normalized away in the empty case, so concatenation works for both.

Touch up the inaccurate comment block at the top of `ensure_loaded()` (lines 12–18) and the `index.html` comment at lines 55–61 to describe the actual behavior.

---

## Commit 2 — Issue 1: ICE/connection-state diagnostics + re-snapshot SDP on Copy

This is scoped to "make the failure mode visible" — no STUN default change, no signalling-broker work.

**Critical files:**

- `clients/chess-web/src/transport/webrtc.rs` (install state-change listeners; expose a live `ReadSignal<IceDiag>` on `HostHandshake` / `JoinerHandshake`; widen the per-page SDP exposure to re-read `pc.local_description()` reactively)
- `clients/chess-web/src/pages/lan.rs` (consume the diag signal in the UI; show inline "ICE: checking → connected" badge; widen the 10-s timeout error to include the final ICE state; re-bind the offer/answer textareas + QR payloads to the live `pc.local_description()` instead of the captured snapshot)
- `clients/chess-web/Cargo.toml` (no changes expected — `RtcIceConnectionState`, `RtcIceGatheringState`, `RtcPeerConnectionState` are all already in the web-sys features at lines 38–45)

### 2a. Wire ICE / connection / gathering state-change handlers

Add to both `connect_as_host()` (around line 302 of `webrtc.rs`, before `create_offer`) and `connect_as_joiner()` (around line 252, after creating the PC):

- `pc.set_oniceconnectionstatechange(...)` — `console.log` the new value of `pc.ice_connection_state()`. Update a reactive `WriteSignal<IceDiag>` field.
- `pc.set_onconnectionstatechange(...)` — same pattern for `pc.connection_state()`.
- `pc.set_onicegatheringstatechange(...)` — keep the existing wait-for-`complete` closure but ALSO push state changes into the diag signal. (Two listeners on the same event is fine; web-sys's setter replaces, so refactor to a single closure that updates the signal AND resolves the gather-complete promise when state == complete.)
- Closures pushed into `_keepalive` (the existing pattern at lines 449–482) so they live as long as the handshake.

Define a small struct in `webrtc.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IceDiag {
    pub ice: RtcIceConnectionState,
    pub conn: RtcPeerConnectionState,
    pub gather: RtcIceGatheringState,
}
```

Add `pub ice_diag: ReadSignal<IceDiag>` to both `HostHandshake` and `JoinerHandshake`. Initialise with the PC's current states at construction time.

### 2b. Use the final ICE state in the 10-s timeout error

In `pages/lan.rs` lines 144–151, change:

```rust
if !wait_for_dc_open(&dc, 10_000).await {
    set_error_msg.set(Some(
        "DataChannel did not open within 10 s — pairing failed (network blocked?)".into(),
    ));
    ...
}
```

to read the latest `ice_diag` snapshot and produce a message like:

```
DataChannel did not open within 10 s — ICE state: <ice>, connection: <conn>.
Common fixes: switch both devices to an iPhone/Android personal hotspot
(this network's WebRTC mDNS resolution may be failing — see
docs/pwa.md for the documented Xiaomi-router quirk), or enable "Use STUN".
```

Keep the message ~3 lines, no rant. The state values matter most for debugging.

### 2c. Inline live ICE-state badge on the LAN host/join pages

After the existing status line `<p>"Status: " {move || format!("{:?}", status.get())}</p>` (line 208 of host, line 429 of join), add a second line that displays the current `ice_diag` reactively. Roughly:

```rust
<Show when=move || handshake_present.get()>
    <p style="font-size:13px;color:#888">
        "ICE: " {move || ice_diag.get().ice_label()}
        " · connection: " {move || ice_diag.get().conn_label()}
        " · gathering: " {move || ice_diag.get().gather_label()}
    </p>
</Show>
```

Where `*_label()` are small helpers mapping the web-sys enum to short strings ("new", "checking", "connected", "disconnected", "failed", "closed"). This is the load-bearing user-facing change — a user pairing on a flaky network can SEE "ICE: failed" instead of staring at "Status: AcceptingAnswer" for 10 s with no insight.

The `handshake_present` signal is the existing `status.get()` being any state past `Idle`. For host page that's everything after `on_open` succeeds; for joiner that's after `on_generate` succeeds.

### 2d. Re-snapshot SDP on Copy

Currently the offer/answer payload is captured once at `connect_as_*` return time and stored in `offer_blob` / `answer_blob` signals. Background ICE gathering continues; later candidates are silently lost from the user-visible textarea / QR.

Fix: bind the displayed SDP to `pc.local_description()` reactively. The cheapest way:

1. Add a `pub fn current_offer(&self) -> OfferBlob` method on `HostHandshake` that reads the PC's `local_description()` at call time, re-runs `encode_sdp(...)`, and returns a fresh blob. Same `current_answer()` for `JoinerHandshake`.
2. In `lan.rs`, replace the initial `set_offer_blob.set(hh.offer.0.clone())` capture with a derived signal that calls `handshake.borrow().as_ref()?.current_offer()` and reruns on a tick signal driven by `oniceicecandidate` / `onicegatheringstatechange`.
3. Simpler alternative: bind a `create_effect` to `ice_diag` (which already fires on gathering state change) and have it re-run `current_offer()` and set `offer_blob`.

The QR code re-renders automatically because `<QrCodeView payload=Signal::derive(move || offer_blob.get())>` is reactive.

Also wire `pc.set_onicecandidate(...)` to update the diag signal on each candidate arrival — this gives the "Copy offer" button its triggers in real time (each new candidate causes the offer text to refresh). Pure UX: the user can wait, watch the byte count tick up, and copy when they're satisfied.

### 2e. Show candidate counts (optional polish)

In the `ice_diag` struct also include `pub candidates: u32`, incremented by the `onicecandidate` handler. Render as `· candidates: N` in the inline badge. Lets the user see "candidates: 1" (only `.local`, mDNS-only) vs "candidates: 3" (got srflx). Trivial to add since we're already wiring `onicecandidate`.

---

## Verification

```bash
# 1. Workspace sanity (issue 2 + 1 should both compile)
cargo check --workspace

# 2. Native chess-web pure-logic unit tests (15 of them — covers
#    routes.rs::base_path used by qr_decode after the change)
cargo test -p chess-web

# 3. Clippy clean
cargo clippy --workspace --all-targets -- -D warnings

# 4. WASM build (the actual GH Pages target)
make build-web-static WEB_BASE=Chinese-Chess_Xiangqi

# 5. Confirm jsQR.min.js was actually emitted at the expected path
ls -l clients/chess-web/dist-static/jsQR.min.js
test -f clients/chess-web/dist-static/jsQR.min.js && echo "asset present"

# 6. Local PWA-style serve at the GH Pages subpath
make serve-web-static WEB_BASE=Chinese-Chess_Xiangqi
# → serves on http://localhost:4173/Chinese-Chess_Xiangqi/

# 7. Use playwright-cli to load the join page (which has the camera
#    button visible), open the scanner, and confirm the loader fires
#    a GET to /Chinese-Chess_Xiangqi/jsQR.min.js (not /jsQR.min.js).
#    Specifically:
#    - browser_navigate http://localhost:4173/Chinese-Chess_Xiangqi/lan/join
#    - browser_evaluate to grant camera mock + flip cam_available
#    - browser_click "📷 Scan offer QR"
#    - browser_network_requests | grep jsQR.min.js
#    - assert path contains /Chinese-Chess_Xiangqi/
#    - assert response is 200 (not 404)

# 8. Diagnostic UX smoke (issue 1) — Trunk dev server is fine here
make play-web
# Open http://localhost:8080/lan/host in tab A, /lan/join in tab B
# Tab A: tap Open room → confirm "ICE: …" badge appears under Status,
#        confirm console.log output for ice/connection state changes,
#        confirm the byte-count next to Copy offer ticks up as ICE
#        gathers further candidates.
# Tab A: tap Accept answer with a deliberately broken SDP → confirm
#        the 10-s timeout error now includes "ICE state: …"
#        instead of just "network blocked?".

# 9. Full LAN flow on localhost (clean network, no router) — both tabs
#    in same browser actually pair successfully via mDNS bypass through
#    loopback. Acts as regression check that the diagnostics changes
#    didn't break the happy path.
```

**Cannot verify locally:** the actual user-reported LAN failure (cross-device mDNS resolution failing on their physical router). That requires their two real devices and isn't reproducible from playwright. The diagnostics changes above are intended exactly so the user can self-diagnose the next time the failure recurs, and so the message points them at the documented workaround (iPhone hotspot).

---

## Out of scope (call out in commit body)

- **STUN default = ON.** User's screenshot confirms STUN srflx doesn't bypass the failure on their network; flipping the default would only slow down offer generation (5 s ICE-gather timeout to wait for an unreachable STUN server) without fixing the connection. Keep default OFF.
- **chess-net signalling broker (Approach C).** The right long-term fix for users on networks that fail mDNS *and* STUN-hairpin, but a much bigger change (new server endpoint, persistent ICE-candidate trickle channel, new protocol envelope). Already tracked in `backlog/webrtc-lan-pairing.md` as a P3 follow-up.
- **Trickle ICE in copy-paste mode.** Re-snapshotting on Copy is a partial workaround; true trickle requires bidirectional signalling that the manual-copy model can't provide.
- **`getUserMedia` exposes real LAN IPs (Approach C in the pitfall doc).** UX nonstarter — asking for mic permission to start a chess game.

If user wants any of these followups, add to `TODO.md` via `scripts/add-todo.sh` after the two commits land.
