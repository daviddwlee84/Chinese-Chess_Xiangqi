# LAN multiplayer over WebRTC — chess-web takeaway

This is the long-form companion to `backlog/webrtc-lan-pairing.md` and
`pitfalls/webrtc-*.md` / `pitfalls/leptos-*.md` — the recipe you re-read
in a year when you want to add LAN multiplayer to ANOTHER game built on
the same `core / net / web` skeleton.

Architecture decisions, the build flow, every trap we hit, and a step-by-
step porting checklist for re-using this on a new game. If you only have
five minutes, read the [TL;DR](#tldr) and [Porting recipe](#porting-recipe).

---

<a id="tldr"></a>
## TL;DR

- **Two browsers on the same WiFi play a full multiplayer session over
  `RtcDataChannel` with NO server in the loop at runtime**. One browser
  acts as the "host" and runs the authoritative game state machine
  in-WASM; the other is the "joiner" that connects via WebRTC.
- **Wire protocol is unchanged from the existing chess-net WS path**.
  The host's in-WASM `Room` is the same code that the native chess-net
  server runs (`crates/chess-net/src/room.rs`). Pages
  (`pages/play.rs`, `pages/lobby.rs`) don't know which transport
  they're talking to — they consume `transport::Session { handle,
  incoming, state }` regardless.
- **Pairing is on-device manual SDP exchange** (offer + answer over
  textareas + Copy buttons; QR generation is the next-step polish, see
  [Future work](#future-work)).
- **Five distinct pitfalls were discovered + documented** during Phase
  0–5 implementation (4 Leptos / WebRTC timing races + 1 mDNS / router
  AP-isolation issue). All five are individually grep-able under
  `pitfalls/` — see [Pitfalls index](#pitfalls-index). Each one is a
  *class* of bug that will recur on any similar transport boundary.
- **End-to-end testing via `playwright-cli`** (two tabs in one
  Chromium, scripted offer/answer dance + chat + move sync) is what
  finally caught the last race. *Do this before pushing anything that
  touches the transport boundary*. See [Testing loop](#testing-loop).

---

<a id="when-to-use"></a>
## When to use this pattern (and when not)

The fit-for-purpose checklist before copying this:

| Constraint | This pattern fits | This pattern doesn't fit |
|---|---|---|
| Players physically together | ✅ same WiFi / hotspot | ❌ across continents (need TURN relay) |
| Server cost | ✅ zero (no chess-net needed at runtime) | n/a |
| Session length | ✅ minutes to hours (1 chess game) | ❌ multi-day persistence (host tab close = room ends) |
| Player count | ✅ 1v1 + ~4 spectators | ⚠️ 8+ peers needs mesh routing or full server |
| State authority | ✅ one peer is "host" + owns truth | ❌ truly decentralised consensus |
| Discovery | ⚠️ manual SDP exchange (or QR) | ❌ no auto-discovery without a relay |
| Reconnect | ❌ host tab close = game over | n/a |
| Network requirements | ✅ same LAN, mDNS works (most home WiFi) | ❌ AP-isolated networks (some routers, public WiFi) |
| Re-pair a saved peer | ❌ every session needs fresh SDP | n/a |

**The killer feature**: zero external dependency at runtime. Once both
PWAs are installed (one HTTPS visit each), the game works in
airplane-mode-with-WiFi: train, classroom, park, anywhere with shared
WiFi or a hotspot.

**The biggest UX cost**: every session starts with an SDP exchange
(~660 bytes JSON). Manual copy-paste is clunky; QR is much better but
still requires camera aiming. There's no "saved peer = instant
reconnect" — WebRTC SDPs are session-specific.

**Same-LAN gotcha**: some routers (verified: Mi AX9000) silently drop
WebRTC's mDNS `<uuid>.local` resolution while letting general mDNS
through (Bonjour, AirPlay). See
[`pitfalls/webrtc-mdns-lan-ap-isolation.md`](../pitfalls/webrtc-mdns-lan-ap-isolation.md).
The STUN-fallback toggle in the UI papers over this when present, but
needs reachable STUN servers (which the GFW blocks for
`stun.l.google.com` — we ship a multi-server list ordered for CN
reachability).

---

<a id="architecture-overview"></a>
## Architecture in one diagram

```
                       ┌──────────────────────────┐
                       │   chess-core (no IO)     │
                       │   GameState · RuleSet    │
                       └────────────┬─────────────┘
                                    │ used by
                       ┌────────────┴─────────────┐
                       │   chess-net::room::Room  │  ← transport-agnostic
                       │   apply(peer, ClientMsg) │     state machine
                       │     -> Vec<Outbound>     │
                       └─┬─────────────────────┬──┘
            used by      │                     │      used by
           ┌─────────────┘                     └──────────────┐
           ▼                                                  ▼
  ┌────────────────────┐                          ┌──────────────────────┐
  │ chess-net server   │                          │  chess-web HostRoom  │
  │ (native, axum/ws)  │                          │  (in-WASM, browser)  │
  │                    │                          │                      │
  │ WS transport       │                          │ WebRTC transport     │
  │ socket per peer    │                          │ DataChannel per peer │
  └─────────┬──────────┘                          └──────────┬───────────┘
            │                                                │
            │            same wire protocol                  │
            │       (chess_net::protocol::ClientMsg /        │
            │              ServerMsg, v6)                    │
            │                                                │
  ┌─────────▼──────────┐                          ┌──────────▼───────────┐
  │   chess-web page   │                          │   chess-web page     │
  │ (chess-net mode)   │                          │ (LAN host or joiner) │
  │                    │                          │                      │
  │ pages/play.rs      │  ←── same component ──→  │ pages/play.rs        │
  │                    │      same Session         │                      │
  │ transport::ws::    │                          │ transport::webrtc::  │
  │   connect(url)     │                          │   connect_as_*()     │
  └────────────────────┘                          └──────────────────────┘
