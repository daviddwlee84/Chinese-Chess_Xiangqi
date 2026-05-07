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
//! Empty rooms are GC'd when the last seat disconnects, except `main` which
//! is permanent so v1 clients always have a stable default to land in.
//!
//! Reconnect / mixed-variant rooms / spectators / time controls / TLS still
//! deferred — see `TODO.md`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

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
    variant_label, ClientMsg, RoomStatus, RoomSummary, ServerMsg, PROTOCOL_VERSION,
};

const DEFAULT_ROOM: &str = "main";
const MAX_PASSWORD_LEN: usize = 64;
const MAX_ROOM_ID_LEN: usize = 32;

struct RoomState {
    state: GameState,
    /// (side, outbound mpsc tx). Empty until first connect; capped at 2.
    seats: Vec<(Side, mpsc::UnboundedSender<ServerMsg>)>,
    /// Sides that have requested a rematch since the last reset. Cleared
    /// every time the game resets; mutual consent triggers reset.
    rematch: Vec<Side>,
    /// Friend-list lock. `None` = open. Set by the first joiner from the
    /// `?password=` query string and never mutated afterwards. **Plain text
    /// is intentional**: this is a "type the secret to enter the game with
    /// the right friends" mechanism, not a security boundary. Real auth
    /// belongs in a follow-up alongside TLS.
    password: Option<String>,
}

impl RoomState {
    fn new(rules: RuleSet, password: Option<String>) -> Self {
        Self {
            state: GameState::new(rules),
            seats: Vec::with_capacity(2),
            rematch: Vec::with_capacity(2),
            password,
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
            has_password: self.password.is_some(),
            status,
        }
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
}

impl AppState {
    fn new(default_rules: RuleSet) -> Arc<Self> {
        Arc::new(Self {
            rooms: Arc::new(Mutex::new(HashMap::new())),
            summaries: Arc::new(Mutex::new(HashMap::new())),
            lobby_subs: Arc::new(Mutex::new(Vec::new())),
            default_rules,
        })
    }
}

#[derive(Debug, Deserialize)]
struct AuthQuery {
    #[serde(default)]
    password: Option<String>,
}

/// Server options. `rules` is required; `static_dir` is opt-in and only
/// honored when the `static-serve` feature is enabled.
#[derive(Clone, Debug)]
pub struct ServeOpts {
    pub rules: RuleSet,
    pub static_dir: Option<std::path::PathBuf>,
}

impl ServeOpts {
    pub fn new(rules: RuleSet) -> Self {
        Self { rules, static_dir: None }
    }

    pub fn with_static_dir(mut self, dir: Option<std::path::PathBuf>) -> Self {
        self.static_dir = dir;
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
    let app_state = AppState::new(opts.rules);
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
    ws.on_upgrade(move |socket| {
        handle_room_socket(socket, app, DEFAULT_ROOM.to_string(), q.password)
    })
}

async fn upgrade_room(
    State(app): State<Arc<AppState>>,
    Path(room_id): Path<String>,
    Query(q): Query<AuthQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let room_id_clone = room_id.clone();
    ws.on_upgrade(move |socket| handle_room_socket(socket, app, room_id_clone, q.password))
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

    // Get-or-create room + seat assignment + Hello. Held under one lock per
    // layer so a concurrent connect can't squeeze between assignment and
    // seat insertion. Outer-then-inner ordering throughout.
    let seat: Side = {
        let room = {
            let mut rooms = app.rooms.lock().await;
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
        let mut g = room.lock().await;
        // Existing room: validate password matches what was set on creation.
        if g.password != password_param {
            drop(g);
            let _ =
                send_close_with(&mut sender, ServerMsg::Error { message: "bad password".into() })
                    .await;
            return;
        }
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
                g.seats.push((s, out_tx.clone()));
                eprintln!("[server][{}] seated {:?} ({}/2)", room_id, s, g.seats.len());
                let summary = g.summary(&room_id);
                drop(g);
                refresh_summary(&app, &room_id, summary).await;
                s
            }
            None => {
                drop(g);
                let _ =
                    send_close_with(&mut sender, ServerMsg::Error { message: "room full".into() })
                        .await;
                return;
            }
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

    // Read loop.
    while let Some(frame) = receiver.next().await {
        let frame = match frame {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[server][warn][{}] ws read error ({:?}): {e}", room_id, seat);
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
        // Get the room (it might have been GC'd in pathological races, but
        // since we hold a seat that can only happen after our own retain,
        // not before — so the lookup is virtually always Some).
        let room = match get_room(&app, &room_id).await {
            Some(r) => r,
            None => break,
        };
        let summary_after = {
            let mut g = room.lock().await;
            process_client_msg(&mut g, seat, msg, &room_id);
            g.summary(&room_id)
        };
        refresh_summary(&app, &room_id, summary_after).await;
    }

    // Disconnect: remove seat, notify peer if game was live, GC empty rooms
    // except "main".
    eprintln!("[server][{}] {:?} disconnected", room_id, seat);
    if let Some(room) = get_room(&app, &room_id).await {
        let (summary_after, room_now_empty) = {
            let mut g = room.lock().await;
            g.seats.retain(|(s, _)| *s != seat);
            // Drop any rematch flag the leaver had set.
            g.rematch.retain(|s| *s != seat);
            if matches!(g.state.status, GameStatus::Ongoing) && !g.seats.is_empty() {
                for (_, tx) in &g.seats {
                    let _ = tx.send(ServerMsg::Error { message: "opponent disconnected".into() });
                }
            }
            let empty = g.seats.is_empty();
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

fn process_client_msg(g: &mut RoomState, seat: Side, msg: ClientMsg, room_id: &str) {
    match msg {
        ClientMsg::Rematch => process_rematch(g, seat, room_id),
        ClientMsg::Move { mv } => {
            if !matches!(g.state.status, GameStatus::Ongoing) {
                send_to(
                    g,
                    seat,
                    ServerMsg::Error {
                        message: "game is over — press 'n' to request a rematch".into(),
                    },
                );
                return;
            }
            process_move(g, seat, mv, room_id);
        }
        ClientMsg::Resign => {
            if !matches!(g.state.status, GameStatus::Ongoing) {
                send_to(g, seat, ServerMsg::Error { message: "game is over".into() });
                return;
            }
            let winner = if seat == Side::RED { Side::BLACK } else { Side::RED };
            g.state.status = GameStatus::Won { winner, reason: WinReason::Resignation };
            broadcast_update(g);
        }
        ClientMsg::ListRooms => {
            send_to(
                g,
                seat,
                ServerMsg::Error {
                    message: "ListRooms is a lobby-only message; subscribe via /lobby".into(),
                },
            );
        }
    }
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
}
