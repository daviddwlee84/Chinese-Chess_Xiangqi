//! Multi-room ws server. Routes:
//!
//!   GET /             → upgrade into default room "main" (back-compat)
//!   GET /ws           → upgrade into default room "main" (back-compat)
//!   GET /ws/:room_id  → upgrade into named room (auto-created on first join)
//!   GET /lobby        → subscribe to live `Rooms` pushes
//!   GET /rooms        → JSON snapshot for `curl`/debug
//!
//! Optional `?password=<secret>` query param on the game-room routes locks
//! the room to the first joiner's secret. Wrong password gets
//! `Error{"bad password"}` + close.
//!
//! Optional `?role=spectator` upgrades the connection as a read-only
//! spectator instead of a seat. Spectators receive `Spectating` (instead of
//! `Hello`) plus `ChatHistory`, then live `Update` and `Chat` pushes. They
//! cannot move, resign, request rematch, or send chat. Player connections
//! arrive without `?role=spectator` — back-compat means v2 clients keep
//! getting "room full" if they're the third joiner.
//!
//! Empty rooms are GC'd when the last seat disconnects **and** the last
//! spectator leaves, except `main` which is permanent so v1 clients always
//! have a stable default to land in.
//!
//! Reconnect / mixed-variant rooms / time controls / TLS still deferred —
//! see `TODO.md`.
//!
//! ## Refactor history
//!
//! As of 2026-05-11 (Phase 1 of `backlog/webrtc-lan-pairing.md`) all room
//! state lives in [`crate::room::Room`] — a transport-agnostic state
//! machine that compiles for `wasm32-unknown-unknown`. This file is now
//! a thin axum/tokio glue that owns:
//!
//! 1. The per-room `Arc<Mutex<RoomEntry>>` table (room + peer routing map).
//! 2. The lobby-summary projection + push fan-out.
//! 3. `?password=`/`?role=`/`?hints=` URL parsing.
//! 4. The `--static-dir` static-file serving.
//!
//! Per-message work is just: deserialize → `room.apply(peer, msg)` →
//! fan out the returned `Vec<Outbound>` via the per-room routing table.

// === module skeleton — section bodies filled in via edits below ===

// SECTION: imports
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
#[cfg(feature = "static-serve")]
use axum::http::{header, HeaderValue};
#[cfg(feature = "static-serve")]
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chess_core::rules::RuleSet;
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};

use crate::protocol::{ClientMsg, RoomSummary, ServerMsg};
use crate::room::{
    parse_hints_param, valid_password, valid_room_id, JoinError, Outbound, PeerId, Room,
    DEFAULT_MAX_SPECTATORS, DEFAULT_ROOM, MAX_PASSWORD_LEN, MAX_ROOM_ID_LEN,
};
// SECTION: cache constants (static-serve)
#[cfg(feature = "static-serve")]
const IMMUTABLE_ASSET_CACHE: &str = "public, max-age=31536000, immutable";
#[cfg(feature = "static-serve")]
const HTML_CACHE: &str = "no-cache";
// SECTION: app state types

/// Per-connection outbound channel. The handler hands one of these to the
/// per-room peer map; the `Room::apply` fan-out delivers `ServerMsg`s into
/// it; a per-connection write task drains it onto the WebSocket sink.
type PeerSender = mpsc::UnboundedSender<ServerMsg>;

/// One row in the per-room peer routing table.
struct RoomEntry {
    room: Room,
    peers: HashMap<PeerId, PeerSender>,
}

impl RoomEntry {
    fn new(room: Room) -> Self {
        Self { room, peers: HashMap::new() }
    }

    /// Deliver an `Outbound` list to the matching peer senders. Drops any
    /// outbound whose target peer has already disconnected — its sender
    /// is gone from the map.
    fn fanout(&self, out: Vec<Outbound>) {
        for ob in out {
            if let Some(tx) = self.peers.get(&ob.peer) {
                let _ = tx.send(ob.msg);
            }
        }
    }
}

