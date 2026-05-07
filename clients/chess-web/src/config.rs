//! WebSocket URL helpers. Same-origin by default — Trunk dev-server proxies
//! `/ws` and `/lobby` to chess-net at :7878 (see `Trunk.toml`); in production
//! chess-net serves the `dist/` build itself, so origin == host.

pub fn ws_base() -> String {
    let win = web_sys::window().expect("no window");
    let loc = win.location();
    let proto = loc.protocol().unwrap_or_else(|_| "http:".into());
    let scheme = if proto == "https:" { "wss" } else { "ws" };
    let host = loc.host().unwrap_or_default();
    format!("{}://{}", scheme, host)
}

pub fn lobby_url() -> String {
    format!("{}/lobby", ws_base())
}

pub fn room_url(room: &str, password: Option<&str>, spectator: bool) -> String {
    let base = format!("{}/ws/{}", ws_base(), encode(room));
    let mut params: Vec<String> = Vec::new();
    if let Some(pw) = password.filter(|p| !p.is_empty()) {
        params.push(format!("password={}", encode(pw)));
    }
    if spectator {
        params.push("role=spectator".to_string());
    }
    if params.is_empty() {
        base
    } else {
        format!("{}?{}", base, params.join("&"))
    }
}

fn encode(s: &str) -> String {
    js_sys::encode_uri_component(s).as_string().unwrap_or_default()
}
