# WebRTC mDNS `.local` candidates fail to connect across LAN devices

## Verbatim symptom

Two browsers on the same WiFi network exchange WebRTC SDP successfully
(`signalingState: stable` on both sides) but the connection never opens:

- Host shows `iceConnectionState: Checking → Failed` (or `Disconnected`).
- DataChannel `onopen` never fires.
- No console errors — ICE just times out silently.

Each peer's SDP contains exactly one host candidate with a `.local`
mDNS hostname:

```
a=candidate:1922153879 1 udp 2113937151 134f414a-126d-4995-be11-f07d6ec0b725.local 59843 typ host generation 0 network-cost 999
```

Hits both Safari and Chrome on iOS / iPadOS; macOS Safari ↔ macOS Safari
in the same machine works fine. So this is a network-path issue, not
browser-specific.

**Confirmed reproduction** (2026-05-12): macOS Safari host + iPad
Safari/Chrome joiner on a Xiaomi MiWiFi router (firmware 1.0.168, subnet
192.168.31.x) → Disconnected. **Same two devices on the iPhone's
personal hotspot → Connected in <1 second.** The router is the variable.

## Root cause

Modern browsers (Chrome ≥ 76, Safari ≥ 13.x) hide LAN IP addresses
behind randomised `<uuid>.local` mDNS hostnames in WebRTC ICE candidates
as a fingerprinting-prevention measure
([Chrome announcement](https://groups.google.com/g/discuss-webrtc/c/6stQXi72BEU)).
Each peer must resolve the OTHER's `.local` hostname via mDNS multicast
(UDP 5353 to 224.0.0.251) before it can attempt connectivity.

Many home WiFi routers — most notoriously Xiaomi MiWiFi consumer
firmware — block client-to-client multicast bridging without ever
exposing a toggle. The classic culprit name is "AP Isolation" /
"Client Isolation" / "Wireless Isolation" but Xiaomi's web UI doesn't
surface it under any name; the behaviour is just baked in. iPhone
personal hotspots, by contrast, bridge multicast freely.

Common offenders:

- **Xiaomi / Mi Router firmware**: 192.168.31.x default subnet, no
  exposed AP-isolation toggle anywhere in 高级設置 / 常用設置 /
  Wi-Fi 設置. Confirmed 2026-05-12.
- **Public / hotel / conference WiFi**: almost universally isolated.
- Most "guest network" SSIDs on enterprise / mesh routers
- Some mesh router setups split clients across radios (2.4 GHz vs 5 GHz)
  in a way that breaks multicast bridging

## Workarounds

### A. Switch to a phone hotspot (works immediately)

If both devices can connect to the *same* iPhone / Android personal
hotspot, that's a guaranteed-clean LAN with full multicast bridging.
Confirmed working 2026-05-12 with the chess-web Phase 0 spike (LanOnly
mode, no STUN).

This is also the recommended workaround for friends visiting your home
who can't reconfigure your router: one of you turns on personal
hotspot and the other joins it.

### B. Use a router that exposes peer-to-peer multicast

OpenWRT, Asus Merlin firmware, OPNsense, mikroTik, and most prosumer
routers either bridge mDNS by default or have an explicit toggle. If
the user has admin access and the will to flash, OpenWRT on a Xiaomi
router is well-documented.

### C. Add STUN (diagnostic only — breaks "LAN-only at runtime")

Pass `iceServers: [{...}]` with one or more STUN servers to
`RtcPeerConnection`. Browsers then publish `srflx` (server-reflexive)
candidates with the public IP as seen by the STUN server. Two peers
behind the same NAT can sometimes reach each other via NAT hairpin,
though it's not guaranteed.

**STUN-server choice matters in mainland China**: `stun.l.google.com`
is on Google infrastructure and is **blocked by the GFW**. The
browser's gathering loop hangs waiting for a response that never
arrives — `iceGatheringState` sits at `gathering` forever. The
`clients/chess-web/src/spike/rtc.rs` configuration uses a multi-server
list including `stun.miwifi.com:3478`, `stun.qq.com:3478`, and
`stun.cloudflare.com:3478` first so CN-network users get a working
STUN. **The spike's `wait_for_ice_complete` also caps the wait at 5
seconds** so a dead STUN server can't hang the page indefinitely (see
the `ICE_GATHER_TIMEOUT_MS` constant).

This is purely diagnostic for the spike — production wants pure LAN
with no external STUN dependency. If STUN works but `iceServers: []`
doesn't, the router is the culprit; address per (A) or (B).

### D. Get `getUserMedia` permission to expose real LAN IPs

Granting microphone / camera permission to a page disables the mDNS
hostname obfuscation (browsers reason: "if you got mic permission you
already have the user's trust, so the small extra IP-leak is moot").
Real `host` candidates with the LAN IP appear instead of `.local`.

This works but is **wildly user-hostile** — asking for mic permission
just to start a chess game is a UX nonstarter. Filed for completeness;
not a real fix.

### E. Approach C from `backlog/webrtc-lan-pairing.md` — LAN signalling helper

If neither (A) nor (B) is achievable for end users (and our test data
suggests the very common Xiaomi router silently breaks (A) with no fix
short of swapping routers), fall back to a small chess-net endpoint
that brokers the SDP exchange + candidate forwarding. This defeats the
"no server" promise but works on any network. Phase 5 follow-up if the
spike reveals AP isolation is too common in the wild.

## Prevention

`backlog/webrtc-lan-pairing.md` Phase 0 spike findings explicitly call
out the `Disconnected` / `Failed` ICE-connection-state symptoms so the
next agent debugging this knows to:

1. Try the iPhone-hotspot workaround first (15-second triage that
   confirms or kills the router-blocking-mDNS hypothesis).
2. Check that any STUN diagnostic uses CN-reachable servers
   (`stun.miwifi.com` / `stun.qq.com` / `stun.cloudflare.com` are in
   the spike's default list; Google STUN is blocked from CN).
3. Recognise that "no AP-isolation toggle visible" doesn't mean the
   router isn't blocking peer-to-peer multicast — Xiaomi routers do
   it without a toggle.

The `lan_echo.rs` warning banner mentions both the iOS-backgrounding
gotcha (separate issue) and points users at the iPhone-hotspot
workaround if connection fails.