type RoomMap = Arc<Mutex<HashMap<String, Arc<Mutex<RoomEntry>>>>>;
type SummaryMap = Arc<Mutex<HashMap<String, RoomSummary>>>;
type LobbySubs = Arc<Mutex<Vec<mpsc::UnboundedSender<ServerMsg>>>>;

/// Server-wide state shared across every connection. Lock acquisition order
/// when multiple maps are touched: `rooms` → `summaries` → `lobby_subs`.
/// Per-room inner locks (`Arc<Mutex<RoomEntry>>`) live separately and are
/// never held across an outer-map lock acquisition.
pub struct AppState {
    rooms: RoomMap,
    /// Lobby-visible projection of every live room. Updated by
    /// `refresh_summary` / `drop_summary` after every room mutation, so
    /// `notify_lobby` can build a `Rooms` push without touching any inner
    /// `Room` lock.
    summaries: SummaryMap,
    /// Outbound senders for every connected lobby socket.
    lobby_subs: LobbySubs,
    /// Variant for every auto-created room. Mixed-variant servers are a
    /// separate TODO (`chess-net: mixed-variant rooms`).
    default_rules: RuleSet,
    /// Per-room cap on `?role=spectator` connections. The 17th spectator
    /// (at the default cap of 16) gets `Error{"room watch capacity reached"}`.
    max_spectators: usize,
    /// Monotonic counter for `PeerId` allocation. Only the value's
    /// uniqueness within a single room matters; using one global counter
    /// keeps the implementation trivial.
    next_peer_id: AtomicU64,
}

impl AppState {
    fn new(default_rules: RuleSet, max_spectators: usize) -> Arc<Self> {
        Arc::new(Self {
            rooms: Arc::new(Mutex::new(HashMap::new())),
            summaries: Arc::new(Mutex::new(HashMap::new())),
            lobby_subs: Arc::new(Mutex::new(Vec::new())),
            default_rules,
            max_spectators,
            next_peer_id: AtomicU64::new(1),
        })
    }

    fn alloc_peer(&self) -> PeerId {
        PeerId(self.next_peer_id.fetch_add(1, Ordering::Relaxed))
    }
}
// SECTION: query types

#[derive(Debug, Deserialize)]
struct AuthQuery {
    #[serde(default)]
    password: Option<String>,
    /// Opt-in spectator request. `Some("spectator")` upgrades the connection
    /// as a read-only watcher. v2 clients never set this so back-compat
    /// behaviour is preserved (third joiner still gets "room full").
    #[serde(default)]
    role: Option<String>,
    /// Opt-in AI hint sanction. `Some("1")` / `Some("true")` from the
    /// **first joiner** sets `Room.hints_allowed = true` for the room's
    /// lifetime; subsequent joiners' values are ignored.
    #[serde(default)]
    hints: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum JoinRole {
    Player,
    Spectator,
}

fn parse_role(raw: Option<&str>) -> JoinRole {
    match raw {
        Some(s) if s.eq_ignore_ascii_case("spectator") => JoinRole::Spectator,
        _ => JoinRole::Player,
    }
}
// SECTION: serve options + run/serve entry points

/// Server options. `rules` is required; `static_dir` is opt-in and only
/// honored when the `static-serve` feature is enabled.
#[derive(Clone, Debug)]
pub struct ServeOpts {
    pub rules: RuleSet,
    pub static_dir: Option<std::path::PathBuf>,
    /// Per-room spectator cap. Defaults to [`DEFAULT_MAX_SPECTATORS`] when
    /// constructed via [`ServeOpts::new`].
    pub max_spectators: usize,
}

impl ServeOpts {
    pub fn new(rules: RuleSet) -> Self {
        Self { rules, static_dir: None, max_spectators: DEFAULT_MAX_SPECTATORS }
    }

