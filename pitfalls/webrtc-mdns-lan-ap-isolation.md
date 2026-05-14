# WebRTC mDNS `.local` candidates fail to connect across LAN devices

(file slug kept as `webrtc-mdns-lan-ap-isolation.md` for cross-link
stability; original "AP isolation" hypothesis was wrong, see Root cause
section below for the corrected story.)

## Verbatim symptom

Two browsers on the same WiFi network exchange WebRTC SDP successfully
(`signalingState: stable` on both sides) but the connection never opens:

- Host shows `iceConnectionState: Checking → Failed` (or `Disconnected`).
- DataChannel `onopen` never fires.
- No console errors — ICE just times out silently.

Each peer's SDP contains exactly one host candidate with a `<uuid>.local`
mDNS hostname:

```
a=candidate:1922153879 1 udp 2113937151 134f414a-126d-4995-be11-f07d6ec0b725.local 59843 typ host generation 0 network-cost 999
```

Hits both Safari and Chrome on iOS / iPadOS; macOS Safari ↔ macOS Safari
in the same machine works fine. So this is a network-path issue, not
browser-specific.

**Confirmed reproduction** (2026-05-12): macOS Safari host + iPad
Safari/Chrome joiner on a Xiaomi AX9000 (firmware MiWiFi 1.0.168 CN,
subnet 192.168.31.x) → Disconnected. **Same two devices on the iPhone's
personal hotspot → Connected in <1 second.** The router is the variable.

## Root cause

