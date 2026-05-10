//! WebSocket URL helpers. Server-hosted builds use same-origin by default:
//! Trunk dev-server proxies `/ws` and `/lobby` to chess-net at :7878, while
//! production chess-net serves `dist/` at the same origin as the WS routes.
//!
//! Static builds (GitHub Pages) cannot assume same-origin WS routes, so they
//! require a user-provided `?ws=wss://host` URL or a stored localStorage value.

use crate::routes::{is_static_hosting, normalize_ws_base, WsBaseError};

pub const WS_QUERY_KEY: &str = "ws";
pub const WS_STORAGE_KEY: &str = "chess-web.ws-base";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WsBase {
    pub base: String,
    pub persist_in_urls: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WsBaseChoiceError {
    Missing,
    Invalid(WsBaseError),
}

pub fn resolve_ws_base(raw_query: Option<String>) -> Result<WsBase, WsBaseChoiceError> {
    if let Some(raw) = raw_query {
        let base = normalize_ws_base(&raw).map_err(WsBaseChoiceError::Invalid)?;
        write_stored_ws_base(&base);
        return Ok(WsBase { base, persist_in_urls: true });
    }

    if is_static_hosting() {
        if let Some(stored) = read_stored_ws_base() {
            if let Ok(base) = normalize_ws_base(&stored) {
                return Ok(WsBase { base, persist_in_urls: true });
            }
        }

        Err(WsBaseChoiceError::Missing)
    } else {
        Ok(WsBase { base: same_origin_ws_base(), persist_in_urls: false })
    }
}

pub fn same_origin_ws_base() -> String {
    let win = web_sys::window().expect("no window");
    let loc = win.location();
    let proto = loc.protocol().unwrap_or_else(|_| "http:".into());
    let scheme = if proto == "https:" { "wss" } else { "ws" };
    let host = loc.host().unwrap_or_default();
    format!("{}://{}", scheme, host)
}

pub fn lobby_url(ws: &WsBase) -> String {
    format!("{}/lobby", ws.base)
}

pub fn room_url(
    ws: &WsBase,
    room: &str,
    password: Option<&str>,
    spectator: bool,
    hints: bool,
) -> String {
    let base = format!("{}/ws/{}", ws.base, encode(room));
    let mut params: Vec<String> = Vec::new();
    if let Some(pw) = password.filter(|p| !p.is_empty()) {
        params.push(format!("password={}", encode(pw)));
    }
    if spectator {
        params.push("role=spectator".to_string());
    }
    if hints {
        // First-joiner request — sets `RoomState.hints_allowed = true`
        // for the room's lifetime. Subsequent joiners' values are
        // ignored server-side. See `crates/chess-net/src/protocol.rs`
        // PROTOCOL_VERSION v4→v5 doc.
        params.push("hints=1".to_string());
    }
    if params.is_empty() {
        base
    } else {
        format!("{}?{}", base, params.join("&"))
    }
}

pub fn ws_query_pair(ws: &WsBase) -> Option<String> {
    ws.persist_in_urls.then(|| format!("{WS_QUERY_KEY}={}", encode(&ws.base)))
}

pub fn append_query_pairs(path: &str, pairs: impl IntoIterator<Item = String>) -> String {
    let pairs: Vec<String> = pairs.into_iter().filter(|s| !s.is_empty()).collect();
    if pairs.is_empty() {
        path.to_string()
    } else {
        let sep = if path.contains('?') { '&' } else { '?' };
        format!("{path}{sep}{}", pairs.join("&"))
    }
}

pub fn read_stored_ws_base() -> Option<String> {
    local_storage().and_then(|storage| storage.get_item(WS_STORAGE_KEY).ok().flatten())
}

pub fn write_stored_ws_base(base: &str) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(WS_STORAGE_KEY, base);
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

fn encode(s: &str) -> String {
    js_sys::encode_uri_component(s).as_string().unwrap_or_default()
}