    pub fn with_static_dir(mut self, dir: Option<std::path::PathBuf>) -> Self {
        self.static_dir = dir;
        self
    }

    pub fn with_max_spectators(mut self, cap: usize) -> Self {
        self.max_spectators = cap;
        self
    }
}

/// Bind `addr` and serve until error / SIGINT (caller's choice — we don't
/// install signal handling here). For tests that need an ephemeral port,
/// use [`serve`] directly with a pre-bound listener.
pub async fn run(addr: SocketAddr, rules: RuleSet) -> Result<()> {
    run_with(addr, ServeOpts::new(rules)).await
}

/// Like [`run`] but with the full options struct (e.g. `--static-dir`).
pub async fn run_with(addr: SocketAddr, opts: ServeOpts) -> Result<()> {
    let listener =
        tokio::net::TcpListener::bind(addr).await.with_context(|| format!("bind {addr}"))?;
    eprintln!("[server] listening on ws://{}", listener.local_addr()?);
    if opts.static_dir.is_some() {
        eprintln!("[server] serving static dir at /");
    }
    serve_with(listener, opts).await
}

/// Serve on a pre-bound listener. The listener owns the port; use
/// `listener.local_addr()` before passing it in if you need to know it
/// (e.g. ephemeral ports in tests).
pub async fn serve(listener: tokio::net::TcpListener, rules: RuleSet) -> Result<()> {
    serve_with(listener, ServeOpts::new(rules)).await
}

/// Like [`serve`] but with the full options struct.
pub async fn serve_with(listener: tokio::net::TcpListener, opts: ServeOpts) -> Result<()> {
    let app_state = AppState::new(opts.rules, opts.max_spectators);
    let mut app = Router::new()
        .route("/ws", get(upgrade_default))
        .route("/ws/:room_id", get(upgrade_room))
        .route("/lobby", get(upgrade_lobby))
        .route("/rooms", get(rooms_snapshot_json));

    #[cfg(feature = "static-serve")]
    {
        if let Some(dir) = opts.static_dir.as_ref() {
            // SPA fallback — `/local/xiangqi`, `/play/foo`, etc. all
            // serve `index.html` so client-side routing takes over. The
            // explicit `/ws*`, `/lobby`, `/rooms` routes above still match
            // first because axum tries routes before fallback.
            //
            // NOTE: enabling --static-dir means `GET /` serves index.html.
            // Old `chess-tui --connect ws://host` (which hits `/`) must
            // switch to `--connect ws://host/ws`.
            use tower::ServiceBuilder;
            use tower_http::compression::CompressionLayer;
            use tower_http::services::{ServeDir, ServeFile};
            let index = dir.join("index.html");
            let serve_dir = ServeDir::new(dir).fallback(ServeFile::new(index));
            let static_service = ServiceBuilder::new()
                .layer(CompressionLayer::new().no_deflate().no_zstd())
                .service(serve_dir);
            app = app
                .fallback_service(static_service)
                .layer(axum::middleware::from_fn(static_cache_headers));
        } else {
            app = app.route("/", get(upgrade_default));
        }
    }
    #[cfg(not(feature = "static-serve"))]
    {
        let _ = opts.static_dir;
        app = app.route("/", get(upgrade_default));
    }

    let app = app.with_state(app_state);
    axum::serve(listener, app).await.context("axum::serve")?;
    Ok(())
}
// SECTION: static-serve cache headers

#[cfg(feature = "static-serve")]
async fn static_cache_headers(req: axum::extract::Request, next: Next) -> axum::response::Response {
    let path = req.uri().path().to_string();
    let mut res = next.run(req).await;
    if let Some(value) =
        cache_control_for_static_response(&path, res.headers().get(header::CONTENT_TYPE))
    {
        res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    }
    res
}

#[cfg(feature = "static-serve")]
fn cache_control_for_static_response(
    path: &str,
    content_type: Option<&HeaderValue>,
) -> Option<&'static str> {
    if path == "/rooms" || path == "/lobby" || path.starts_with("/ws") {
        return None;
    }
    let content_type = content_type.and_then(|value| value.to_str().ok()).unwrap_or_default();
    if content_type.starts_with("text/html") {
        return Some(HTML_CACHE);
    }
    if is_hashed_asset_path(path) {
        return Some(IMMUTABLE_ASSET_CACHE);
    }
    Some(HTML_CACHE)
}

