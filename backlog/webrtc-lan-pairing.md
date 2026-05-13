# WebRTC LAN pairing — PWA-only multiplayer without external network

## Why this is in backlog

User asked: can we move chess-net's wss:// hosting into the PWA itself,
so two players on the same LAN can play without any external server?

The motivation is **two phones on the same WiFi, fully offline at runtime**
— e.g. on a train, in a classroom, in a public park with no usable mobile
data. Today's online lobby (`wss://chess-net-server`) needs both peers to
reach the same hosted server; even when both phones are on the same router,
traffic round-trips through the public Internet.

Three constraints came out of the kickoff conversation (2026-05-11):

1. **LAN-only at runtime** — no external network dependency once both
   PWAs are installed.
2. **Stay PWA-only** — no native sidecar binary; nothing to install
   beyond the existing `chess-web` build.
3. **On-device pairing** — pairing happens on the two phones, not via
   share-sheet to a messaging app. URL-based invites are out because
   they require app-switching to deliver.

Replaces the previous P? entry (`TODO.md` 2026-05-11) which scoped this
as "WebRTC fallback for hosting cost" — the actual driver turns out to
be offline-LAN play, which is a stronger reason and a different design.

## Constraints (locked-in)

| Item | Decision | Rationale |
|---|---|---|
| Player count | 1v1 (host = 1 seat, joiner = 1 seat) | Matches existing chess-net seat model; multi-peer mesh is a follow-up |
| Spectators | Host accepts up to **4** spectator DataChannels | More requires too many QR scans; can be raised later |
| Pairing | **2 QR scans** (offer + answer, swap-roles) | No external signalling; `getUserMedia` available in PWA on both iOS Safari and Android Chrome once the page was originally served over HTTPS |
| Network | RTC `iceServers: []` (mDNS `.local` candidates only) | Forces same-LAN; zero external dependency |
| PWA bootstrap | Both phones must have loaded the PWA over HTTPS at least once (e.g. via GitHub Pages) and let the service worker cache it | iOS Safari requires HTTPS context for `getUserMedia` and for service worker to be registered; user explicitly accepted this prerequisite |
| Variant scope | xiangqi + banqi v1 | Three-kingdom (3-player) needs mesh; defer until 三國暗棋 itself ships |
| Wire protocol | Sealed; reuse `chess-net::protocol` v5 unchanged | Host-in-WASM is just a new transport for the same `ClientMsg`/`ServerMsg` schema |
| Reconnect | Out of scope v1 | Host tab close = room ends; document clearly in UI |
| Three-kingdom 3-player | Out of scope v1 | Needs N-peer mesh AND the variant itself; tracked as separate follow-up |

## Why "PWA as WS server" is impossible

A browser tab — including PWAs and Service Workers — **cannot bind a
listening TCP socket**. This is a fundamental browser security boundary,
not a permission we can request:

- `WebSocket` API is client-side only. There is no server-side counterpart.
- `WebTransport` requires server-side QUIC; the browser is still client.
- Service Workers can only intercept fetch events for their own origin —
  they don't accept inbound connections.
- `BroadcastChannel` / `MessageChannel` are same-origin only; useless across
  devices.

The current `chess-net::server::run` calls `tokio::net::TcpListener::bind`
(see `crates/chess-net/src/server.rs:240`). That code path will never
execute in WASM.

The only browser API that lets two devices exchange bytes without one of
them listening on a public port is **WebRTC DataChannel**: peers go
through ICE / STUN / TURN to find a path, then talk peer-to-peer. For
same-LAN traffic, browsers since ~2019 emit `<uuid>.local` mDNS ICE
candidates that resolve over the local network without any STUN server.

## Approaches considered

### A. Native sidecar binary

Ship `chess-net-server` as a per-platform installer; host runs it locally,
PWA connects to `ws://<host-lan-ip>:7878`.

- ✅ Functionally exists today (`--static-dir` already serves PWA + WS).
- ❌ Phones can't host (no native binary on iOS/Android).
- ❌ Violates "stay PWA-only" constraint.

Useful as a fallback later for desktop hosts; not the chosen path.

### B. Pure WebRTC P2P with manual QR signalling — **chosen**

Host's PWA generates SDP offer + mDNS ICE candidates → encodes as QR.
Friend scans → produces SDP answer → encodes as QR. Host scans answer
→ DataChannel ready.

- ✅ Zero external dependency.
- ✅ Same-LAN guaranteed via mDNS.
- ✅ Works on iOS Safari and Android Chrome (subject to spike confirmation).
- ⚠️ Two QR scans per join is real UX cost; acceptable per user input
  ("on-device pairing is friendlier than URL relay through messaging app").

