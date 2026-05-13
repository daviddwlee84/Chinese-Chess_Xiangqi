//! QR code rendering for LAN-pairing SDP envelopes (Phase 5.5).
//!
//! Renders a `&str` payload as an inline SVG QR code via the
//! [`qrcode`](https://crates.io/crates/qrcode) crate. The SVG is
//! embedded into the page via `inner_html` so it scales to whatever
//! container size the parent layout gives it (CSS controls the
//! visual size, not the QR module count).
//!
//! Used by `pages/lan.rs` to render:
//! - Host's offer SDP (joiner scans).
//! - Joiner's answer SDP (host scans).
//!
//! ## Capacity sanity
//!
//! The Phase 5 SDP envelopes measured ~660 bytes. With auto-version
//! selection at ECC level M (15% damage tolerance), a 660-byte
//! payload encodes as QR v25 (byte mode capacity 718 bytes — fits
//! with headroom). If the payload grows (e.g. STUN srflx candidates
//! adding ~80 bytes per server), v30 (1085 bytes byte-mode capacity)
//! kicks in automatically. Beyond v30 the QR becomes too dense for
//! a phone camera at arm's length — see
//! `backlog/webrtc-lan-pairing-qr.md` for the capacity table.
//!
//! ## Why ECC level M (medium)
//!
//! Trade-off between damage tolerance (~15% restorable) and density
//! (more modules → smaller per-module size at fixed CSS dimensions).
//! Higher levels (Q = 25%, H = 30%) push the payload into a higher
//! QR version which makes scanning harder on small phone screens.
//! M is the standard sweet spot for screen-displayed QRs.

use leptos::*;
use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};

/// Render `payload` as an SVG QR code at the given pixel size.
///
/// Returns `None` if `payload.is_empty()` (don't render anything for
/// empty input — caller's `Show` should already be gating this) or if
/// the QR encoder fails (would only happen for payloads larger than
/// QR version 40 byte-mode max ~2.9 KB; unreachable for valid SDPs).
fn encode_svg(payload: &str, min_dim: u32) -> Option<String> {
    if payload.is_empty() {
        return None;
    }
    let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M).ok()?;
    let svg = code
        .render()
        .min_dimensions(min_dim, min_dim)
        .dark_color(svg::Color("#000"))
        .light_color(svg::Color("#fff"))
        .quiet_zone(true)
        .build();
    Some(svg)
}

/// Inline-SVG QR code for `payload`. Renders at `min_dim` pixels
/// minimum (the actual size grows with QR version + CSS).
///
/// Wrap in CSS that gives the SVG a definite size — the QR scales
/// to fit its parent. Recommended container: 280×280 CSS pixels
/// minimum so a phone camera at arm's length can decode it
/// reliably.
#[component]
pub fn QrCodeView(
    /// SDP envelope (or any string) to encode.
    payload: Signal<String>,
    /// Minimum SVG dimension in pixels. The QR encoder picks the
    /// smallest version that fits the payload; the SVG is then
    /// scaled to this minimum. Keep ≥ 200 px for readable scanning.
    #[prop(default = 280)]
    min_dim: u32,
    /// Optional aria-label for screen readers; shows as alt text.
    #[prop(default = "QR code")]
    label: &'static str,
) -> impl IntoView {
    let svg = move || encode_svg(&payload.get(), min_dim);
    view! {
        <figure
            class="qr-card"
            role="img"
            aria-label=label
            style="margin:0;display:flex;flex-direction:column;align-items:center;gap:0.5rem"
        >
            {move || match svg() {
                Some(s) => view! { <div class="qr-svg" inner_html=s /> }.into_view(),
                None => view! {
                    <div class="qr-placeholder" style="width:280px;height:280px;background:#222;color:#888;display:flex;align-items:center;justify-content:center;font-size:14px">
                        "(no payload yet)"
                    </div>
                }.into_view(),
            }}
        </figure>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_empty_returns_none() {
        assert!(encode_svg("", 200).is_none());
    }

    #[test]
    fn encode_small_payload_returns_svg() {
        let svg = encode_svg("hello world", 200).expect("small payload encodes");
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"), "got: {}", &svg[..50]);
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn encode_typical_sdp_size_fits() {
        // Synthesize a payload around the size of a real LAN-pairing
        // SDP envelope (~660 bytes). Should encode without error.
        let payload = "x".repeat(660);
        let svg = encode_svg(&payload, 280).expect("660-byte payload should fit");
        assert!(svg.contains("</svg>"));
    }
}
