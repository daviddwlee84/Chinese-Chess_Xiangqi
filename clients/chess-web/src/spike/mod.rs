//! Phase 0 spike for `backlog/webrtc-lan-pairing.md`.
//!
//! This entire module is **throwaway code**. Its only job is to validate
//! two unknowns on real iOS / iPadOS / macOS Safari hardware before
//! committing to Phases 3–6:
//!
//! 1. **Does iOS Safari + mDNS-only ICE actually open a peer-to-peer
//!    DataChannel between two devices on the same WiFi?** WebRTC docs
//!    promise it; reality is sometimes different.
//! 2. **How big is the SDP-with-ICE-candidates blob in practice?** This
//!    decides whether QR encoding (Phase 5) is feasible or whether we
//!    need multi-frame QR / a tiny LAN signalling helper.
//!
//! What we deliberately punt to Phase 5: QR generation + camera
//! scanning. The spike uses textareas — host produces base64 SDP, user
//! pastes it via AirDrop / Nearby Share / SMS / typing into the joiner's
//! textarea, joiner produces answer base64, user pastes back. Once the
//! DataChannel opens, both sides exchange echo messages.
//!
//! Routes (only mounted in debug / Trunk-dev builds; production build
//! ships them anyway because the spike has zero security surface — it's
//! just pure-client RTC plumbing — and chess-web has no auth model
//! either way):
//!
//!   /spike/lan/host  — start a room, copy offer SDP, paste answer SDP
//!   /spike/lan/join  — paste host's offer, copy our answer

pub mod lan_echo;
pub mod rtc;