### C. WebRTC P2P with self-hosted signalling endpoint

Same as B but SDP/ICE flow through a tiny `/signal/<token>` endpoint on
chess-net (which the user would still need to host).

- ✅ Smoother UX (no QR scanning).
- ❌ Still needs a server reachable by both peers, defeating the
  offline-LAN goal.

Reserved as **automatic fallback if Phase 0 spike shows iOS Safari
won't connect over pure mDNS** — in that case we could ship a tiny
LAN-only signalling server bundled with chess-net for the spec-only
case where the user happens to also be running chess-net somewhere.

### D. Web Bluetooth / Web NFC for OOB SDP

- ❌ iOS Safari supports neither.
- ❌ Web Bluetooth requires user gesture per connection and pairing UX
  is worse than QR.

### E. Web Share Target API + AirDrop / Nearby Share

- ✅ Could deliver the offer to the friend's installed PWA via OS share.
- ❌ Requires PWA install on both ends (we already accept this).
- ❌ Two-way exchange (offer → answer) requires share-back, which is
  per-OS-glue heavy.
- ❌ Doesn't actually save UX over QR for two-step exchange.

Filed for a possible future "Share Target" enhancement once Phase B ships.

## Chosen approach

**Approach B: pure WebRTC P2P + 2-QR-scan pairing.**

Architecture:

```
┌─────────────────────────┐                     ┌─────────────────────────┐
│ Host phone PWA          │                     │ Joiner phone PWA        │
│ ┌─────────────────────┐ │                     │ ┌─────────────────────┐ │
│ │ chess_net::room::   │ │                     │ │ pages/play.rs       │ │
│ │   Room (authority)  │ │                     │ │ + Transport         │ │
│ └──────────┬──────────┘ │                     │ └──────────┬──────────┘ │
│            │            │                     │            │            │
│ ┌──────────┴──────────┐ │  RtcDataChannel     │ ┌──────────┴──────────┐ │
│ │ host_room.rs        │◄├═════════════════════├►│ transport/webrtc.rs │ │
│ │ (multi-peer router) │ │  (mDNS .local ICE)  │ │                     │ │
│ └─────────────────────┘ │                     │ └─────────────────────┘ │
└─────────────────────────┘                     └─────────────────────────┘
```

Key insight: **the wire protocol does not change**. Today's chess-net
server already converts WS frames → `ClientMsg` → mutate `RoomState` →
emit `ServerMsg`s. We extract that mutation into a transport-agnostic
`Room` module (Phase 1), then run it from the host PWA's WASM (Phase 4)
fed by RtcDataChannel frames instead of WS frames.

Joiner side is even simpler: same `pages/play.rs` consuming `ServerMsg`s
that come off any `Transport`. The only new code is the WebRTC transport
implementation.

## Phase plan

### Phase 0 — Spike (0.5–1 day, NO production code)

Goal: validate three unknowns before committing 5+ days of work.

1. **iOS Safari + mDNS-only ICE**: do two iPhones on the same WiFi
   actually connect with `iceServers: []` and `.local` candidates only?
   Build a smallest-possible `RtcPeerConnection` echo demo (`alert("got: " + msg)`).
2. **SDP blob size**: gather the full local description (offer with
   ICE candidates included), `JSON.stringify` → deflate → base64. Target
   ≤2 KB for a comfortable QR version 25-30 alphanumeric. Larger means
   either splitting across multiple QRs or relaxing the no-STUN
   constraint.
3. **Camera decode latency**: jsQR (~50 KB JS) vs `rqrr` (pure Rust,
   needs JS frame-grab plumbing). Try jsQR first; expect ≥10 fps decode
   on a mid-range phone.

Output: `pitfalls/webrtc-mdns-ios-quirks.md` if any quirks surface;
notes appended to this doc with the measured numbers.

**Gate**: spike must succeed end-to-end on at least one iOS device + one
Android device. Failure → reconsider Approach C (signalling endpoint).

#### Spike scaffolding (shipped 2026-05-12)

`clients/chess-web/src/spike/` ships the throwaway plumbing:

- `lan_echo.rs` — `/spike/lan/host` and `/spike/lan/join` route components.
- `rtc.rs` — `open_host` / `open_joiner` / `accept_answer` wrappers
  around `web_sys::RtcPeerConnection`. `iceServers: []` hard-coded.

The spike deliberately defers QR encoding (a Phase 5 question) and uses
**plain textareas with raw SDP** for the offer/answer exchange. The OOB
delivery channel is whatever the user has handy — AirDrop, Nearby Share,
SMS, or copy-paste over a regular browser tab. The diag panel reports
the byte count so we can verify QR fits before committing.

