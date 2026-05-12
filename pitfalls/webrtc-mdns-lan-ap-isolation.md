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

## Root cause

Modern browsers (Chrome ≥ 76, Safari ≥ 13.x) hide LAN IP addresses
behind randomised `<uuid>.local` mDNS hostnames in WebRTC ICE candidates
as a fingerprinting-prevention measure
([Chrome announcement](https://groups.google.com/g/discuss-webrtc/c/6stQXi72BEU)).
Each peer must resolve the OTHER's `.local` hostname via mDNS multicast
(UDP 5353 to 224.0.0.251) before it can attempt connectivity.

Many home WiFi routers — especially those configured for "AP Isolation"
/ "Client Isolation" / "Guest network" / "Wireless Isolation" — block
client-to-client multicast and even unicast traffic. With mDNS resolution
broken, peers can't translate `.local → IP`, no candidate pair can be
formed, and ICE fails with no actionable error.

Common offenders:

- Default config on Xiaomi / Mi Router (192.168.31.x subnet) firmware
- Most "公共WiFi" / public hotspots, hotel WiFi, conference WiFi
- Any guest network on enterprise / mesh routers
- Some mesh router setups split clients across radios (2.4 GHz vs 5 GHz)
  in a way that breaks multicast bridging

## Workarounds

### A. Fix the network (preferred for chess-web LAN play)

Disable AP Isolation / Client Isolation in the router admin UI. On
Xiaomi: Settings → WiFi → Advanced → "AP isolation". On TP-Link / ASUS:
look for "Wireless Isolation" or "AP isolation" in the WLAN advanced
panel.

After disabling: same SDP exchange will succeed, ICE pairs the `.local`
candidates, DataChannel opens within ~1 second.

### B. Use a phone hotspot for the test

If both devices can connect to the *same* iPhone / Android hotspot,
that's a guaranteed-clean LAN with no AP isolation. Useful triage to
prove the rest of the WebRTC pipe works while you debug the router.

### C. Add STUN (diagnostic only — breaks "LAN-only at runtime")

Pass `iceServers: [{ urls: 'stun:stun.l.google.com:19302' }]` to
`RtcPeerConnection`. Browsers then publish `srflx` (server-reflexive)
candidates with the public IP as seen by the STUN server. Two peers
behind the same NAT can sometimes reach each other via NAT hairpin,
though it's not guaranteed.

The `clients/chess-web/src/spike/lan_echo.rs` page has a "Use Google
STUN" checkbox that flips this for the spike. **Enable it on BOTH
sides before generating offer / answer** — they must agree.

This is purely diagnostic — production wants pure LAN with no external
STUN dependency. If STUN works but `iceServers: []` doesn't, the router
is the culprit; fix per (A).

### D. Get `getUserMedia` permission to expose real LAN IPs

Granting microphone / camera permission to a page disables the mDNS
hostname obfuscation (browsers reason: "if you got mic permission you
already have the user's trust, so the small extra IP-leak is moot").
Real `host` candidates with the LAN IP appear instead of `.local`.

This works but is **wildly user-hostile** — asking for mic permission
just to start a chess game is a UX nonstarter. Filed for completeness;
not a real fix.

### E. Approach C from `backlog/webrtc-lan-pairing.md` — LAN signalling helper

If neither (A) nor (B) is achievable for end users (not all friends will
disable AP isolation on their routers), fall back to a small chess-net
endpoint that brokers the SDP exchange + candidate forwarding. This
defeats the "no server" promise but works on any network. Phase 5
follow-up if the spike reveals AP isolation is too common in the wild.

## Prevention

`backlog/webrtc-lan-pairing.md` Phase 0 spike findings now call out the
`Disconnected` / `Failed` ICE-connection-state symptoms explicitly so
the next agent debugging this knows to check the router first instead
of chasing browser-side bugs. The `lan_echo.rs` warning banner will
mention AP isolation as a likely cause once the spike concludes.
