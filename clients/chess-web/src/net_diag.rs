//! Tiny pure-string helpers that look at a WebRTC SDP and classify the
//! host candidates by network range — so the LAN host page can surface
//! "your VPN is hijacking the LAN" instead of just "network blocked?"
//! when pairing times out.
//!
//! Lives outside `transport/webrtc.rs` so it's native-testable; the
//! actual `RtcPeerConnection` plumbing is wasm32-only but the SDP it
//! produces is just a string.

/// Extract the `connection-address` field from every `a=candidate:` line
/// in `sdp`. SDP `a=candidate` format (RFC 5245 §15):
///
/// ```text
/// a=candidate:<foundation> <component> <transport> <priority>
///             <connection-address> <port> typ <type> [...]
/// ```
///
/// Field 5 (0-indexed 4 after stripping `a=candidate:`) is the address
/// we want. mDNS hostnames look like `<uuid>.local`; srflx / relay
/// have real IPs; host candidates on a VPN may have tunnel addresses.
pub fn parse_candidate_addrs(sdp: &str) -> Vec<String> {
    sdp.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("a=candidate:")?;
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 6 {
                return None;
            }
            Some(parts[4].to_string())
        })
        .collect()
}

/// `true` if `ip` is in `198.18.0.0/15` — RFC 2544 benchmark range,
/// also used by iOS as the fake LAN address when a VPN profile is
/// active (Cloudflare WARP, NordVPN, etc.). A WebRTC `typ host`
/// candidate landing in this range almost always means the OS is
/// routing LAN through a VPN tunnel.
pub fn is_vpn_tunnel_ip(ip: &str) -> bool {
    ip_in_prefix(ip, &[198, 18], 15)
}

/// `true` if `ip` is in `100.64.0.0/10` — RFC 6598 CGNAT range used
/// by mobile carriers and some ISPs for Carrier-Grade NAT. Direct P2P
/// often fails on CGNAT because the carrier's outer NAT doesn't
/// hairpin.
pub fn is_cgnat_ip(ip: &str) -> bool {
    let Some((100, rest)) = parse_v4_first(ip) else {
        return false;
    };
    (64..=127).contains(&rest)
}

/// Classify a parsed SDP's candidate addresses, picking the most-
/// specific hint we can give the user.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NetDiag {
    /// Host candidate is in `198.18.0.0/15` — VPN is hijacking LAN.
    VpnTunnel,
    /// Host candidate is in `100.64.0.0/10` — ISP is using CGNAT.
    Cgnat,
    /// Nothing distinguishing; fall through to the generic hint.
    Plain,
}

/// Walk the candidate addresses and return the first specific class
/// detected, or `Plain` if none. VPN takes precedence over CGNAT
/// because they suggest different fixes (disable VPN vs. switch
/// network), and VPN's more common.
pub fn classify(addrs: &[String]) -> NetDiag {
    if addrs.iter().any(|a| is_vpn_tunnel_ip(a)) {
        return NetDiag::VpnTunnel;
    }
    if addrs.iter().any(|a| is_cgnat_ip(a)) {
        return NetDiag::Cgnat;
    }
    NetDiag::Plain
}

// --- private helpers ----------------------------------------------------

fn parse_v4_first(ip: &str) -> Option<(u8, u8)> {
    let mut parts = ip.split('.');
    let a = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    Some((a, b))
}

/// Match `ip` against an IPv4 prefix expressed as `first_two_octets` +
/// prefix length in bits. Only used for the 198.18.0.0/15 check today,
/// where prefix=15 means the first octet matches exactly and the
/// second octet's high 7 bits match (so 198.18.x.x AND 198.19.x.x
/// qualify).
fn ip_in_prefix(ip: &str, first_two: &[u8; 2], prefix_bits: u32) -> bool {
    let Some((a, b)) = parse_v4_first(ip) else {
        return false;
    };
    if a != first_two[0] {
        return false;
    }
    // /15 means the second octet's high 7 bits must match. Compute the
    // mask: prefix 15 → 8 bits of first octet + 7 bits of second.
    let second_bits = prefix_bits.saturating_sub(8);
    if second_bits == 0 {
        return true; // /8 — first octet enough
    }
    let mask = 0xFFu8 << (8 - second_bits);
    (b & mask) == (first_two[1] & mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extracts_address_field_from_candidate_line() {
        let sdp = "v=0\r\n\
                   a=candidate:1234 1 udp 2113937151 192.168.1.5 51234 typ host\r\n\
                   a=candidate:5678 1 udp 2113937151 abc-def.local 51235 typ host\r\n";
        assert_eq!(
            parse_candidate_addrs(sdp),
            vec!["192.168.1.5".to_string(), "abc-def.local".to_string()],
        );
    }

    #[test]
    fn parse_ignores_non_candidate_lines_and_short_candidates() {
        let sdp = "v=0\r\n\
                   a=group:BUNDLE 0\r\n\
                   a=candidate:short 1 udp\r\n";
        assert!(parse_candidate_addrs(sdp).is_empty());
    }

    #[test]
    fn vpn_tunnel_detects_198_18_and_198_19() {
        assert!(is_vpn_tunnel_ip("198.18.0.1"));
        assert!(is_vpn_tunnel_ip("198.18.255.255"));
        assert!(is_vpn_tunnel_ip("198.19.0.1"));
        assert!(is_vpn_tunnel_ip("198.19.255.255"));
    }

    #[test]
    fn vpn_tunnel_rejects_neighbours_and_unrelated() {
        assert!(!is_vpn_tunnel_ip("198.17.0.1"));
        assert!(!is_vpn_tunnel_ip("198.20.0.1"));
        assert!(!is_vpn_tunnel_ip("192.168.1.5"));
        assert!(!is_vpn_tunnel_ip("abc.local"));
        assert!(!is_vpn_tunnel_ip(""));
    }

    #[test]
    fn cgnat_detects_100_64_through_100_127() {
        assert!(is_cgnat_ip("100.64.0.1"));
        assert!(is_cgnat_ip("100.127.255.255"));
    }

    #[test]
    fn cgnat_rejects_100_63_and_100_128() {
        assert!(!is_cgnat_ip("100.63.255.255"));
        assert!(!is_cgnat_ip("100.128.0.1"));
        assert!(!is_cgnat_ip("100.0.0.1"));
    }

    #[test]
    fn classify_picks_vpn_over_cgnat_over_plain() {
        let vpn_then_cgnat = vec!["198.18.0.1".into(), "100.64.0.1".into()];
        assert_eq!(classify(&vpn_then_cgnat), NetDiag::VpnTunnel);

        let cgnat_only = vec!["100.64.0.1".into(), "abc.local".into()];
        assert_eq!(classify(&cgnat_only), NetDiag::Cgnat);

        let normal = vec!["192.168.1.5".into(), "abc.local".into()];
        assert_eq!(classify(&normal), NetDiag::Plain);

        assert_eq!(classify(&[]), NetDiag::Plain);
    }
}