#[cfg(feature = "static-serve")]
fn is_hashed_asset_path(path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or_default();
    (file_name.ends_with(".css") || file_name.ends_with(".js") || file_name.ends_with(".wasm"))
        && file_name.contains('-')
}
// SECTION: room socket upgrade

async fn upgrade_default(
    State(app): State<Arc<AppState>>,
    Query(q): Query<AuthQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let role = parse_role(q.role.as_deref());
    let hints = parse_hints_param(q.hints.as_deref());
    ws.on_upgrade(move |socket| {
        handle_room_socket(socket, app, DEFAULT_ROOM.to_string(), q.password, role, hints)
    })
}

async fn upgrade_room(
    State(app): State<Arc<AppState>>,
    Path(room_id): Path<String>,
    Query(q): Query<AuthQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let room_id_clone = room_id.clone();
    let role = parse_role(q.role.as_deref());
    let hints = parse_hints_param(q.hints.as_deref());
    ws.on_upgrade(move |socket| {
        handle_room_socket(socket, app, room_id_clone, q.password, role, hints)
    })
}

async fn upgrade_lobby(
    State(app): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_lobby_socket(socket, app))
}
// SECTION: room socket handler

async fn handle_room_socket(
    socket: WebSocket,
    app: Arc<AppState>,
    room_id: String,
    password_param: Option<String>,
    role: JoinRole,
    hints_param: bool,
) {
    let (mut sender, mut receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<ServerMsg>();

    // === Pre-flight validation ===
    if !valid_room_id(&room_id) {
        let _ = send_close_with(
            &mut sender,
            ServerMsg::Error {
                message: format!(
                    "invalid room id (allowed: a-z A-Z 0-9 _ -, max {MAX_ROOM_ID_LEN} chars)"
                ),
            },
        )
        .await;
        return;
    }
    if let Some(pw) = password_param.as_ref() {
        if !valid_password(pw) {
            let _ = send_close_with(
                &mut sender,
                ServerMsg::Error {
                    message: format!(
                        "invalid password (max {MAX_PASSWORD_LEN} chars, no control chars)"
                    ),
                },
            )
            .await;
            return;
        }
    }

    // === Get-or-create the room ===
    //
    // Spectators don't auto-create — joining as a spectator into a
    // non-existent room would be a confusing UX (you'd see an empty board
    // with no players). Players still create on demand.
    let room_entry = {
        let mut rooms = app.rooms.lock().await;
        if role == JoinRole::Spectator && !rooms.contains_key(&room_id) {
            drop(rooms);
            let _ = send_close_with(
                &mut sender,
                ServerMsg::Error { message: "no such room to spectate".into() },
            )
            .await;
            return;
        }
        rooms
            .entry(room_id.clone())
            .or_insert_with(|| {
                Arc::new(Mutex::new(RoomEntry::new(Room::new(
                    app.default_rules.clone(),
                    password_param.clone(),
                    hints_param,
                ))))
            })
            .clone()
    };

    // === Password gate ===
    //
    // Locked rooms are locked for everyone (seats and spectators).
    {
        let entry = room_entry.lock().await;
        if entry.room.password() != password_param.as_deref() {
            drop(entry);
            let _ =
                send_close_with(&mut sender, ServerMsg::Error { message: "bad password".into() })
                    .await;
            return;
        }
    }

    // === Seat the peer ===
    let peer = app.alloc_peer();
    let summary_after_join = {
        let mut entry = room_entry.lock().await;
        // Insert sender FIRST so any outbound the room emits during join
        // (Hello / ChatHistory) is deliverable.
        entry.peers.insert(peer, out_tx.clone());
        // The two role branches return different shapes — `join_player`
        // also reports the assigned `Side` for the eprintln. Handle each
        // branch separately and reduce both into `Result<Vec<Outbound>, JoinError>`.
        let join_result: Result<Vec<Outbound>, JoinError> = match role {
            JoinRole::Player => match entry.room.join_player(peer) {
                Ok((side, out)) => {
                    eprintln!(
                        "[server][{}] seated {:?} ({}/2) as peer {}",
                        room_id,
                        side,
                        entry.room.seat_count(),
                        peer.0
                    );
                    Ok(out)
                }
                Err(e) => Err(e),
            },
            JoinRole::Spectator => {
                let cap = app.max_spectators;
                match entry.room.join_spectator(peer, cap) {
                    Ok(out) => {
                        eprintln!(
                            "[server][{}] spectator joined as peer {} ({}/{})",
                            room_id,
                            peer.0,
                            entry.room.spectator_count(),
                            cap
                        );
                        Ok(out)
                    }
                    Err(e) => Err(e),
                }
            }
        };
        match join_result {
            Ok(outbound) => {
                entry.fanout(outbound);
                Some(entry.room.summary(&room_id))
            }
            Err(err) => {
                // Roll back the peer insert; never seated.
                entry.peers.remove(&peer);
                let msg = match err {
                    JoinError::RoomFull => "room full",
                    JoinError::SpectatorCapReached => "room watch capacity reached",
                };
                drop(entry);
                let _ =
                    send_close_with(&mut sender, ServerMsg::Error { message: msg.into() }).await;
                return;
            }
        }
    };
    if let Some(summary) = summary_after_join {
        refresh_summary(&app, &room_id, summary).await;
    }

    // === Write task: drain mpsc → ws sink ===
    //
    // Exits naturally when all senders drop.
    let write_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[server][warn] serialize ServerMsg: {e}");
                    continue;
                }
            };
            if sender.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
        let _ = sender.close().await;
    });

    // === Read loop ===
    while let Some(frame) = receiver.next().await {
        let frame = match frame {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[server][warn][{}] ws read error (peer {}): {e}", room_id, peer.0);
                break;
            }
        };
        let text = match frame {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };
        let msg: ClientMsg = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                let _ =
                    out_tx.send(ServerMsg::Error { message: format!("malformed message: {e}") });
                continue;
            }
        };
        let entry_arc = match get_room(&app, &room_id).await {
            Some(r) => r,
            None => break,
        };
        let summary_after = {
            let mut entry = entry_arc.lock().await;
            let outbound = entry.room.apply(peer, msg);
            entry.fanout(outbound);
            entry.room.summary(&room_id)
        };
        refresh_summary(&app, &room_id, summary_after).await;
    }

    // === Disconnect cleanup ===
    eprintln!("[server][{}] peer {} disconnected", room_id, peer.0);
    if let Some(entry_arc) = get_room(&app, &room_id).await {
        let (summary_after, room_now_empty) = {
            let mut entry = entry_arc.lock().await;
            let outbound = entry.room.leave(peer);
            entry.fanout(outbound);
            entry.peers.remove(&peer);
            (entry.room.summary(&room_id), entry.room.is_empty())
        };
        if room_now_empty && room_id != DEFAULT_ROOM {
            let mut rooms = app.rooms.lock().await;
            rooms.remove(&room_id);
            drop(rooms);
            drop_summary(&app, &room_id).await;
        } else {
            refresh_summary(&app, &room_id, summary_after).await;
        }
    }

    drop(out_tx);
    let _ = write_task.await;
}
// SECTION: lobby socket handler