> **2026-05-12 spike-build fix**: first run failed with
> `JsValue(SyntaxError: Invalid SDP line.)` on the joiner side. Root
> cause: SDP per RFC 4566 requires CRLF line endings; pasting the raw
> SDP through clipboard / AirDrop / IM apps strips the `\r`. Fixed by
> wrapping `(type, sdp)` in a JSON envelope so transport-side
> normalisation can't break the format, and normalising back to CRLF
> on `set_remote_description`. Each readonly textarea also gained a
> "Copy" button (uses `navigator.clipboard.writeText`) since
> long-press-select on a phone textarea is fiddly. See
> `clients/chess-web/src/spike/rtc.rs::encode_sdp` /
> `decode_sdp_envelope`.

> **2026-05-12 spike second-run finding (iOS Safari + AirDrop)**: with
> the JSON envelope fix in place, the second run on iPhone (host) +
> iPad (joiner) hit a different failure: host
> `JsValue(InvalidStateError: Failed to set remote answer sdp: Called
> in wrong state: stable )`. Both devices' `iceConnectionState` was
> `Failed` even before `accept_answer` ran. Hypothesis: **iOS Safari
> aggressively pauses / tears down WebRTC sessions when the page is
> backgrounded**. Tapping "Copy offer" → AirDrop sheet → switching to
> Notes / iMessage to forward → returning to Safari covers a 30+
> second window in which iOS unilaterally closes the PeerConnection
> (signalling state silently rolls back to `stable`, ICE goes
> `failed`). This is documented Safari behaviour, not a spike bug.
>
> Mitigations applied to the spike (still pending real-world re-test):
>
> - `accept_answer` pre-flights `signalingState`; if it's not
>   `HaveLocalOffer`, surface a clear "iOS paused the page or you
>   double-tapped Start hosting" error before the browser's cryptic
>   one fires.
> - `Start hosting` and `Accept answer` buttons disable after their
>   first use so accidental double-taps can't churn the PC. Failures
>   re-enable so retry is one tap away.
> - Diag panel now shows `signalingState` alongside `iceConnectionState`
>   so the next failure mode is diagnosable from the screenshot alone.
> - Each page got a "Reset (reload page)" button to start a fresh PC
>   without manual reload gymnastics on a phone.
> - Visible warning at the top of the host page: do not switch apps
>   between Start hosting and Accept answer.
>
> **Recommended next-test order** to isolate the iOS-backgrounding
> issue from the underlying RTC plumbing:
>
> 1. **Two macOS Safari tabs on one MacBook** — open `/spike/lan/host`
>    in tab A, `/spike/lan/join` in tab B. Copy via in-page Copy
>    button + paste in the other tab (no app switching, no
>    backgrounding). If this works, the RTC pipe is fine and the
>    iPhone failure was iOS backgrounding.
> 2. **macOS Safari host + iPad Safari joiner over LAN** — host on
>    Mac (immune to mobile backgrounding), joiner on iPad. Use AirDrop
>    answer → MacBook. Tests cross-device while keeping the
>    handshake-sensitive side (host) on a stable platform.
> 3. **Two iOS devices** — only after (1) and (2) succeed. May require
>    the PWA to be installed (`Add to Home Screen`) so iOS treats it
>    as a first-class app rather than a Safari tab. Or fall back to
>    Approach C (LAN signalling endpoint).