```

**The two pillars**:

1. **`chess-net::room::Room`** is wire-protocol-aware but transport-
   agnostic. It takes `(PeerId, ClientMsg)` and returns
   `Vec<Outbound { peer: PeerId, msg: ServerMsg }>`. The server side
   wraps it in a tokio task per WS connection; the LAN host side wraps
   it in a Leptos `Rc<HostRoom>` that owns a `HashMap<PeerId,
   PeerSink>` (Local for the host's own play page, Remote for joiner's
   DataChannel).

2. **`chess-web::transport::Session { handle, incoming, state }`** is
   the page-level abstraction. `handle: Rc<dyn Transport>` for
   sending; `incoming: Incoming` for receiving (queue + tick signal
   pair, see [Pitfalls index](#pitfalls-index)); `state:
   ReadSignal<ConnState>` for connection status. Pages
   (`pages/play.rs`, `pages/lobby.rs`) consume only `Session` — they
   don't know if it's WS-backed or WebRTC-backed.

**Result**: adding LAN multiplayer to the existing chess-net-mode game
was *zero changes to game UI / page components*. The two new
mountable components (`LanHostPage`, `LanJoinPage`) build a `Session`
and inject it into `PlayConnected` via the new `injected_session`
prop.

---

<a id="full-handshake-walkthrough"></a>
## Full handshake — what actually happens

End-to-end timeline from "user taps Open room" to "both peers see
synchronized chess board". Numbers in parens are file:line refs.

### 1. Host page mount → user taps "Open room"

`pages/lan.rs::LanHostPage::on_open` callback runs:

```rust
let cfg = WebRtcConfig {
    ice_mode: if use_stun { IceMode::WithStun } else { IceMode::LanOnly },
};
spawn_local(async move {
    match connect_as_host(cfg).await {
        Ok(hh) => {
            set_offer_blob.set(hh.offer.0.clone());
            *handshake_slot.borrow_mut() = Some(hh);
            set_status.set(HostStatus::AwaitingAnswer);
        }
        Err(e) => { /* ... */ }
    }
});
```

`transport::webrtc::connect_as_host` (`webrtc.rs:287`):

1. `RtcPeerConnection::new_with_configuration(cfg)` — `iceServers: []`
   for LAN-only, multi-server STUN list for `WithStun` (CN-friendly
   ordering).
2. `pc.create_data_channel_with_data_channel_dict("chess",
   ordered+reliable)` — host always initiates the DC. Joiner
   discovers it via `ondatachannel`.
3. `pc.create_offer().await` → `pc.set_local_description(offer).await`.
4. `wait_for_ice_complete(&pc, 5_000ms)` — block until ICE gathering
   finishes OR timeout (caps the failure mode where one configured
   STUN is unreachable and would otherwise hang forever).
5. Read `pc.local_description().sdp()`, wrap in JSON envelope (`{
   "type": "offer", "sdp": "..." }` with CRLF preserved as `\r\n`
   escapes — survives clipboard/AirDrop/IM normalisation).
6. Return `HostHandshake { pc, dc, offer, state, _keepalive }`.

Page renders the offer SDP in a textarea + Copy button. User
copy/AirDrops it to the joiner.

### 2. Joiner pastes offer → taps "Generate answer"

`pages/lan.rs::LanJoinPage::on_generate` callback runs `connect_as_joiner`
(`webrtc.rs:226`):

1. Same `RtcPeerConnection::new_with_configuration(cfg)`.
2. **Wire `ondatachannel` BEFORE setting remote description** — the
   DC arrives as a side-effect of `setRemoteDescription(offer)` and
   we mustn't miss the event.
3. `pc.set_remote_description(offer).await`.
4. `pc.create_answer().await` → `pc.set_local_description(answer).await`.
5. `wait_for_ice_complete(&pc, 5_000ms)`.
6. Return `JoinerHandshake { session, answer, pc, _keepalive }`.

`session.incoming` is a fresh `Incoming::new()`. The `ondatachannel`
callback (when it fires) calls `install_dc_handlers_for_joiner(dc,
incoming, set_state)` — wires `onmessage` to `incoming.push(msg)`,
`onopen` to `state.set(Open)`, `onclose` to `state.set(Closed)`. AND
defensively checks `dc.ready_state() == Open` immediately after
installing the handler (covers the fast-LAN race where SCTP completes
faster than our Rust closure-wrap chain — see
[`pitfalls/webrtc-set-remote-description-resolves-before-dc-open.md`](../pitfalls/webrtc-set-remote-description-resolves-before-dc-open.md)).

Page renders the answer SDP in a textarea. User copy/AirDrops it back
to the host.

### 3. Host pastes answer → taps "Accept answer"

`pages/lan.rs::LanHostPage::on_accept` callback runs:

```rust
spawn_local(async move {
    let hh = handshake_slot.borrow_mut().take()?;
    if let Err(e) = hh.accept_answer(blob).await { /* error */ }

    // CRITICAL: wait for DC actually open before fanning out Hello.
    // accept_answer resolves on SDP-application; SCTP handshake is async after.
    let dc = hh.dc.borrow().clone()?;
    if !wait_for_dc_open(&dc, 10_000).await {
        return error("DataChannel did not open within 10 s");
    }

    let (room, session) = HostRoom::new(RuleSet::xiangqi(), None, false);
    room.attach_remote_player_dc(dc)?;

    *host_room_slot.borrow_mut() = Some(room);
    set_play_session.set(Some(session));
    set_status.set(HostStatus::Playing);
});
```

`HostRoom::new` (`host_room.rs:148`) does the actual game-state setup:

1. `Room::new(rules, password, hints_allowed)` — fresh transport-
   agnostic state machine.
2. Allocate `PeerId(1)` for host; create `Incoming::new()` for the
   host's local sink.
3. **Insert host's `PeerSink::Local(incoming.clone())` into the
   sinks map BEFORE calling `room.join_player(self_peer)`** —
   otherwise the synchronous Hello + ChatHistory outbounds have
   nowhere to go and get dropped.
4. `room.join_player(PeerId(1))` returns `(Side::RED, Vec<Outbound>)`
   — fanout the outbounds (Hello, ChatHistory) to the matching
   sinks. Host's local incoming queue now has `[Hello, ChatHistory]`.
5. Return `(Rc<HostRoom>, Session { handle: HostSelfTransport,
   incoming, state: Open })`.

`HostRoom::attach_remote_player_dc(dc)` (`host_room.rs:268`):

1. `add_remote_player(PeerSink::Remote(dc.clone()))` → `Room::join_player(PeerId(2))`
   → fanout Hello + ChatHistory for peer 2 to its `PeerSink::Remote`
   → `dc.send_with_str(serialized_hello)`. **DC is open by now**
   thanks to the `wait_for_dc_open` upstream, so the send succeeds.
2. `install_remote_dc_handlers(host, peer, &dc)` — wire DC's
   `onmessage` to `host.handle_remote_msg(peer, msg)` (which calls
   `Room::apply` and fanouts the result), and `onclose` to
   `host.drop_peer(peer)`. Both closures use `Weak<HostRoom>` to
   avoid an Rc cycle with the keepalive vector.

### 4. Both pages render the chess board

LanHostPage's view flips from the offer/answer dance to:

```rust
<PlayConnected
    ws_base=lan_dummy_ws_base()
    room="lan-host".to_string()
    password=None watch_only=false
    debug_enabled=false hints_requested=false
    injected_session=session            // ← THE bridge
    back_link_override="/lan/host".to_string()