async fn handle_lobby_socket(socket: WebSocket, app: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<ServerMsg>();

    // Snapshot first, then subscribe. Doing it in this order guarantees the
    // subscriber doesn't miss a push that lands between snapshot and
    // subscribe — the worst case is they get the same state twice, which
    // the client treats as idempotent.
    let snapshot = current_summary_list(&app).await;
    if out_tx.send(ServerMsg::Rooms { rooms: snapshot }).is_err() {
        return;
    }

    {
        let mut subs = app.lobby_subs.lock().await;
        subs.push(out_tx.clone());
    }

    let write_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[server][warn] serialize lobby ServerMsg: {e}");
                    continue;
                }
            };
            if sender.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
        let _ = sender.close().await;
    });

    // Lobby read loop: only `ListRooms` is meaningful. Everything else gets
    // a polite Error (the client may have been confused about which socket
    // it has open).
    while let Some(frame) = receiver.next().await {
        let frame = match frame {
            Ok(f) => f,
            Err(_) => break,
        };
        let text = match frame {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };
        match serde_json::from_str::<ClientMsg>(&text) {
            Ok(ClientMsg::ListRooms) => {
                let snap = current_summary_list(&app).await;
                let _ = out_tx.send(ServerMsg::Rooms { rooms: snap });
            }
            Ok(_) => {
                let _ = out_tx.send(ServerMsg::Error {
                    message: "lobby socket only accepts ListRooms; join /ws/<id> to play".into(),
                });
            }
            Err(e) => {
                let _ =
                    out_tx.send(ServerMsg::Error { message: format!("malformed message: {e}") });
            }
        }
    }

    // Cleanup: remove this sub from the broadcast list.
    {
        let mut subs = app.lobby_subs.lock().await;
        subs.retain(|tx| !tx.same_channel(&out_tx));
    }
    drop(out_tx);
    let _ = write_task.await;
}
// SECTION: lobby summary helpers