> **2026-05-12 spike third-run findings (RTC plumbing OK, network blocks LAN P2P)**:
>
> Test 1 — **Two macOS Safari tabs on one MacBook**: ✅ end-to-end
> green. SDPs exchanged via JSON envelope, signalingState stable on
> both, DataChannel opened, echo messages flowed. Confirms the entire
> RTC pipe (envelope, mDNS-only ICE, host PC + joiner PC, DataChannel
> handlers) is correctly implemented. Offer / answer raw bytes both
> ~660 — well within QR v25 alphanumeric capacity for Phase 5.
>
> Test 2 — **macOS Safari host + iPad Safari joiner over LAN**:
> ❌ host `iceConnectionState: Disconnected` after accepting answer.
> Both signalling states cleanly reach `Stable`, ICE gathering done in
> ~128 ms with one mDNS `.local` host candidate per side, the SDP
> exchange itself succeeds — but the ICE pair check never converges.
>
> Test 2-1 — **macOS Safari host + iPad Chrome joiner over LAN**:
> ❌ same failure shape as Test 2 (`Checking → Failed`). The fact
> that BOTH iPad browsers fail with the *same* symptom while the
> macOS-only path works rules out browser-engine quirks. Common
> failure axis: **cross-device LAN with mDNS `.local` candidates**.
>
> Most likely root cause: the WiFi router is blocking client-to-client
> traffic (AP Isolation / Client Isolation / Wireless Isolation). On
> the test network (subnet `192.168.31.x` — common Xiaomi default)
> this is shipped enabled by some firmware revisions. With AP
> isolation on, the iPad cannot resolve the MacBook's
> `<uuid>.local` mDNS hostname (mDNS multicast UDP/5353 dropped),
> so no candidate pair can be checked, ICE fails. See
> [`pitfalls/webrtc-mdns-lan-ap-isolation.md`](../pitfalls/webrtc-mdns-lan-ap-isolation.md)
> for the full root-cause analysis + mitigation menu.
>
> Spike scaffolding gained a "Use Google STUN (diagnostic)" checkbox
> on both pages so the next test can compare LAN-only vs srflx
> candidate behaviour. Toggle BOTH sides before tapping Start hosting
> / Generate answer.
>
> **Recommended fourth test (in order):**
>
> 4. **MacBook + iPad both connected to the iPhone's hotspot.** The
>    iPhone's personal hotspot is a guaranteed-clean LAN with no AP
>    isolation. If both devices land on the iPhone's WiFi and the
>    `LanOnly` test (no STUN) succeeds, the spike's RTC pipe is
>    confirmed and the home WiFi router is the only remaining problem.
> 5. **Same network, but tick "Use Google STUN" on BOTH pages** before
>    Start hosting. Compares LAN-only vs srflx-augmented behaviour.
>    If this works on the home WiFi where LAN-only didn't, that's
>    extra evidence for AP isolation (and it gives us an option C-lite
>    fallback: ship a STUN-using mode for users on hostile networks,
>    even though it requires brief internet during pairing).
> 6. **Disable AP Isolation on the home router** (Xiaomi: WiFi →
>    Advanced → "AP isolation"). Re-run Test 2 — if it now succeeds,
>    we've identified + fixed the root cause and can recommend it as
>    a setup step for users.
>
> Net effect on Phase 0 gate: **the underlying WebRTC + mDNS pipe is
> validated** (Test 1). What remains unproven is whether typical home
> WiFi routers will let it work end-to-end. If the answer is "no for a
> meaningful fraction of users", we'll need to either tell users to
> reconfigure their routers (poor UX) or fall back to Approach C
> (signalling endpoint that helps both peers learn each other's IPs
> without relying on mDNS).

> **2026-05-12 spike fourth-run findings (root cause confirmed)**:
>
> Test 4 — **MacBook + iPad both joined to iPhone personal hotspot,
> LanOnly mode**: ✅ `State: Connected, ICE state: Connected, gather
> done at 169 ms, DataChannel opened at 17078 ms (the 17s is the
> user copy/paste round-trip, not actual ICE checking time)`. With
> hotspot, browsers gathered TWO mDNS candidates per peer (one per
> network interface) instead of just one, and the connection opened
> as soon as the answer was accepted. SDP grew slightly to 788 bytes
> with the extra candidate — still well within QR v25 alphanumeric
> capacity for Phase 5.
>
> Test 5 — **Same iPhone hotspot but with "Use Google STUN" enabled**:
> ❌ Stuck at `State: Gathering`, no SDP ever generated.
> `wait_for_ice_complete` hung indefinitely because
> `stun.l.google.com` is unreachable from mainland China (GFW). Two
> follow-up fixes:
>
> 1. **Add `ICE_GATHER_TIMEOUT_MS = 5000` cap** to
>    `wait_for_ice_complete` — if a configured STUN server is
>    unreachable, give up after 5 seconds and produce SDP with
>    whatever candidates have arrived. No more permanent hangs.
> 2. **Replace single Google STUN with a multi-server list** that
>    includes CN-reachable servers as the first entries:
>    `stun.miwifi.com:3478` (Xiaomi), `stun.qq.com:3478` (Tencent),
>    `stun.cloudflare.com:3478` (Cloudflare), with Google STUN as
>    the last fallback. Browsers race them and use whichever responds
>    first.
>
> Test 6 — **Mi WiFi router AP-isolation toggle hunt**: ❌ no toggle
> found. Inspected 高级設置 (QoS / DDNS / 端口转发 / VPN / 其他) and
> 常用設置 (Wi-Fi / 上网 / 安全中心 / 局域网 / 系统状态) — none
> exposes an AP isolation / Client Isolation / Wireless Isolation
> control. Xiaomi MiWiFi consumer firmware (1.0.168) appears to block
> peer-to-peer multicast bridging unconditionally without a user-facing
> override. The hotspot test (Test 4) confirms this empirically —
> identical devices, only the network changed, and LAN P2P went from
> failing to working. Updated `pitfalls/webrtc-mdns-lan-ap-isolation.md`
> to drop the "look for the AP isolation toggle" advice for Xiaomi
> users; the realistic mitigation is "use a phone hotspot or flash
> OpenWRT".

