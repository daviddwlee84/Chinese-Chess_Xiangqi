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

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_core::state::{GameState, GameStatus, WinReason};
use chess_core::view::PlayerView;
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};

use crate::protocol::{
    variant_label, ChatLine, ClientMsg, RoomStatus, RoomSummary, ServerMsg, PROTOCOL_VERSION,
};

const DEFAULT_ROOM: &str = "main";
const MAX_PASSWORD_LEN: usize = 64;
const MAX_ROOM_ID_LEN: usize = 32;
const MAX_CHAT_LEN: usize = 256;
const CHAT_HISTORY_CAP: usize = 50;
const DEFAULT_MAX_SPECTATORS: usize = 16;

struct RoomState {
    state: GameState,
    /// (side, outbound mpsc tx). Empty until first connect; capped at 2.
    seats: Vec<(Side, mpsc::UnboundedSender<ServerMsg>)>,
    /// Read-only watchers connected via `?role=spectator`. Capped per
    /// `AppState::max_spectators`; the cap is enforced in
    /// `handle_room_socket` before insertion.
    spectators: Vec<mpsc::UnboundedSender<ServerMsg>>,
    /// Sides that have requested a rematch since the last reset. Cleared
    /// every time the game resets; mutual consent triggers reset.
    rematch: Vec<Side>,
    /// Friend-list lock. `None` = open. Set by the first joiner from the
    /// `?password=` query string and never mutated afterwards. **Plain text
    /// is intentional**: this is a "type the secret to enter the game with
    /// the right friends" mechanism, not a security boundary. Real auth
    /// belongs in a follow-up alongside TLS.
    password: Option<String>,
    /// In-memory ring buffer of recent chat lines. Capped at
    /// `CHAT_HISTORY_CAP`; oldest line drops when a new one arrives at
    /// capacity. Sent verbatim to every new joiner via `ChatHistory`.
    chat: VecDeque<ChatLine>,
}

impl RoomState {
    fn new(rules: RuleSet, password: Option<String>) -> Self {
        Self {
            state: GameState::new(rules),
            seats: Vec::with_capacity(2),
            spectators: Vec::new(),
            rematch: Vec::with_capacity(2),
            password,
            chat: VecDeque::with_capacity(CHAT_HISTORY_CAP),
        }
    }

    fn next_seat(&self) -> Option<Side> {
        match self.seats.len() {
            0 => Some(Side::RED),
            1 => {
                let taken = self.seats[0].0;
                Some(if taken == Side::RED { Side::BLACK } else { Side::RED })
            }
            _ => None,
        }
    }

    fn summary(&self, id: &str) -> RoomSummary {
        let status = match self.state.status {
            GameStatus::Won { .. } | GameStatus::Drawn { .. } => RoomStatus::Finished,
            GameStatus::Ongoing => {
                if self.seats.len() >= 2 {
                    RoomStatus::Playing
                } else {
                    RoomStatus::Lobby
                }
            }
        };
        RoomSummary {
            id: id.to_string(),
            variant: variant_label(&self.state.rules).to_string(),
            seats: self.seats.len() as u8,
            spectators: self.spectators.len() as u16,
            has_password: self.password.is_some(),
            status,
        }
    }

    fn chat_history(&self) -> Vec<ChatLine> {
        self.chat.iter().cloned().collect()
    }
}

type RoomMap = Arc<Mutex<HashMap<String, Arc<Mutex<RoomState>>>>>;
type SummaryMap = Arc<Mutex<HashMap<String, RoomSummary>>>;
type LobbySubs = Arc<Mutex<Vec<mpsc::UnboundedSender<ServerMsg>>>>;

/// Server-wide state shared across every connection. The three top-level
/// locks are independent and acquired in a strict order whenever multiple
/// are needed: `rooms` → `summaries` → `lobby_subs`. Per-room inner locks
/// (`Arc<Mutex<RoomState>>`) live separately and are never held across an
/// outer-map lock acquisition.
pub struct AppState {
    rooms: RoomMap,
    /// Lobby-visible projection of every live room. Updated by
    /// [`refresh_summary`] / [`drop_summary`] after every room mutation, so
    /// `notify_lobby` can build a `Rooms` push without touching any inner
    /// `RoomState` lock.
    summaries: SummaryMap,
    /// Outbound senders for every connected lobby socket.
    lobby_subs: LobbySubs,
    /// Variant for every auto-created room. Mixed-variant servers are a
    /// separate TODO (`chess-net: mixed-variant rooms`).
    default_rules: RuleSet,
    /// Per-room cap on `?role=spectator` connections. The 17th spectator (at
    /// the default cap of 16) gets `Error{"room watch capacity reached"}`.
    max_spectators: usize,
}