async fn get_room(app: &AppState, room_id: &str) -> Option<Arc<Mutex<RoomEntry>>> {
    let rooms = app.rooms.lock().await;
    rooms.get(room_id).cloned()
}

async fn current_summary_list(app: &AppState) -> Vec<RoomSummary> {
    let summaries = app.summaries.lock().await;
    let mut rooms: Vec<RoomSummary> = summaries.values().cloned().collect();
    rooms.sort_by(|a, b| a.id.cmp(&b.id));
    rooms
}

async fn refresh_summary(app: &AppState, room_id: &str, summary: RoomSummary) {
    {
        let mut summaries = app.summaries.lock().await;
        summaries.insert(room_id.to_string(), summary);
    }
    notify_lobby(app).await;
}

async fn drop_summary(app: &AppState, room_id: &str) {
    {
        let mut summaries = app.summaries.lock().await;
        summaries.remove(room_id);
    }
    notify_lobby(app).await;
}

async fn notify_lobby(app: &AppState) {
    let snapshot = current_summary_list(app).await;
    let msg = ServerMsg::Rooms { rooms: snapshot };
    let subs: Vec<_> = {
        let s = app.lobby_subs.lock().await;
        s.clone()
    };
    for tx in subs {
        let _ = tx.send(msg.clone());
    }
}
// SECTION: rooms snapshot json

async fn rooms_snapshot_json(State(app): State<Arc<AppState>>) -> Json<Vec<RoomSummary>> {
    let summaries = app.summaries.lock().await;
    let mut rooms: Vec<RoomSummary> = summaries.values().cloned().collect();
    rooms.sort_by(|a, b| a.id.cmp(&b.id));
    Json(rooms)
}
// SECTION: send_close helper

async fn send_close_with(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    msg: ServerMsg,
) -> Result<(), axum::Error> {
    let json = serde_json::to_string(&msg).unwrap_or_default();
    sender.send(Message::Text(json)).await?;
    sender.close().await
}