> **2026-05-12 spike fifth-finding (corrected diagnosis — AP isolation
> is NOT the cause)**: deeper investigation with `tcpdump -i en0 -n udp
> port 5353` and `dns-sd -B` proved the Xiaomi AX9000 router DOES bridge
> mDNS multicast across LAN devices fine. Captured live mDNS traffic on
> Mac from other devices: `_miio._udp.local.` from a Mi IoT device
> (192.168.31.63 lumi-acpartner-v2 air-conditioner companion),
> `_airplay._tcp.local.` + `_raop._tcp.local.` TXT records from a
> 192.168.31.188 iPad Pro AirPlay receiver, IPv6 mDNS on `ff02::fb` too.
> All three SSIDs (different per-band names) bridge to one `br-lan`.
> AirPlay handshake (which IS mDNS-driven service discovery + UDP P2P)
> works between the same Mac and iPad pair that fail at WebRTC.
>
> So the failure mode is **narrower** than "router blocks mDNS" — it's
> something specific to WebRTC's `<uuid>.local` registration /
> resolution path. Best current hypotheses (none verified):
>
> - WebRTC mDNS hostnames are dynamically registered per
>   ICE-gathering pass; possible timing race between SDP send and
>   responder advertisement
> - AWDL interference on Apple devices (`awdl0` virtual interface);
>   Apple↔Apple `<uuid>.local` queries may prefer AWDL which doesn't
>   bridge through the router the same way as `en0`
> - Mi router's `miotrelay` (畅快连) daemon is ON and bridges Mi IoT
>   discovery; possibly rewrites or filters non-Mi `<uuid>.local`
>   traffic in ways that confuse browsers (no SSH access to the router
>   means we can't inspect what it actually does)
>
> Practical implications:
>
> - **The mitigation menu in the pitfall doc still applies as-is**
>   (iPhone hotspot works → use it; STUN-with-CN-friendly-list as
>   fallback; Approach C signalling helper for hostile networks).
> - **Don't recommend "flash OpenWRT" to users** — until we identify
>   the actual narrow cause, that may not even fix it.
> - The Phase 0 gate decision still holds: build Phases 3-5 with
>   iPhone hotspot as the documented fallback, reserve Approach C
>   (LAN signalling endpoint) for users on networks where WebRTC
>   mDNS resolution is broken.
> - Future investigation could try: (a) installing OpenWRT on a
>   spare Xiaomi to A/B-test mDNS bridging; (b) packet-capturing
>   mDNS during a WebRTC handshake to see whether the queries are
>   sent at all and whether the responder ever advertises the
>   `<uuid>.local`; (c) testing on Linux↔Linux to take Apple's AWDL
>   out of the picture.

> **Phase 0 gate decision**: spike succeeds on the technical merits
> (Tests 1 + 4 prove the LAN-only WebRTC pipe + mDNS works perfectly
> when the network cooperates, ~660-790 byte SDP fits QR v25
> comfortably, iOS Safari ↔ macOS Safari interop works once the router
> isn't sabotaging multicast). **But ~1 hostile-router data point is
> enough to make Approach B alone uncomfortable for production** —
> we'd be telling Chinese users with Xiaomi routers (a large fraction
> of the target audience) that the feature works "except on your
> network, with no fix available". Recommendation for Phase 3+:
>
> - Build the WebRTC transport (Phases 3-5) as planned, with iPhone
>   hotspot as the documented "if it doesn't work, try this" remedy.
> - **Reserve Approach C (chess-net `/signal/<token>` LAN signalling
>   endpoint) as a P3 follow-up** that activates when the
>   DataChannel-open timer fires without progress, transparently
>   falling back to a self-hosted relay. The `room` module from
>   Phase 1 is already transport-agnostic, so the same `Room::apply`
>   loop drives whichever transport completes the handshake first.
> - Skip Approach D (mic permission for raw IPs) entirely — UX cost
>   too high.

#### How to run the spike on real devices

Prerequisite: every test device must be able to reach the dev page. iOS
Safari requires WebRTC over a secure context (HTTPS or `localhost`); on
the LAN that means **either**:

- Serve via `make serve-web-static` from the host laptop, then deploy the
  static dist to GitHub Pages and visit `https://<user>.github.io/Chinese-Chess_Xiangqi/spike/lan/host`
  on each device. Slow iteration but no certificate work.
- **Recommended for fast iteration**: use [`mkcert`](https://github.com/FiloSottile/mkcert)
  to generate a LAN trust cert and run Trunk with `--tls`:
  ```
  mkcert -install
  cd clients/chess-web
  mkcert 192.168.x.y                     # your laptop's LAN IP
  trunk serve --tls-cert ./192.168.x.y.pem --tls-key ./192.168.x.y-key.pem --address 0.0.0.0
  ```
  Then `https://192.168.x.y:8080/spike/lan/host` on the host phone,
  `/spike/lan/join` on the joiner. iOS will warn about the self-signed
  cert; tap "advanced" → trust manually.

Run order:

1. **Host phone** opens `/spike/lan/host`. Tap "1. Start hosting". The
   page generates an offer SDP; copy the entire textarea contents.
2. **Joiner phone** opens `/spike/lan/join`. Paste the host's SDP into
   the first textarea. Tap "2. Generate answer". The page runs the
   answer flow and writes the answer SDP into the second textarea.
3. Copy the answer back to the host phone (AirDrop / Nearby Share /
   typed). Paste into the host's bottom textarea, tap "3. Accept answer".
4. Both diag panels should flip to `state: Connected`,
   `ICE state: "Connected"`. Type messages on either side; they should
   appear on the other.

What to record in this doc afterwards:

- ICE state progression on each side (`new → checking → connected → completed`?
  or `new → checking → failed`?).
- SDP raw byte count from the diag panel — both directions.
- DataChannel `open` time relative to the offer-generated time.
- Browser version + iOS / iPadOS / macOS version per device.
- Any console errors (open Web Inspector via Mac `Develop` menu).

### Phase 1 — Extract `chess-net::room` (1–2 days, INDEPENDENT REFACTOR)

This is the only phase that touches existing shipped code; it's also
the highest-value single chunk because the resulting module makes
chess-net easier to test and unblocks every other transport.

New file: `crates/chess-net/src/room.rs`

- Move from `server.rs`:
  - `RoomState` (struct)
  - `process_client_msg`
  - `process_move`
  - `process_chat`
  - `process_rematch`
  - `broadcast_update`
  - `broadcast_to_all`
  - `send_to`
  - `now_ms`
  - chat constants (`MAX_CHAT_LEN`, `CHAT_HISTORY_CAP`)
- Generalise the seat → outbound mpsc coupling: introduce `PeerId(u32)`
  and `enum Recipient { Peer(PeerId), AllSeats, AllPeers }`.
- Replace direct `mpsc::UnboundedSender<ServerMsg>` storage with
  `PeerId`. The server-side outer code keeps a `HashMap<PeerId, Sender>`
  and translates `Recipient` → fan-out.
- Expose:
  ```rust
  pub struct Room { /* ex-RoomState, no senders inside */ }
  pub struct Outbound { pub recipient: Recipient, pub msg: ServerMsg }
  impl Room {
      pub fn new(rules, password, hints_allowed) -> Self;
      pub fn join_player(&mut self, peer: PeerId) -> Result<(Side, Vec<Outbound>), JoinError>;
      pub fn join_spectator(&mut self, peer: PeerId, max: usize) -> Result<Vec<Outbound>, JoinError>;
      pub fn leave(&mut self, peer: PeerId) -> Vec<Outbound>;
      pub fn apply(&mut self, peer: PeerId, msg: ClientMsg) -> Vec<Outbound>;
      pub fn summary(&self, id: &str) -> RoomSummary;
  }
  ```
- Module is **`#![cfg_attr(not(feature = "server"), no_std?)]`** — actually
  no, `Room` uses `HashMap`/`VecDeque` so `std` is required, but it does
  not depend on `tokio`/`axum`/`futures-util`. Cargo gate it under a
  cheap default feature `"room"` (always on, including for wasm32
  consumers).

Refactor: `server.rs::handle_room_socket` becomes thin glue:

- `let peer = PeerId(next_id())`.
- Insert into `HashMap<PeerId, mpsc::UnboundedSender<ServerMsg>>`.
- For each WS frame: deserialize → `room.apply(peer, msg)` → fan out
  via the routing table.
- On disconnect: `room.leave(peer)`; remove from routing table.

### Phase 2 — Transport trait in chess-web (0.5 day)

New `clients/chess-web/src/transport/{mod.rs, ws.rs}`:

```rust
pub trait Transport {
    fn send(&self, msg: ClientMsg) -> bool;
    fn incoming(&self) -> ReadSignal<Option<ServerMsg>>;
    fn state(&self) -> ReadSignal<ConnState>;
}
```

Move existing `clients/chess-web/src/ws.rs` content into
`transport/ws.rs` as `WsTransport`, implementing the trait. Keep
`ws.rs` as a thin `pub use transport::ws::*;` for one PR; delete in
Phase 3.

`pages/play.rs` and `pages/lobby.rs` consume `Rc<dyn Transport>`
(or `Box<dyn Transport>`) instead of the concrete `WsHandle`.

### Phase 3 — WebRTC client transport (1–2 days)

New `clients/chess-web/src/transport/webrtc.rs`:

- `RtcPeerConnection` configured with `iceServers: []`.
- One reliable + ordered `RtcDataChannel` labelled `"chess"`.
- Handshake helper:
  - `pub async fn create_offer() -> (PeerConnection, OfferBlob)` for host.
  - `pub async fn accept_offer(offer: OfferBlob) -> (PeerConnection, AnswerBlob)`
    for joiner.
  - `pub async fn accept_answer(pc: &PeerConnection, answer: AnswerBlob)`
    completes the host side.
- `OfferBlob` / `AnswerBlob` = compressed-then-base64 SDP+ICE strings,
  optimised for QR encoding.
- Implements `Transport` once the DataChannel is open.

### Phase 4 — Host authority + multi-peer routing (1–2 days)

New `clients/chess-web/src/host_room.rs`:

- Owns one `chess_net::room::Room` (the new module from Phase 1).
- Routing table `HashMap<PeerId, RtcDataChannel>`.
- For each new peer:
  - Create `RtcPeerConnection` with a fresh DataChannel.
  - UI renders the offer QR; user taps "Scan answer" to accept the
    joiner's reply.
  - On DataChannel `open`: `room.join_player(peer)` or
    `room.join_spectator(peer, MAX_SPECTATORS)`; fan out the resulting
    `Outbound` list to the matching channels.
- On DataChannel `message`: deserialize `ClientMsg` →
  `room.apply(peer, msg)` → fan out.
- On DataChannel `close`: `room.leave(peer)`; broadcast updated summary.
- Spectator cap = 4 (configurable in URL: `?max-spectators=2`).

### Phase 5 — `/lan/host` + `/lan/join` pages with QR (1–2 days)

**Status: shipped 2026-05-13** (commit chain through `d8017fe`).
End-to-end pairing + move sync verified via `playwright-cli`
two-tab automation: host opens room → joiner pastes offer →
generates answer → host pastes + accepts → both flip to play view
→ host moves cannon h2→h4 → joiner sees `Black 黑 to move` and
the cannon position update. v1 ships with textareas + Copy
buttons (no QR yet — QR generation/scan deferred to Phase 5.5).

Three intermediate pitfalls discovered + documented during this
phase, all in the chess-web/transport boundary:

* `pitfalls/leptos-rwsignal-queue-self-clear-race.md` — drain-then-
  clear on a Leptos signal queue silently drops concurrent pushes.
  Fixed by `transport::Incoming` (VecDeque + monotonic tick).
* `pitfalls/leptos-create-effect-inside-spawn-local-silent-gc.md`
  — `create_effect` called inside a `spawn_local` future has no
  Leptos owner; the effect is silently GC'd and never re-runs on
  signal change. Fixed by holder-pattern at component scope.
* `pitfalls/webrtc-set-remote-description-resolves-before-dc-open.md`
  — `accept_answer().await` resolves as soon as
  `setRemoteDescription` returns, BEFORE the SCTP handshake
  completes. Calling `dc.send_with_str(Hello)` immediately
  silently fails. Fixed by `transport::webrtc::wait_for_dc_open`
  helper that polls `ready_state()` with a 10 s timeout.

Routes added to `clients/chess-web/src/routes.rs`:

- `/lan/host` — variant + rules picker → "Open room" → renders offer
  QR → "Scan friend's answer" button (camera) → on success, navigate to
  in-progress play view. Sidebar: "+ Invite spectator" button repeats
  the offer/answer cycle.
- `/lan/join` — first screen: "Player" / "Spectator" radio. Second
  screen: open camera, scan host's offer QR. Generate answer, render
  answer QR for host to scan. On open: navigate to play view (read-only
  for spectators).
- `/lan/play` — same play UI as today, but `Transport` is the
  WebRTC one.

QR libraries:

- **Render**: [`qrcode`](https://crates.io/crates/qrcode) crate (pure
  Rust, wasm-friendly, SVG output). No JS dep, lives in our existing
  `cargo` graph.
- **Scan**: TBD by spike result. Default: import jsQR via
  `wasm-bindgen` (~50 KB JS, well-known). Alt: `rqrr` if the spike
  shows acceptable performance.

### Phase 6 — Polish + docs (0.5–1 day)

- `OfflineIndicator`: new states `LAN-host-active`, `LAN-peer-connected`,
  `LAN-disconnected`.
- DataChannel `close` → toast: "Player disconnected" / "Lost connection
  to host".
- Host tab close: `beforeunload` confirm "Closing this tab will end the
  LAN room. Continue?".
- `docs/pwa.md`: new section "LAN play (no server)" with screenshots
  and the install prerequisite.
- `pitfalls/`: anything weird from spike.
- `TODO.md`: promote this entry to Done; open follow-up TODOs:
  - 3-player mesh (blocked on 三國暗棋 itself shipping)
  - Spectator cap raise + QR refresh strategy
  - Optional PIN code anti-mis-scan
  - Approach C signalling endpoint for cross-LAN if anyone asks

## Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| iOS Safari rejects mDNS-only ICE | Medium | Phase B fails; fall back to Approach C | **Phase 0 spike gates everything** |
| SDP blob too large for QR | Low-Medium | Need multi-frame QR or relax STUN constraint | Spike measures real bytes; QR v40 alphanumeric holds 4296 chars |
| Camera autofocus too slow on dense QR | Medium | Joiner gives up | Render QR at high contrast; large module size; spike on real phone |
| User loads PWA only over HTTP (no HTTPS) | Low | `getUserMedia` blocked; no service worker | Documented prereq; install banner on initial visit |
| Host tab closes mid-game | High | Room ends abruptly | UI warning + `beforeunload` confirm |
| Three-player banqi requested early | Low | Mesh complexity 3× | Explicitly out-of-scope v1; tracked separately |
| Phase 1 refactor breaks existing chess-net tests | Medium | CI red | Keep wire protocol identical; existing integration tests act as the safety net; refactor in small PRs |
| WebRTC `negotiationneeded` event timing on iOS | Medium | Inconsistent connect | Spike covers; document in pitfalls if hit |

## Open questions / follow-ups

- **PIN code as anti-mis-scan**: Phase 6 polish? Two QRs in a busy room
  could accidentally pair the wrong people. A 4-digit PIN displayed on
  host + typed by joiner before scan would prevent that. Defer to v2.
- **Reconnect after host tab refresh**: today the room is gone. Could
  persist `Room` to IndexedDB and resume on host page reload, but RTC
  peers see a fresh PeerConnection so they'd need to re-pair anyway.
  Defer.
- **Spectator cap of 4** is arbitrary. Likely fine for friend groups; if
  classroom-scale spectating is requested, switch to "host announces
  one shared QR, multiple spectators scan it" (requires SDP rotation).
- **Three-player mesh** for 三國暗棋 needs either (a) full mesh of 3
  PeerConnections, or (b) host as relay. Track once 三國暗棋 itself
  ships from `backlog/three-kingdoms-banqi.md`.
- **chess-tui parity**: the TUI client cannot be a WebRTC peer (no JS
  runtime). Either it stays online-only, or we ship a tiny native
  signalling helper for it. Defer.
- **Cross-LAN fallback**: if user demand emerges, bring back Approach C
  (chess-net `/signal/<token>` endpoint that does nothing but forward
  SDP blobs). Code-wise it's a 50-line addition once the WebRTC
  transport exists.

## Related

- `crates/chess-net/src/server.rs` — current `RoomState` + `handle_room_socket`;
  source for the Phase 1 extraction.
- `crates/chess-net/src/protocol.rs` — wire schema (unchanged).
- `clients/chess-web/src/ws.rs` — current single-transport pump; moves to
  `transport/ws.rs` in Phase 2.
- `clients/chess-web/src/pages/{play.rs,lobby.rs}` — consumers that switch
  to `dyn Transport`.
- `docs/pwa.md` — install prereq; gets a "LAN play" section in Phase 6.
- `docs/adr/0005-multi-room-lobby.md` / `0006-chess-net-spectators-chat.md`
  — informs the `Room` API extracted in Phase 1.
- `TODO.md` — the entry being upgraded from `[?/M]` to `[P2/L]`.
- `backlog/web-ws-reconnect.md` — adjacent (both want session-token-ish
  semantics; reconnect is harder for P2P).
- `backlog/three-kingdoms-banqi.md` — blocks the 3-player mesh follow-up.