impl AppState {
    fn new(default_rules: RuleSet, max_spectators: usize) -> Arc<Self> {
        Arc::new(Self {
            rooms: Arc::new(Mutex::new(HashMap::new())),
            summaries: Arc::new(Mutex::new(HashMap::new())),
            lobby_subs: Arc::new(Mutex::new(Vec::new())),
            default_rules,
            max_spectators,
        })
    }
}

#[derive(Debug, Deserialize)]
struct AuthQuery {
    #[serde(default)]
    password: Option<String>,
    /// Opt-in spectator request. `Some("spectator")` upgrades the connection
    /// as a read-only watcher. v2 clients never set this so back-compat
    /// behaviour is preserved (third joiner still gets "room full").
    #[serde(default)]
    role: Option<String>,
}

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
            // SPA fallback — `/local/xiangqi`, `/lobby`, `/play/foo` all
            // serve `index.html` so client-side routing takes over. The
            // explicit `/ws*`, `/lobby`, `/rooms` routes above still match
            // first because axum tries routes before fallback.
            //
            // NOTE: enabling --static-dir means `GET /` serves index.html.
            // Old `chess-tui --connect ws://host` (which hits `/`) must
            // switch to `--connect ws://host/ws`.
            use tower_http::services::{ServeDir, ServeFile};
            let index = dir.join("index.html");
            let serve_dir = ServeDir::new(dir).fallback(ServeFile::new(index));
            app = app.fallback_service(serve_dir);
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

fn valid_room_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ROOM_ID_LEN
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn valid_password(pw: &str) -> bool {
    !pw.is_empty() && pw.len() <= MAX_PASSWORD_LEN && pw.chars().all(|c| !c.is_control())
}