/>
```

`PlayConnected` (`pages/play.rs:96`) reads `session.incoming` (the
queue) and `session.state` (Open). Its `create_effect` drains the
queue, processing each `ServerMsg` in arrival order. The first drain
processes `Hello` (sets `role`, `view`, `rules`, `hints_allowed`)
then `ChatHistory` (empty initially). UI shows the board with
"You play Red 紅", "Red 紅 to move", "Connected".

Joiner side: state signal flips to Open via DC.onopen → component-
scope holder effect (`pages/lan.rs::LanJoinPage`) fires → `set_play_session(Some(session))` → Show component
mounts `<PlayConnected injected_session=session ... />`. Same
draining effect processes Hello + ChatHistory. UI shows the board
with "You play Black 黑".

### 5. Move + chat sync

Host moves a piece: `Board` component fires `on_move` → page's
`on_move` callback → `handle.send(ClientMsg::Move { mv })`. Handle is
`HostSelfTransport`:

```rust
impl Transport for HostSelfTransport {
    fn send(&self, msg: ClientMsg) -> bool {
        self.host.handle_self_send(self.self_peer, msg);
        true
    }
}
```

`handle_self_send` → `room.apply(self_peer, ClientMsg::Move)` →
returns `Vec<Outbound>` containing `Update { view: ... }` for both
peers → `fanout` calls `sink.deliver(msg)` for each:

- Host's sink is `PeerSink::Local(incoming)` → `incoming.push(Update)`.
- Joiner's sink is `PeerSink::Remote(dc)` → `dc.send_with_str(serialized_update)`.

Joiner's DC.onmessage handler fires → `incoming.push(Update)` on
joiner's queue. Both pages' draining effects re-fire (their tick
signals incremented). Both view signals update. Both boards re-render.
Sidebars both show "Black 黑 to move".

Joiner moves: `WebRtcTransport::send` → `dc.send_with_str(serialized_move)`
→ host's DC.onmessage handler → `handle_remote_msg(peer, msg)` →
same `Room::apply` + fanout. Host receives the Update via Local sink;
joiner receives it via DC echo. Symmetric.

Chat is identical to Move (same fanout path), just with
`ClientMsg::Chat` / `ServerMsg::Chat`.

---

<a id="file-by-file"></a>
## File-by-file (what each file owns)

The 7 files that make up the LAN multiplayer feature, in dependency
order (lowest first):

| File | LoC | Purpose | Reusable as-is? |
|---|---:|---|---|
| `crates/chess-net/src/room.rs` | ~600 | Transport-agnostic `Room::apply` state machine. Pre-existed for chess-net server; extracted from `server.rs` in Phase 1. | ✅ Yes — any game with a turn-based state machine can mirror this shape. |
| `crates/chess-net/src/protocol.rs` | ~150 | Wire types: `ClientMsg`, `ServerMsg`, `PlayerView`, `RoomSummary`. Unchanged. | ✅ Yes — generic framing shape. |
| `crates/chess-net/Cargo.toml` | — | wasm32-only `js-sys = "0.3"` dep for `Date::now()`. | ✅ Pattern reusable; see `pitfalls/wasm32-systemtime-now-panics.md`. |
| `clients/chess-web/src/transport/mod.rs` | ~165 | `Transport` trait + `Session` + `Incoming` (queue + tick signal). | ✅ Yes — the `Incoming` design is forced by Leptos, applies to any reactive UI talking to async transports. |
| `clients/chess-web/src/transport/ws.rs` | ~95 | WS impl of `Transport`. Pre-existed for chess-net mode; touched only to migrate to `Incoming::push`. | ✅ Pattern only (your project's WS will differ). |
| `clients/chess-web/src/transport/webrtc.rs` | ~575 | WebRTC impl: `connect_as_host`, `connect_as_joiner`, `wait_for_dc_open`, SDP envelope codec, ICE config + gather-timeout. | ✅ **The reusable WebRTC core**. Copy-paste with at most a `RuleSet`/protocol type swap. |
| `clients/chess-web/src/host_room.rs` | ~415 | `HostRoom` — `Rc<Self>`-based multi-peer routing wrapper around `Room`. `PeerSink::{Local, Remote, Mock}`, `attach_remote_player_dc`, `Weak`-captured DC handlers. | ✅ Yes — the `PeerSink` enum + Local/Remote split + `Weak`-cycle-break is the pattern. |
| `clients/chess-web/src/pages/lan.rs` | ~440 | `LanHostPage` + `LanJoinPage` route components. State machines, holder pattern for spawn_local result bridging, textarea-based pairing UI. | ✅ Pattern only (your UI shape will differ). |
| `clients/chess-web/src/pages/play.rs` | ~750 | `PlayConnected` made `pub` and gained `injected_session: Option<Session>` + `back_link_override: Option<String>` props so LAN routes can reuse it unchanged. | ✅ The "inject a Session" pattern is the key — write your play UI to consume Session generically and the LAN/server modes share it. |

Roughly **~2,650 LoC total**, of which **~1,400 LoC is reusable** for
another game on the same skeleton (transport mod + WebRTC mod + room
extraction pattern + page-level Session injection).

---

<a id="testing-loop"></a>
## Testing loop — `playwright-cli` is the secret weapon

The four runtime bugs were ALL invisible without browser console logs
(silent panics, dropped messages, owner-less effects). Real-device
retesting on iPhone/iPad worked but had a 5-min round-trip per fix.
`playwright-cli` two-tab automation reduced this to <30 s per
iteration once the harness was scripted.

### Minimal smoke test (what to run before pushing transport changes)

```sh
# 1. Start trunk serve in one terminal:
cd clients/chess-web
trunk serve --tls-cert-path ./192.168.31.136.pem \
            --tls-key-path ./192.168.31.136-key.pem \
            --address 0.0.0.0 > /tmp/trunk.log 2>&1 &