Modern browsers (Chrome ≥ 76, Safari ≥ 13.x) hide LAN IP addresses
behind randomised `<uuid>.local` mDNS hostnames in WebRTC ICE candidates
as a fingerprinting-prevention measure
([Chrome announcement](https://groups.google.com/g/discuss-webrtc/c/6stQXi72BEU)).
Each peer must resolve the OTHER's `<uuid>.local` hostname via mDNS
(UDP 5353 / 224.0.0.251) before it can attempt connectivity.

**The original guess on this network was "router blocks mDNS multicast"
— that turned out to be WRONG, see "What we ruled out" below.** The
real failure mode appears to be more subtle and is still under
investigation. The symptom + workaround menu still apply, but the root
cause of *why* the iPhone hotspot succeeds where the home WiFi fails is
not "AP isolation".

## What we ruled out (and the evidence)

Hypothesis | Status | Evidence
---|---|---
**"Router does AP / Client / Wireless Isolation"** | ❌ disproven | Mac's `tcpdump -i en0 -n udp port 5353` captures live mDNS traffic from other LAN devices: `_miio._udp.local.` from Mi IoT (192.168.31.63 lumi-acpartner-v2), `_airplay._tcp.local.` + `_raop._tcp.local.` TXT records from a 192.168.31.188 iPad Pro AirPlay receiver, IPv6 mDNS on `ff02::fb` too. Multicast bridges fine across the router's 2.4G/5G/5G-Game radios into one `br-lan`. `dns-sd -B _services._dns-sd._udp local.` discovers cross-device services normally.
**"Router blocks all multicast"** | ❌ disproven | Same evidence — multicast clearly works.
**"Devices on different VLANs / SSIDs can't see each other"** | ❌ disproven | All three SSIDs (different names per band) bridge to the same `br-lan`. iPad ↔ Mac AirPlay handshake works, which is itself mDNS-driven service discovery + UDP P2P.
**"Xiaomi router has a hidden AP-isolation toggle in admin UI"** | ❌ disproven | Inspected every page in 高级設置 (QoS / DDNS / 端口转发 / VPN / 其他) and 常用設置 (Wi-Fi / 上网 / 安全中心 / 局域网 / 系统状态). Only mDNS-relevant toggles surfaced are `miotrelay` (Mi IoT relay) and `miscan` (5G AIoT scan), both ON. No AP-isolation control. Stock firmware; SSH/Telnet disabled (no shell access to inspect `ebtables` / `brctl` / wireless driver multicast-snooping state).

## What's actually happening (best current hypothesis)

General mDNS service discovery (`_airplay._tcp`, `_miio._udp`, `_raop._tcp`)
works on this LAN — confirmed by tcpdump and `dns-sd`. WebRTC's
`<uuid>.local` resolution specifically does NOT work between Mac and
iPad. The differences between general mDNS service discovery and
WebRTC's hostname resolution that could matter:

1. **WebRTC mDNS hostnames are dynamically registered per session.**
   Browsers generate a fresh UUID per ICE-gathering pass, register it
   with the OS-level mDNS responder (Bonjour on macOS, similar on iOS),
   and unregister it when the PeerConnection closes. There's a small
   window between "hostname appears in SDP and is sent to peer" and
   "responder advertises it on the network". On a busy / Mi-relayed
   network the timing might be fragile.
2. **AWDL interference.** Apple devices use AWDL (`awdl0` on the Mac)
   for direct Apple↔Apple P2P (AirDrop, AirPlay etc.). The Mac's mDNS
   responder advertises on both `awdl0` and `en0`. WebRTC's mDNS
   resolution may prefer `awdl0` for `<uuid>.local` queries from another
   Apple device, and AWDL may not bridge through the router the same
   way regular Wi-Fi does. Possibly relevant: hotspot LAN doesn't have
   the same AWDL ambiguity (the iPhone IS the gateway).
3. **`network-cost: 999`** in candidates suggests browsers tag this
   network as "expensive" — possibly affecting candidate priority but
   shouldn't kill the connection.
4. **Mi router `miotrelay` (畅快连)** — Mi's own multicast relay
   daemon. It bridges Mi IoT discovery across radios, but its semantics
   for non-Mi `<uuid>.local` traffic are undocumented; it may rewrite
   or filter packets in ways that confuse browser mDNS responders.
   No SSH access means we can't inspect what it actually does.
5. **Double NAT layer** at 192.168.1.1 → 192.168.31.1 shouldn't matter
   (mDNS is link-local) but adds another variable.

This is the part the spike's data points have NOT yet isolated. The
practical mitigation menu still works whatever the actual cause is.

## Sibling failure mode — VPN tunnel on either device (confirmed 2026-05-14)

A separate but visually identical failure: an active VPN client on
*either* device — Cloudflare WARP, NordVPN, ExpressVPN, corporate
ZTNA agents, the iOS / iPadOS "VPN" toggle, etc. — silently masks
the OS-level LAN routing. iOS specifically allocates an `198.18.0.0/15`
address (RFC 2544 Benchmark range, treated as a synthetic loopback by
the VPN extension) and reports *that* to WebRTC as the host candidate
instead of the real WiFi IP. Same symptom as mDNS resolution failure
— ICE sits at `checking` and times out — but a different cause:

- **No mDNS issue at all**: even disabling mDNS obfuscation wouldn't
  help, because the LAN IP the browser would emit is also a VPN
  tunnel address that the peer cannot reach.
- **STUN doesn't rescue it**: STUN srflx gets the public IP, but both
  peers see *the same VPN exit IP*, and the VPN provider's NAT
  doesn't hairpin between two clients of the same exit.
- **Personal hotspot doesn't rescue it either**: if the iPad/iPhone
  is on a VPN, even the device's own hotspot routes peer traffic
  through the VPN. Confirmed: tested on iPhone hotspot with iPad VPN
  ON → still failed; turned VPN OFF → succeeded in <1 second.

### How to spot it

Open `/lan/host`, watch the new ICE diag badge under `Status:`. If the
host candidate IP visible in the offer SDP (or any candidate in the
joiner's answer) falls in `198.18.0.0/15`, that's iOS's VPN tunnel
address. `100.64.0.0/10` indicates CGNAT (different fix — needs TURN
or `/lobby`). `clients/chess-web/src/net_diag.rs::classify` does this
parsing; the LAN page surfaces the diagnosis automatically in the
10-s DC-open-timeout error.

### Mitigation

Disable the VPN on **both** devices before pairing. If the VPN must
stay on (corporate policy etc.), configure split-tunnel so the
following ranges bypass the tunnel:

- `192.168.0.0/16`, `10.0.0.0/8`, `172.16.0.0/12` — private LANs
- `172.20.10.0/28` — iOS personal hotspot range
- `224.0.0.0/4` — IPv4 multicast (covers mDNS)
- `fe80::/10` + `ff00::/8` — IPv6 link-local + multicast

Not every VPN client exposes split-tunnel; the user-visible hint on
`/lan/host` and `/lan/join` calls this out so users don't blame the
chess app for what's actually their VPN.

## Workarounds

### A. Switch to a phone hotspot (works immediately, confirmed 2026-05-12)

If both devices can connect to the *same* iPhone / Android personal
hotspot, that's a guaranteed-clean LAN. Confirmed working with the
chess-web Phase 0 spike (LanOnly mode, no STUN) — DataChannel opens
within 1 second of accepting the answer.

This is also the recommended workaround for friends visiting your
home who can't reconfigure your router: one of you turns on personal
hotspot and the other joins it.

### B. Add STUN with CN-reachable servers (diagnostic / fallback)

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
list including `stun.miwifi.com:3478` (Xiaomi), `stun.qq.com:3478`
(Tencent), `stun.cloudflare.com:3478` (Cloudflare), with Google STUN
as last resort. **The spike's `wait_for_ice_complete` also caps the
wait at 5 seconds** so a dead STUN server can't hang the page (see
the `ICE_GATHER_TIMEOUT_MS` constant).

### C. Get `getUserMedia` permission to expose real LAN IPs

Granting microphone / camera permission to a page disables the mDNS
hostname obfuscation (browsers reason: "if you got mic permission you
already have the user's trust, so the small extra IP-leak is moot").
Real `host` candidates with the LAN IP appear instead of `.local`.

This works but is **wildly user-hostile** — asking for mic permission
just to start a chess game is a UX nonstarter. Filed for completeness;
not a real fix.

### D. Approach C from `backlog/webrtc-lan-pairing.md` — LAN signalling helper

For users on networks where (A) is impractical and (B) doesn't help
enough, fall back to a small chess-net endpoint that brokers the SDP
exchange + candidate forwarding. This defeats the "no server"
promise but works on any network. Phase 5 follow-up if the spike
reveals the WebRTC-mDNS failure mode is too common in the wild.

### E. Use a router that exposes peer-to-peer multicast / WebRTC mDNS

OpenWRT, Asus Merlin firmware, OPNsense, mikroTik, and most prosumer
routers either bridge mDNS by default or have explicit toggles. If
the user has admin access and the will to flash, OpenWRT on a Xiaomi
router is well-documented. **However, given that general mDNS
already works on this Xiaomi stock firmware**, this might NOT actually
fix the WebRTC-specific symptom — the failure mode here is something
narrower than "router blocks multicast". Test before committing to a
flash.

## Prevention

`backlog/webrtc-lan-pairing.md` Phase 0 spike findings explicitly call
out the `Disconnected` / `Failed` ICE-connection-state symptoms so the
next agent debugging this knows to:

1. **Try the iPhone-hotspot workaround first** (15-second triage that
   confirms or kills the network-blocking-WebRTC-mDNS hypothesis).
2. **Don't assume "router blocks mDNS"** without tcpdump evidence —
   on the AX9000 reproduction, general mDNS clearly works (Mi IoT,
   AirPlay TXT records all visible) but WebRTC's `<uuid>.local`
   resolution still fails. The failure mode is something narrower
   than "no multicast at all".
3. Check that any STUN diagnostic uses CN-reachable servers
   (`stun.miwifi.com` / `stun.qq.com` / `stun.cloudflare.com` are in
   the spike's default list; Google STUN is blocked from CN).
4. Recognise that "no AP-isolation toggle visible" doesn't tell you
   anything either way — Xiaomi MiWiFi consumer firmware doesn't
   expose one even when peer-to-peer multicast IS bridging fine.

The `lan_echo.rs` warning banner mentions both the iOS-backgrounding
gotcha (separate issue) and points users at the iPhone-hotspot
workaround if connection fails.

## Useful diagnostic commands (macOS)

```sh
# Live mDNS browse on en0 — should show cross-device services on a healthy LAN
dns-sd -B _services._dns-sd._udp local.

# Resolve a specific .local hostname to IP — should succeed for any device
# advertising itself; will TIMEOUT (not error) if the responder isn't reachable
dns-sd -G v4 some-uuid.local

# Capture all mDNS traffic on en0 for ~30s — count cross-device packets
sudo tcpdump -i en0 -n udp port 5353 -G 30 -W 1

# Capture IPv6 mDNS too
sudo tcpdump -i en0 'ip6 and udp port 5353' -G 30 -W 1
```

For Xiaomi MiWiFi routers the user maintains a read-only LuCI JSON API
CLI at `~/bin/mi-router` (`mi-router info / wifi / devices / mdns / raw
<endpoint>`); useful for inspecting router state without SSH access.
