//! Tiny text-input helpers for the host/lobby/create-room prompts.
//!
//! Deliberately not pulling in the `tui-input` crate — these inputs are
//! short ASCII strings (host:port, room id, password) and a 30-line helper
//! covers every keypress we care about.

/// Append a printable char, capped at `max_len` bytes. Non-printables and
/// over-budget chars are silently dropped.
pub fn push_char(buf: &mut String, ch: char, max_len: usize) {
    if !ch.is_control() && buf.len() + ch.len_utf8() <= max_len {
        buf.push(ch);
    }
}

/// Pop the last char (or no-op if empty).
pub fn backspace(buf: &mut String) {
    buf.pop();
}

/// Mask a password for display: `••••••` (one `•` per char) up to 32, then
/// `…` for any extra. Empty becomes the placeholder `(none)`.
pub fn mask(pw: &str) -> String {
    if pw.is_empty() {
        return "(none)".into();
    }
    let n = pw.chars().count();
    if n <= 32 {
        "•".repeat(n)
    } else {
        format!("{}…", "•".repeat(31))
    }
}