# 2. Open two tabs (one host, one joiner) in playwright-cli:
playwright-cli open --browser=chrome https://192.168.31.136:8080/lan/host
playwright-cli tab-new https://192.168.31.136:8080/lan/join

# 3. Drive the offer/answer dance (see scripts/test-lan-pairing.sh).
#    Three reusable extracted ops:
#       (a) tab-select 0; click "Open room"; eval read offer textarea value
#       (b) tab-select 1; fill offer; click "Generate answer"; eval read answer
#       (c) tab-select 0; fill answer; click "Accept answer"

# 4. Verify both tabs reach play view (snapshot greps for "to move").

# 5. Send chat from each tab; verify both see both messages.

# 6. Make a piece-move (find cell-hit rect by attribute, click via real
#    mouse OR set an id then click "#cell_h2"); verify joiner sees it.

# 7. Check console for any panics:
playwright-cli console     # surfaces RuntimeError stacks
```

### What broke and how playwright-cli caught it

| Bug | How it surfaced via playwright-cli |
|---|---|
| Hello dropped (queue self-clear) | Joiner snapshot showed `Awaiting seat assignment` despite `Connected` state. |
| Joiner stuck on WaitingForOpen | Joiner snapshot showed `Status: WaitingForOpen` indefinitely; host worked. |
| Hello lost on wire (DC not yet open) | Joiner mounted PlayConnected (sidebar present) but board placeholder showed `Waiting for server greeting…`. |
| Chat panic | Chat input + Send appeared to do nothing; `playwright-cli console` showed `panicked at .../time.rs:31:9: time not implemented on this platform`. |

The fourth bug (the wasm32 SystemTime panic) would have taken hours
to diagnose on a real device without the console-tap. With
`playwright-cli console` it took <30 seconds.

### Adding playwright-cli to your dev loop

For any future networked-game change in this repo, recommend these
steps in the AGENTS.md pre-push checklist:

1. `cargo build --target wasm32-unknown-unknown -p chess-web` (verify
   compile).
2. Restart trunk if running (its incremental rebuild can stall —
   verify the bundle hash actually changed via `curl -ks
   https://localhost:8080/ | grep -o 'chess-web-[a-f0-9]*'`).
