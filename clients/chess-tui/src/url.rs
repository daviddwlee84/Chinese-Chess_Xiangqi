//! URL helpers for the host-prompt / lobby / connect flows.
//!
//! All hand-rolled — the entire URL space we care about is `ws://host:port`
//! plus an optional `/ws/<id>?password=…` suffix, so pulling in the `url`
//! crate isn't worth the dep.

/// Normalize a free-text host string into a `ws://host:port` form (no path).
/// Examples:
///   "127.0.0.1:7878"   → "ws://127.0.0.1:7878"
///   "ws://127.0.0.1"   → "ws://127.0.0.1"
///   "ws://host:7878/ws"→ "ws://host:7878"   (trailing path stripped)
pub fn normalize_host_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("Empty host. Try ws://127.0.0.1:7878".into());
    }
    let with_scheme = if raw.starts_with("ws://") || raw.starts_with("wss://") {
        raw.to_string()
    } else if raw.starts_with("http://") {
        format!("ws://{}", raw.trim_start_matches("http://"))
    } else if raw.starts_with("https://") {
        format!("wss://{}", raw.trim_start_matches("https://"))
    } else {
        format!("ws://{raw}")
    };
    // Strip any path / query so we can re-append `/lobby` or `/ws/<id>` cleanly.
    let scheme_end = with_scheme.find("://").map(|i| i + 3).ok_or("missing scheme")?;
    let rest = &with_scheme[scheme_end..];
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    if rest[..host_end].is_empty() {
        return Err("Empty host after scheme.".into());
    }
    Ok(format!("{}{}", &with_scheme[..scheme_end], &rest[..host_end]))
}

/// Append `/lobby` if the URL doesn't already end in one of the chess-net
/// path conventions.
pub fn normalize_lobby_url(raw: &str) -> Result<String, String> {
    let host = normalize_host_url(raw)?;
    Ok(format!("{host}/lobby"))
}

/// Auto-append `/ws` for the bare-host shortcut, like the legacy
/// `--connect ws://host:port`. Anything containing `/ws` or `/lobby` is
/// preserved as-is.
pub fn normalize_connect_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.contains("/ws") || raw.contains("/lobby") {
        return Ok(raw.to_string());
    }
    let host = normalize_host_url(raw)?;
    Ok(format!("{host}/ws"))
}

/// Minimal percent-encoding for the `password=` query value. Encodes the
/// reserved chars that would break URL parsing; spaces become `%20`.
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Validate a room id matches the server-side regex `^[a-zA-Z0-9_-]{1,32}$`.
pub fn valid_room_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_url_adds_scheme() {
        assert_eq!(normalize_host_url("127.0.0.1:7878").unwrap(), "ws://127.0.0.1:7878");
    }

    #[test]
    fn host_url_strips_path() {
        assert_eq!(normalize_host_url("ws://h:1/ws/foo").unwrap(), "ws://h:1");
    }

    #[test]
    fn http_to_ws() {
        assert_eq!(normalize_host_url("http://host").unwrap(), "ws://host");
        assert_eq!(normalize_host_url("https://host").unwrap(), "wss://host");
    }

    #[test]
    fn empty_host_rejected() {
        assert!(normalize_host_url("").is_err());
        assert!(normalize_host_url("ws:///path").is_err());
    }

    #[test]
    fn connect_url_preserves_explicit_paths() {
        assert_eq!(normalize_connect_url("ws://h/ws/foo").unwrap(), "ws://h/ws/foo");
        assert_eq!(normalize_connect_url("ws://h").unwrap(), "ws://h/ws");
    }

    #[test]
    fn url_encode_password_special_chars() {
        assert_eq!(urlencode("hello world"), "hello%20world");
        assert_eq!(urlencode("a&b=c"), "a%26b%3Dc");
        assert_eq!(urlencode("plain123"), "plain123");
    }

    #[test]
    fn valid_room_id_rules() {
        assert!(valid_room_id("foo"));
        assert!(valid_room_id("Room_42-x"));
        assert!(!valid_room_id(""));
        assert!(!valid_room_id("with space"));
        assert!(!valid_room_id("../escape"));
        assert!(!valid_room_id(&"x".repeat(33)));
    }
}
