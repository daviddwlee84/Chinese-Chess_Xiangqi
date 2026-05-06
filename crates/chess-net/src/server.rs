//! Single-room ws server. Accepts the first two clients (Red then Black),
//! validates moves through `GameState::make_move`, broadcasts per-side
//! `PlayerView` after each commit. Lobby / multi-room / reconnect / TLS
//! deferred to follow-up PRs (see `TODO.md`).

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_core::state::{GameState, GameStatus, WinReason};
use chess_core::view::PlayerView;
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use tokio::sync::{mpsc, Mutex};

use crate::protocol::{ClientMsg, ServerMsg, PROTOCOL_VERSION};

struct RoomState {
    state: GameState,
    /// (side, outbound mpsc tx). Empty until first connect; capped at 2.
    seats: Vec<(Side, mpsc::UnboundedSender<ServerMsg>)>,
    /// Sides that have requested a rematch since the last reset. Cleared
    /// every time the game resets; mutual consent triggers reset.
    rematch: Vec<Side>,
}

impl RoomState {
    fn new(rules: RuleSet) -> Self {
        Self {
            state: GameState::new(rules),
            seats: Vec::with_capacity(2),
            rematch: Vec::with_capacity(2),
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
}

type Room = Arc<Mutex<RoomState>>;

/// Bind `addr` and serve until error / SIGINT (caller's choice — we don't
/// install signal handling here). For tests that need an ephemeral port,
/// use [`serve`] directly with a pre-bound listener.
pub async fn run(addr: SocketAddr, rules: RuleSet) -> Result<()> {
    let listener =
        tokio::net::TcpListener::bind(addr).await.with_context(|| format!("bind {addr}"))?;
    eprintln!("[server] listening on ws://{}", listener.local_addr()?);
    serve(listener, rules).await
}

/// Serve on a pre-bound listener. The listener owns the port; use
/// `listener.local_addr()` before passing it in if you need to know it
/// (e.g. ephemeral ports in tests).
pub async fn serve(listener: tokio::net::TcpListener, rules: RuleSet) -> Result<()> {
    let room: Room = Arc::new(Mutex::new(RoomState::new(rules)));
    let app = Router::new().route("/", get(upgrade)).route("/ws", get(upgrade)).with_state(room);
    axum::serve(listener, app).await.context("axum::serve")?;
    Ok(())
}

async fn upgrade(State(room): State<Room>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, room))
}

async fn handle_socket(socket: WebSocket, room: Room) {
    let (mut sender, mut receiver) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<ServerMsg>();

    // Seat assignment + Hello. Held under one lock so a third connect can't
    // squeeze between assignment and seat insertion.
    let seat: Side = {
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
                g.seats.push((s, out_tx.clone()));
                eprintln!("[server] seated {:?} ({}/2)", s, g.seats.len());
                s
            }
            None => {
                let _ = sender
                    .send(Message::Text(
                        serde_json::to_string(&ServerMsg::Error { message: "room full".into() })
                            .unwrap_or_default(),
                    ))
                    .await;
                let _ = sender.close().await;
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
                eprintln!("[server][warn] ws read error ({:?}): {e}", seat);
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
        let mut g = room.lock().await;
        process_client_msg(&mut g, seat, msg);
    }

    // Disconnect: remove seat, notify peer if game was live.
    eprintln!("[server] {:?} disconnected", seat);
    {
        let mut g = room.lock().await;
        g.seats.retain(|(s, _)| *s != seat);
        if matches!(g.state.status, GameStatus::Ongoing) && !g.seats.is_empty() {
            for (_, tx) in &g.seats {
                let _ = tx.send(ServerMsg::Error { message: "opponent disconnected".into() });
            }
        }
    }

    drop(out_tx);
    let _ = write_task.await;
}

fn process_client_msg(g: &mut RoomState, seat: Side, msg: ClientMsg) {
    match msg {
        ClientMsg::Rematch => process_rematch(g, seat),
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
            process_move(g, seat, mv);
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
    }
}

fn process_rematch(g: &mut RoomState, seat: Side) {
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
        eprintln!("[server] rematch — fresh game");
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

fn process_move(g: &mut RoomState, seat: Side, mv: Move) {
    if g.state.side_to_move != seat {
        send_to(g, seat, ServerMsg::Error { message: "not your turn".into() });
        return;
    }
    match g.state.make_move(&mv) {
        Ok(()) => {
            g.state.refresh_status();
            eprintln!("[server] {:?} -> {:?}", seat, mv);
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