3. `playwright-cli` two-tab smoke test above.
4. `playwright-cli console` to scan for panics.
5. Then `git push`.

---

<a id="pitfalls-index"></a>
## Pitfalls index

Five distinct classes of bug, each with a dedicated `pitfalls/<slug>.md`.
Read these *before* writing similar code — at least the headlines.

### Network / WebRTC

1. **[`webrtc-mdns-lan-ap-isolation.md`](../pitfalls/webrtc-mdns-lan-ap-isolation.md)**
   — Some routers (verified: Mi AX9000 firmware 1.0.168) silently
   drop WebRTC's `<uuid>.local` mDNS resolution while letting general
   mDNS through. STUN fallback works around it but needs reachable
   STUN servers.

2. **[`webrtc-set-remote-description-resolves-before-dc-open.md`](../pitfalls/webrtc-set-remote-description-resolves-before-dc-open.md)**
   — `accept_answer().await` resolves on SDP application, BEFORE
   the SCTP handshake completes. Calling `dc.send_with_str(Hello)`
   immediately silently fails (`Err` is discarded by the sink wrapper).
   Fix: poll `dc.ready_state() == Open` before fanning out.

### Leptos signal / reactivity

3. **[`leptos-rwsignal-queue-self-clear-race.md`](../pitfalls/leptos-rwsignal-queue-self-clear-race.md)**
   — A `RwSignal<Vec<T>>` queue with `set(Vec::new())` at end of the
   drain effect has a race: any push between the drain start and the
   clear gets clobbered. Fix: separate the queue (`VecDeque` outside
   reactivity) from the notification (monotonic `u32` tick signal).
   See `transport::Incoming`.

4. **[`leptos-create-effect-inside-spawn-local-silent-gc.md`](../pitfalls/leptos-create-effect-inside-spawn-local-silent-gc.md)**
   — `create_effect` inside a `spawn_local` future has no Leptos owner.
   The effect is silently GC'd at end of the current microtask;
   subsequent `state.set(...)` calls fire but no subscriber exists.
   Fix: hoist the effect to component scope; use a holder signal that
   the spawn_local future `set`s.

### WASM / stdlib

5. **[`wasm32-systemtime-now-panics.md`](../pitfalls/wasm32-systemtime-now-panics.md)**
   — `std::time::SystemTime::now()` panics on `wasm32-unknown-unknown`
   ("time not implemented on this platform"). The panic kills the
   call-handler chain, making the failure mode "an in-browser action
   silently does nothing". Audit any crate that compiles for both
   targets. Fix: cfg-gate to use `js_sys::Date::now()` on wasm32.

### Adjacent (not direct fixes, but informative)

- **[`ios-safari-svg-click-no-tap.md`](../pitfalls/ios-safari-svg-click-no-tap.md)**
  — Pre-existing pitfall about iOS Safari's pointer event model.
  Relevant if you wire taps to SVG `<g>` elements directly.
- **[`leptos-effect-tracking-stale-epoch.md`](../pitfalls/leptos-effect-tracking-stale-epoch.md)**
  — Adjacent Leptos reactivity gotcha; same code path style.
- **[`pwa-base-path-and-stale-cache.md`](../pitfalls/pwa-base-path-and-stale-cache.md)**
  — Important when adding new routes (`/lan/host`, `/lan/join`) to
  a PWA — the service worker precache list must include them.

The first four are the ones you WILL re-trip on if you write a
similar feature without reading them.

---

<a id="porting-recipe"></a>
## Porting recipe — adding LAN multiplayer to a NEW game

Step-by-step checklist for a future you / future agent. Assumes the
target game has the same `core / net / web` workspace shape (pure
state machine + serialisable wire types + Leptos web client).

### Prerequisites

- [ ] **Game state machine is already transport-agnostic.** Wire
      messages flow through a function with shape
      `apply(peer, ClientMsg) -> Vec<Outbound { peer, ServerMsg }>`.
      If your game's server-side code is currently entangled with
      tokio/axum, do this extraction FIRST as its own commit.