async fn upgrade_default(
    State(app): State<Arc<AppState>>,
    Query(q): Query<AuthQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let role = parse_role(q.role.as_deref());
    ws.on_upgrade(move |socket| {
        handle_room_socket(socket, app, DEFAULT_ROOM.to_string(), q.password, role)
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
    ws.on_upgrade(move |socket| handle_room_socket(socket, app, room_id_clone, q.password, role))
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

async fn upgrade_lobby(
    State(app): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_lobby_socket(socket, app))
}

async fn rooms_snapshot_json(State(app): State<Arc<AppState>>) -> Json<Vec<RoomSummary>> {
    let summaries = app.summaries.lock().await;
    let mut rooms: Vec<RoomSummary> = summaries.values().cloned().collect();
    rooms.sort_by(|a, b| a.id.cmp(&b.id));
    Json(rooms)
}

async fn handle_room_socket(
    socket: WebSocket,
    app: Arc<AppState>,
    room_id: String,
    password_param: Option<String>,
    role: JoinRole,
) {
    let (mut sender, mut receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<ServerMsg>();

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

    // Get-or-create the room. Spectators don't auto-create — joining as a
    // spectator into a non-existent room would be a confusing UX (you'd see
    // an empty board with no players). Players still create on demand.
    let room = {
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
                Arc::new(Mutex::new(RoomState::new(
                    app.default_rules.clone(),
                    password_param.clone(),
                )))
            })
            .clone()
    };

    // Validate password (applies to both seats and spectators — locked rooms
    // are locked for everyone).
    {
        let g = room.lock().await;
        if g.password != password_param {
            drop(g);
            let _ =
                send_close_with(&mut sender, ServerMsg::Error { message: "bad password".into() })
                    .await;
            return;
        }
    }

    let connection: Connection = match role {
        JoinRole::Player => {
            let mut g = room.lock().await;
            match g.next_seat() {
                Some(s) => {
                    let view = PlayerView::project(&g.state, s);
                    let hello = ServerMsg::Hello {
                        protocol: PROTOCOL_VERSION,
                        observer: s,
                        rules: g.state.rules.clone(),
                        view,
                    };
                    if out_tx.send(hello).is_err() {
                        return;
                    }
                    if out_tx.send(ServerMsg::ChatHistory { lines: g.chat_history() }).is_err() {
                        return;
                    }
                    g.seats.push((s, out_tx.clone()));
                    eprintln!("[server][{}] seated {:?} ({}/2)", room_id, s, g.seats.len());
                    let summary = g.summary(&room_id);
                    drop(g);
                    refresh_summary(&app, &room_id, summary).await;
                    Connection::Player(s)
                }
                None => {
                    drop(g);
                    let _ = send_close_with(
                        &mut sender,
                        ServerMsg::Error { message: "room full".into() },
                    )
                    .await;
                    return;
                }
            }
        }
        JoinRole::Spectator => {
            let mut g = room.lock().await;
            if g.spectators.len() >= app.max_spectators {
                drop(g);
                let _ = send_close_with(
                    &mut sender,
                    ServerMsg::Error { message: "room watch capacity reached".into() },
                )
                .await;
                return;
            }
            // Spectators see the board from RED's POV — `PlayerView::project`
            // is leak-safe so banqi hidden tiles stay opaque. Three-kingdom
            // would need a neutral projection later (out of scope).
            let view = PlayerView::project(&g.state, Side::RED);
            let welcome = ServerMsg::Spectating {
                protocol: PROTOCOL_VERSION,
                rules: g.state.rules.clone(),
                view,
            };
            if out_tx.send(welcome).is_err() {
                return;
            }
            if out_tx.send(ServerMsg::ChatHistory { lines: g.chat_history() }).is_err() {
                return;
            }
            g.spectators.push(out_tx.clone());
            eprintln!(
                "[server][{}] spectator joined ({}/{})",
                room_id,
                g.spectators.len(),
                app.max_spectators
            );
            let summary = g.summary(&room_id);
            drop(g);
            refresh_summary(&app, &room_id, summary).await;
            Connection::Spectator
        }
    };

    // Write task: drain mpsc → ws sink. Exits naturally when all senders drop.
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

    // Read loop. Both seat and spectator share this — process_client_msg
    // gates on `connection` for Move/Resign/Rematch/Chat permissions.
    while let Some(frame) = receiver.next().await {
        let frame = match frame {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[server][warn][{}] ws read error ({:?}): {e}", room_id, connection);
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
        let room = match get_room(&app, &room_id).await {
            Some(r) => r,
            None => break,
        };
        let summary_after = {
            let mut g = room.lock().await;
            process_client_msg(&mut g, connection, &out_tx, msg, &room_id);
            g.summary(&room_id)
        };
        refresh_summary(&app, &room_id, summary_after).await;
    }

    // Disconnect: remove seat or spectator slot, notify peer if a player
    // dropped during a live game, GC empty rooms except "main".
    eprintln!("[server][{}] {:?} disconnected", room_id, connection);
    if let Some(room) = get_room(&app, &room_id).await {
        let (summary_after, room_now_empty) = {
            let mut g = room.lock().await;
            match connection {
                Connection::Player(seat) => {
                    g.seats.retain(|(s, _)| *s != seat);
                    g.rematch.retain(|s| *s != seat);
                    if matches!(g.state.status, GameStatus::Ongoing) && !g.seats.is_empty() {
                        for (_, tx) in &g.seats {
                            let _ = tx
                                .send(ServerMsg::Error { message: "opponent disconnected".into() });
                        }
                    }
                }
                Connection::Spectator => {
                    g.spectators.retain(|tx| !tx.same_channel(&out_tx));
                }
            }
            let empty = g.seats.is_empty() && g.spectators.is_empty();
            (g.summary(&room_id), empty)
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

#[derive(Copy, Clone, Debug)]
enum Connection {
    Player(Side),
    Spectator,
}

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

async fn get_room(app: &AppState, room_id: &str) -> Option<Arc<Mutex<RoomState>>> {
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

async fn send_close_with(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    msg: ServerMsg,
) -> Result<(), axum::Error> {
    let json = serde_json::to_string(&msg).unwrap_or_default();
    sender.send(Message::Text(json)).await?;
    sender.close().await
}

fn process_client_msg(
    g: &mut RoomState,
    connection: Connection,
    self_tx: &mpsc::UnboundedSender<ServerMsg>,
    msg: ClientMsg,
    room_id: &str,
) {
    let seat = match connection {
        Connection::Player(s) => Some(s),
        Connection::Spectator => None,
    };
    match msg {
        ClientMsg::Rematch => match seat {
            Some(s) => process_rematch(g, s, room_id),
            None => {
                let _ = self_tx.send(ServerMsg::Error {
                    message: "spectators cannot request a rematch".into(),
                });
            }
        },
        ClientMsg::Move { mv } => match seat {
            Some(s) => {
                if !matches!(g.state.status, GameStatus::Ongoing) {
                    send_to(
                        g,
                        s,
                        ServerMsg::Error {
                            message: "game is over — press 'n' to request a rematch".into(),
                        },
                    );
                    return;
                }
                process_move(g, s, mv, room_id);
            }
            None => {
                let _ = self_tx.send(ServerMsg::Error { message: "spectators cannot move".into() });
            }
        },
        ClientMsg::Resign => match seat {
            Some(s) => {
                if !matches!(g.state.status, GameStatus::Ongoing) {
                    send_to(g, s, ServerMsg::Error { message: "game is over".into() });
                    return;
                }
                let winner = if s == Side::RED { Side::BLACK } else { Side::RED };
                g.state.status = GameStatus::Won { winner, reason: WinReason::Resignation };
                broadcast_update(g);
            }
            None => {
                let _ =
                    self_tx.send(ServerMsg::Error { message: "spectators cannot resign".into() });
            }
        },
        ClientMsg::ListRooms => {
            let _ = self_tx.send(ServerMsg::Error {
                message: "ListRooms is a lobby-only message; subscribe via /lobby".into(),
            });
        }
        ClientMsg::Chat { text } => match seat {
            Some(s) => process_chat(g, s, text, self_tx, room_id),
            None => {
                let _ = self_tx.send(ServerMsg::Error { message: "spectators cannot chat".into() });
            }
        },
    }
}

fn process_chat(
    g: &mut RoomState,
    from: Side,
    raw: String,
    self_tx: &mpsc::UnboundedSender<ServerMsg>,
    room_id: &str,
) {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        let _ = self_tx.send(ServerMsg::Error { message: "chat message is empty".into() });
        return;
    }
    let mut text: String = trimmed.chars().take(MAX_CHAT_LEN).collect();
    text.shrink_to_fit();
    let line = ChatLine { from, text, ts_ms: now_ms() };
    if g.chat.len() >= CHAT_HISTORY_CAP {
        g.chat.pop_front();
    }
    g.chat.push_back(line.clone());
    eprintln!("[server][{}] chat {:?}: {}", room_id, from, line.text);
    broadcast_to_all(g, &ServerMsg::Chat { line });
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn process_rematch(g: &mut RoomState, seat: Side, room_id: &str) {
    if matches!(g.state.status, GameStatus::Ongoing) {
        send_to(
            g,
            seat,
            ServerMsg::Error {
                message: "Game still in progress — finish or resign first.".into()
            },
        );
        return;
    }
    if g.seats.len() < 2 {
        send_to(
            g,
            seat,
            ServerMsg::Error { message: "No opponent connected — can't start a rematch.".into() },
        );
        return;
    }
    if !g.rematch.contains(&seat) {
        g.rematch.push(seat);
    }
    let want_seats: Vec<Side> = g.seats.iter().map(|(s, _)| *s).collect();
    let all_ready = want_seats.iter().all(|s| g.rematch.contains(s));
    if all_ready {
        let rules = g.state.rules.clone();
        g.state = GameState::new(rules);
        g.rematch.clear();
        eprintln!("[server][{}] rematch — fresh game", room_id);
        for (side, tx) in &g.seats {
            let view = PlayerView::project(&g.state, *side);
            let _ = tx.send(ServerMsg::Hello {
                protocol: PROTOCOL_VERSION,
                observer: *side,
                rules: g.state.rules.clone(),
                view,
            });
        }
        if !g.spectators.is_empty() {
            let view = PlayerView::project(&g.state, Side::RED);
            for tx in &g.spectators {
                let _ = tx.send(ServerMsg::Spectating {
                    protocol: PROTOCOL_VERSION,
                    rules: g.state.rules.clone(),
                    view: view.clone(),
                });
            }
        }
    } else {
        for (s, tx) in &g.seats {
            let msg = if *s == seat {
                ServerMsg::Error { message: "Rematch requested. Waiting for opponent…".into() }
            } else {
                ServerMsg::Error {
                    message: "Opponent wants a rematch. Press 'n' to accept.".into(),
                }
            };
            let _ = tx.send(msg);
        }
    }
}

fn process_move(g: &mut RoomState, seat: Side, mv: Move, room_id: &str) {
    if g.state.side_to_move != seat {
        send_to(g, seat, ServerMsg::Error { message: "not your turn".into() });
        return;
    }
    match g.state.make_move(&mv) {
        Ok(()) => {
            g.state.refresh_status();
            eprintln!("[server][{}] {:?} -> {:?}", room_id, seat, mv);
            broadcast_update(g);
        }
        Err(e) => {
            send_to(g, seat, ServerMsg::Error { message: format!("illegal move: {e}") });
        }
    }
}

fn send_to(g: &RoomState, seat: Side, msg: ServerMsg) {
    if let Some((_, tx)) = g.seats.iter().find(|(s, _)| *s == seat) {
        let _ = tx.send(msg);
    }
}

fn broadcast_update(g: &RoomState) {
    for (side, tx) in &g.seats {
        let view = PlayerView::project(&g.state, *side);
        let _ = tx.send(ServerMsg::Update { view });
    }
    if !g.spectators.is_empty() {
        let view = PlayerView::project(&g.state, Side::RED);
        for tx in &g.spectators {
            let _ = tx.send(ServerMsg::Update { view: view.clone() });
        }
    }
}

/// Fan a single message out to every connection (seats + spectators).
/// Used for `Chat` where every recipient should see the same payload —
/// `Update` uses [`broadcast_update`] instead because seats need
/// per-side projections.
fn broadcast_to_all(g: &RoomState, msg: &ServerMsg) {
    for (_, tx) in &g.seats {
        let _ = tx.send(msg.clone());
    }
    for tx in &g.spectators {
        let _ = tx.send(msg.clone());
    }
}