- [ ] **Wire protocol is wasm-clean.** No dependencies on
      `std::time`, no `thread::spawn`, nothing that panics on
      `wasm32-unknown-unknown`. Check via
      `cargo check --target wasm32-unknown-unknown -p <yourcrate>`.
- [ ] **Web client uses a transport abstraction.** Pages consume a
      `Session { handle, incoming, state }` shape; the WS
      implementation is one impl of a trait. If pages are tightly
      coupled to gloo-net WS, refactor first.

### Phase 0 — spike the network reachability (0.5–1 day, GATES EVERYTHING)

- [ ] Build a throwaway `/spike/lan/{host,join}` page with a single
      DataChannel and an echo loop. Don't touch your game state
      machine yet — just prove SDP exchange + DC.send/onmessage works
      between two real devices on your target networks.
- [ ] Test on **iOS Safari ↔ another browser** specifically. Apple's
      WebRTC stack has the most edge cases (backgrounding tears down
      RTC, mDNS handling differs).
- [ ] Test on **at least 2 router brands**. We confirmed Mi AX9000
      breaks WebRTC mDNS while keeping general mDNS working;
      hotspot from a phone always works.
- [ ] If LAN-only ICE fails on your test network, add a STUN
      fallback. **Use a multi-server list ordered for your users'
      geography** (we use `stun.miwifi.com`, `stun.qq.com`,
      `stun.cloudflare.com`, `stun.l.google.com` for CN+global
      reach).
- [ ] Decision gate: did pure-LAN work on at least one real-device
      pair? If yes → ship LAN-only as default + STUN as opt-in. If
      no on every network → reconsider whether you need a relay.

### Phase 1 — extract transport-agnostic Room

- [ ] In your `*-net` crate, create `room.rs` exporting a struct
      `Room` with method `apply(peer: PeerId, msg: ClientMsg) ->
      Vec<Outbound>`. Move all the chess-net-server code that
      currently calls `socket.send` into producing `Outbound`s
      instead.
- [ ] Add `PeerId(u64)` newtype + `Outbound { peer, msg }` struct.
- [ ] Native server's old logic becomes:
      `let outbounds = room.apply(peer, msg); for ob in outbounds {
      socket_for(ob.peer).send(ob.msg) }`.
- [ ] Run all your existing integration tests — they should pass
      unchanged. Add unit tests for `Room::apply` directly (no
      socket needed).
- [ ] **Audit `std::time` usage.** Wrap any `SystemTime::now()` /
      `Instant::now()` in cfg-gates: native uses `SystemTime`,
      wasm32 uses `js_sys::Date::now()`. Add wasm32-only `js-sys`
      dep to Cargo.toml. See pitfall #5 above.

### Phase 2 — Transport trait + Session shape on the web side

- [ ] Define `transport::Transport` trait (one method: `send(&self,
      ClientMsg) -> bool`).
- [ ] Define `transport::Session { handle: Rc<dyn Transport>,
      incoming: Incoming, state: ReadSignal<ConnState> }`.
- [ ] Define `transport::Incoming` as a `VecDeque + monotonic
      u32 tick signal` (NOT a `RwSignal<Vec<T>>` — see pitfall #3).
- [ ] Refactor your existing WS code into one impl of `Transport`.
      Pages consume only `Session`; they don't import `gloo-net`.

### Phase 3 — WebRTC transport

- [ ] Copy `clients/chess-web/src/transport/webrtc.rs` as a starting
      template. The reusable bits:
  - `WebRtcConfig { ice_mode: IceMode }`, `IceMode { LanOnly, WithStun }`.
  - `connect_as_host(cfg) -> Result<HostHandshake, JsValue>` —
    creates pc + dc, generates offer SDP, returns the dc + pc
    handles for later `attach_remote_player_dc`.
  - `connect_as_joiner(cfg, offer) -> Result<JoinerHandshake, JsValue>`
    — creates pc, applies offer, generates answer SDP, returns a
    fresh `Session` whose `incoming` is wired to the DC.
  - `wait_for_dc_open(dc, timeout_ms) -> bool` — poll-based gate;
    use it after `accept_answer().await` before fanning out (see
    pitfall #2).
  - `encode_sdp` / `decode_sdp_envelope` — JSON-wraps SDP to
    survive clipboard / AirDrop / IM CRLF normalisation.
  - `wait_for_ice_complete(pc, timeout_ms)` — caps the
    "STUN unreachable" hang.
  - **Defensive `dc.ready_state() == Open` check immediately after
    `set_onopen`** — covers the fast-LAN race.
- [ ] Swap your `ClientMsg` / `ServerMsg` types for the chess-net
      ones in the file. JSON serialisation handles the rest.

### Phase 4 — HostRoom (multi-peer routing wrapper)

- [ ] Copy `clients/chess-web/src/host_room.rs` as a starting
      template. The reusable bits:
  - `PeerSink::{Local(Incoming), Remote(RtcDataChannel),
    Mock(Rc<RefCell<Vec<ServerMsg>>>)}` enum + `deliver(msg)` impl.
  - `HostRoom { room: RefCell<Room>, sinks: RefCell<HashMap<PeerId,
    PeerSink>>, next_peer, self_peer, room_id }`.
  - `HostRoom::new(rules, password, hints) -> (Rc<Self>, Session)` —
    seats host as RED before fanning out, returns `Session` whose
    `incoming` is the host's own `Incoming` queue.
  - `HostSelfTransport { host: Rc<HostRoom>, self_peer }` — `Transport`
    impl that routes the host's own `ClientMsg`s through `Room::apply`
    locally.
  - `attach_remote_player_dc(dc)` — calls `add_remote_player` then
    `install_remote_dc_handlers` (which uses `Weak<HostRoom>` to
    avoid the Rc cycle with the keepalive vector).
  - `handle_remote_msg(peer, msg)` / `fanout(outbound)` /
    `drop_peer(peer)` — the remote-DC `onmessage` / `onclose` paths.

### Phase 5 — Pages

- [ ] Make your existing `PlayConnected` (or equivalent) accept
      `injected_session: Option<Session>` and `back_link_override:
      Option<String>` as `#[prop(optional)]`. Default to opening
      a WS via your factory; if injected, use it as-is.
- [ ] Build `LanHostPage` + `LanJoinPage` route components. The
      state machines:
  - Host: `Idle → Generating → AwaitingAnswer → AcceptingAnswer →
    Playing`. Use a `Show` that flips on `status == Playing` to
    mount `<PlayConnected injected_session=session ... />`.
  - Joiner: `Idle → Generating → WaitingForOpen → Playing`. Use the
    **holder pattern** for the `state == Open` watcher (see pitfall
    #4): create a `state_holder: ReadSignal<Option<ReadSignal<ConnState>>>`
    at component scope; the spawn_local future `set`s it; the
    component-scope effect reads holder + inner state and triggers
    play_session.
- [ ] Wire copy/paste UX (textareas + Copy buttons via
      `navigator.clipboard.writeText`). Add a "Reset" button that
      reloads the page (cleanest way to recover from a broken
      handshake; PeerConnection state cleanup is otherwise gnarly).

### Phase 6 — Testing + polish

- [ ] Set up `playwright-cli` two-tab automation (see [Testing
      loop](#testing-loop)). Drive offer/answer dance, verify both
      tabs reach the play view, send chat both directions, make a
      move on each side, verify sync. Run before every push that
      touches transport.
- [ ] Add `OfflineIndicator` LAN states.
- [ ] `beforeunload` confirm on host tab close ("ending the LAN
      room").
- [ ] DataChannel `onclose` toast: "Player disconnected".
- [ ] Document the install prereq (one-time HTTPS visit so PWA +
      service worker register before going offline).

---

<a id="what-not-to-do"></a>
## What NOT to do (anti-patterns we tried first)

Each of these was tried and reverted during implementation. Listed so
you don't waste a session re-trying them.

### Don't: latched `WriteSignal<Option<ServerMsg>>` for the incoming queue

**Why it seems right**: Leptos signals are reactive; just `set` the
latest message and the page reacts.

**Why it's wrong**: `Room::join_player` synchronously emits Hello +
ChatHistory. Two `set` calls in the same microtask batch into ONE
effect firing reading the LAST value (ChatHistory). Hello is silently
dropped.

**Symptom**: page reaches "Connected" but stays on "Awaiting seat
assignment" forever.

### Don't: `RwSignal<Vec<ServerMsg>>` queue with `set(Vec::new())` clear

**Why it seems right**: store messages in a vec, drain effect reads
+ clears, push appends.

**Why it's wrong**: any push between drain start and the clear is
silently overwritten by the clear. JS event-loop microtask scheduling
makes this race timing-dependent + intermittent.

**Symptom**: messages randomly disappear; some sessions work, others
hang. Hardest of the four to diagnose.

**Right answer**: separate the queue (VecDeque outside reactivity)
from the notification (monotonic tick signal). See `Incoming` in
`transport/mod.rs`.

### Don't: `create_effect` inside a `spawn_local` future

**Why it seems right**: handle the result of an async operation
reactively.

**Why it's wrong**: no Leptos owner = no retention = effect GC'd
immediately. Subscribers never fire.

**Symptom**: `state.set(...)` calls work but no effect reacts. Page
state never updates.

**Right answer**: hoist the effect to component scope; bridge with a
holder signal that the spawn_local future `set`s.

### Don't: send via `dc.send_with_str` immediately after `accept_answer().await`

**Why it seems right**: `await` returned, so the DC must be ready,
right?

**Why it's wrong**: `accept_answer` resolves on SDP application; the
SCTP handshake is async after that. `dc.ready_state()` is still
"connecting". `send_with_str` returns Err which the sink wrapper
discards.

**Symptom**: Hello/ChatHistory silently lost on the wire. Joiner
mounts PlayConnected (sidebar shows "Connected") but board placeholder
stays on "Waiting for server greeting…" forever.

**Right answer**: `wait_for_dc_open(dc, 10_000).await` between
`accept_answer` and `attach_remote_player_dc`.

### Don't: rely on `std::time::SystemTime` in code that compiles for wasm32

**Why it seems right**: it's stdlib, it must be portable.

**Why it's wrong**: `wasm32-unknown-unknown` provides a stdlib stub
that PANICS with "time not implemented on this platform". The panic
unwinds through your call chain (often a click handler) and your
action silently fails.

**Symptom**: in-browser action does nothing; browser console shows
`RuntimeError: unreachable` with a Rust trace ending in
`std::time::SystemTime::now`.

**Right answer**: cfg-gate every `SystemTime` / `Instant` call.
`js_sys::Date::now()` for wallclock; `web_sys::Performance::now()`
for monotonic.

### Don't: tie native + wasm32 cargo deps together

**Why it seems right**: simpler `Cargo.toml`.

**Why it's wrong**: pulls `js-sys`, `web-sys`, etc. into your
native build, slowing it down and adding unnecessary attack
surface.

**Right answer**:
```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
js-sys = "0.3"
```

### Don't: write `pages/lan.rs` from scratch

**Why it seems right**: it's just two new pages, how hard can it be?

**Why it's wrong**: the 4 pitfalls above are subtle enough that the
first 4 attempts will probably re-trip on at least one of them. Use
the existing `pages/lan.rs` as your template; it has all the workarounds
baked in.

---

<a id="future-work"></a>
## Future work — what's not yet done

### Phase 5.5 — QR pairing — SHIPPED 2026-05-13

**Now in production**. Both `/lan/host` and `/lan/join` render an
SVG QR (via the `qrcode` crate) of the SDP envelope alongside the
existing copy-text textarea. A "📷 Scan camera" button — visible
only when `camera::has_camera()` returns true — opens a full-screen
modal that uses `getUserMedia` + `requestAnimationFrame` + jsQR to
decode the other peer's QR. On success, the textarea auto-fills and
the next-step action (Generate answer / Accept answer) auto-fires.

**Textarea + Copy is preserved as a co-equal alternative** so PCs
without webcams, denied-permission cases, and async pairing
(printout, screenshot via IM) all still work. See
[`backlog/webrtc-lan-pairing-qr.md`](../backlog/webrtc-lan-pairing-qr.md)
for the full six-commit design.

Verified end-to-end via `scripts/test-lan-pairing.sh` — drives the
text-paste path through playwright-cli (camera path is real-device
only since playwright-cli's headless Chrome has no usable webcam).

### Phase 6 — UI polish (still open)

Tracked in `TODO.md` under the `[L] WebRTC LAN pairing — Phase 6
polish` entry.

Remaining pain points:

- **No spectator UI yet**. The host code path supports up to 4
  spectator DCs but there's no "Add spectator" button on the host
  page (`attach_remote_spectator_dc` exists, just unused from the UI
  layer).
- **No `beforeunload` confirm on host tab close**. Accidentally
  closing the host tab kills the game with no warning.
- **No DC-disconnect toast**. If the joiner's DC closes mid-game,
  the host sees nothing.
- **No `OfflineIndicator` LAN states**. The existing online indicator
  doesn't differentiate "LAN-host-active" / "LAN-peer-connected" /
  "LAN-disconnected".
- **No `docs/pwa.md` "LAN play" section** with screenshots + the
  install prerequisite.

### Out of scope for v1 (and likely v2)

- **Cross-LAN (TURN relay)** — would need a server. The whole point
  of v1 is "no server at runtime". If demand emerges, the original
  backlog has Approach C (chess-net `/signal/<token>` endpoint that
  forwards SDPs without knowing about chess) as a 50-line addition.
- **Three-player mesh for 三國暗棋** — needs full mesh of 3
  PeerConnections OR host-as-relay. Tracked separately; depends on
  the variant itself shipping first
  (`backlog/three-kingdoms-banqi.md`).
- **Reconnect after host tab refresh** — RTC peers see a fresh
  PeerConnection on reload, so they'd need to re-pair anyway. Not
  worth the persistence complexity for v1.
- **chess-tui as a WebRTC peer** — no JS runtime in the TUI. Either
  it stays online-only or we ship a tiny native signalling helper.
  Defer.

---

## Related

- [`backlog/webrtc-lan-pairing.md`](../backlog/webrtc-lan-pairing.md)
  — full Phase 0-6 design rationale, alternative approaches
  considered, real-device test logs.
- [`docs/architecture.md`](architecture.md) — workspace shape that
  this feature plugs into.
- [`docs/pwa.md`](pwa.md) — install prereq + service worker (gets a
  "LAN play" section in Phase 6).
- [`pitfalls/README.md`](../pitfalls/README.md) — index of all
  pitfalls; the five LAN-related ones are tagged in the body of this
  doc.
- ADR-0005 (`docs/adr/0005-multi-room-lobby.md`) and ADR-0006
  (`docs/adr/0006-chess-net-spectators-chat.md`) — the chess-net
  protocol decisions that the host-in-WASM Room reuses unchanged.

