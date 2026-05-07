//! Edge cases around the create / leave / rejoin / GC lifecycle. Locks
//! in the current "no host, room is just a name + state" model so future
//! refactors don't accidentally leak old room state across GC, drop the
//! wrong rematch flag, or forget to notify the opponent on a mid-game
//! disconnect.
//!
//! Pairs with `server_smoke.rs` (basic happy paths) and
//! `spectator_chat.rs` (v3 protocol surface). The split keeps each file
//! readable; some overlap with existing tests is intentional — these are
//! the lifecycle-edge fixtures.

use std::time::Duration;

use anyhow::Result;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_net::{ChatLine, ClientMsg, RoomStatus, RoomSummary, ServerMsg};
use tungstenite::Message;

fn read_json<S: std::io::Read + std::io::Write>(
    ws: &mut tungstenite::WebSocket<S>,
) -> Result<ServerMsg> {
    let m = ws.read()?;
    let text = m.to_text()?;
    Ok(serde_json::from_str(text)?)
}

fn send_json<S: std::io::Read + std::io::Write>(
    ws: &mut tungstenite::WebSocket<S>,
    msg: &ClientMsg,
) -> Result<()> {
    ws.send(Message::Text(serde_json::to_string(msg)?))?;
    Ok(())
}

/// Drain `Hello` (or `Spectating`) + `ChatHistory` so subsequent reads
/// see live traffic. Returns the chat-history lines for tests that care.
fn drain_welcome<S: std::io::Read + std::io::Write>(
    ws: &mut tungstenite::WebSocket<S>,
) -> Result<Vec<ChatLine>> {
    let _welcome = read_json(ws)?;
    match read_json(ws)? {
        ServerMsg::ChatHistory { lines } => Ok(lines),
        other => anyhow::bail!("expected ChatHistory, got {other:?}"),
    }
}

/// Drain `Rooms` pushes from a lobby socket until `pred` matches. Mirrors
/// `server_smoke::read_rooms_until` — the lobby push is eventually
/// consistent and several intermediate snapshots can land back-to-back
/// during fast churn.
fn read_rooms_until<S, F>(
    ws: &mut tungstenite::WebSocket<S>,
    mut pred: F,
) -> Result<Vec<RoomSummary>>
where
    S: std::io::Read + std::io::Write,
    F: FnMut(&[RoomSummary]) -> bool,
{
    for _ in 0..32 {
        match read_json(ws)? {
            ServerMsg::Rooms { rooms } => {
                if pred(&rooms) {
                    return Ok(rooms);
                }
            }
            ServerMsg::Error { message } => {
                anyhow::bail!("unexpected error on lobby socket: {message}")
            }
            other => anyhow::bail!("expected Rooms on lobby socket, got {other:?}"),
        }
    }
    anyhow::bail!("predicate never matched within 32 lobby pushes")
}

/// Read `ServerMsg`s from a player socket until `pred` matches one. The
/// server can interleave its own `Update` / `Error` frames with the one
/// the test cares about; this helper drains until the predicate fires
/// (or we hit a sane budget).
fn read_msgs_until<S, F>(ws: &mut tungstenite::WebSocket<S>, mut pred: F) -> Result<ServerMsg>
where
    S: std::io::Read + std::io::Write,
    F: FnMut(&ServerMsg) -> bool,
{
    for _ in 0..16 {
        let msg = read_json(ws)?;
        if pred(&msg) {
            return Ok(msg);
        }
    }
    anyhow::bail!("predicate never matched within 16 frames")
}

#[tokio::test(flavor = "multi_thread")]
async fn fresh_joiner_after_gc_gets_clean_room() -> Result<()> {
    // Red joins a non-`main` room, leaves; the room is GC'd. A new Red
    // arriving at the same id must see a brand-new RoomState (history
    // empty, chat empty, no spectator carry-over). Catches a regression
    // where the old room's Arc somehow survived the rooms-map remove.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let url = format!("ws://{addr}/ws/scratch");

        // First lifetime: send a chat line so we have observable state.
        let (mut a, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut a)?;
        send_json(&mut a, &ClientMsg::Chat { text: "echo before gc".into() })?;
        assert!(matches!(read_json(&mut a)?, ServerMsg::Chat { .. }));
        drop(a);

        // Allow the server's disconnect handler + GC to settle. Without
        // this, the next connect can race ahead of the rooms-map remove.
        std::thread::sleep(Duration::from_millis(100));

        // Second lifetime: same id, but a brand-new room. ChatHistory
        // must be empty (state didn't survive the GC).
        let (mut b, _) = tungstenite::connect(&url)?;
        let history = drain_welcome(&mut b)?;
        assert!(history.is_empty(), "expected fresh chat history, got {history:?}");
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn locked_room_gced_then_recreated_with_different_password() -> Result<()> {
    // First lifetime locks `secret` with password "alpha"; the wrong
    // password is rejected. After GC, the same id must be openable with
    // a different password — the lock dies with the room.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let alpha_url = format!("ws://{addr}/ws/secret?password=alpha");
        let beta_url = format!("ws://{addr}/ws/secret?password=beta");

        let (mut a, _) = tungstenite::connect(&alpha_url)?;
        assert!(matches!(read_json(&mut a)?, ServerMsg::Hello { .. }));
        // Drain ChatHistory.
        let _ = read_json(&mut a)?;

        // Wrong password while the room is alive → "bad password".
        let (mut bad, _) = tungstenite::connect(&beta_url)?;
        match read_json(&mut bad)? {
            ServerMsg::Error { message } => assert!(message.contains("bad password"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        drop(bad);

        // Drop the only seat so the room GC's.
        drop(a);
        std::thread::sleep(Duration::from_millis(100));

        // Same id, different password — must succeed (the original lock
        // died with the room).
        let (mut b, _) = tungstenite::connect(&beta_url)?;
        match read_json(&mut b)? {
            ServerMsg::Hello { observer, .. } => assert_eq!(observer, Side::RED),
            other => panic!("expected Hello, got {other:?}"),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn mid_game_disconnect_pushes_error_to_opponent() -> Result<()> {
    // Red joins, Black joins, Red disconnects mid-Ongoing. The remaining
    // seat (Black) must receive `Error{"opponent disconnected"}` before
    // the room either GC's or a new joiner takes the empty seat.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let url = format!("ws://{addr}/ws/midgame");
        let (mut red, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut red)?;
        let (mut black, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut black)?;

        drop(red);
        let err = read_msgs_until(&mut black, |msg| {
            matches!(msg, ServerMsg::Error { message } if message.contains("opponent disconnected"))
        })?;
        match err {
            ServerMsg::Error { message } => {
                assert!(message.contains("opponent disconnected"), "{message}")
            }
            _ => unreachable!(),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn reconnect_after_partial_leave_takes_open_seat() -> Result<()> {
    // Red+Black seated, Black disconnects. A new joiner to the same room
    // must be assigned BLACK (the open seat), not RED. This is the
    // "reconnect rejoins as the same color" UX, accidentally — there's
    // no token, the assignment just falls out of next_seat() seeing
    // Red occupied.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let url = format!("ws://{addr}/ws/rejoin");
        let (mut red, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut red)?;
        let (mut black, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut black)?;

        drop(black);
        // Drain the "opponent disconnected" notification on Red so it
        // doesn't queue ahead of subsequent reads in pathological races.
        let _ = read_msgs_until(&mut red, |m| matches!(m, ServerMsg::Error { .. }))?;

        // Tiny delay so the seat removal lands before the new connect.
        std::thread::sleep(Duration::from_millis(50));

        let (mut rejoin, _) = tungstenite::connect(&url)?;
        match read_json(&mut rejoin)? {
            ServerMsg::Hello { observer, .. } => {
                assert_eq!(observer, Side::BLACK, "rejoiner should pick up the open seat");
            }
            other => panic!("expected Hello, got {other:?}"),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn room_persists_with_only_spectators_after_seats_leave() -> Result<()> {
    // Red+Black+spectator. Both players leave. Room must NOT GC because
    // the spectator is still attached. The lobby snapshot must show
    // seats=0, spectators=1 (and room id present). When the spectator
    // also leaves, the room finally GCs.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let game_url = format!("ws://{addr}/ws/lingering");
        let watch_url = format!("ws://{addr}/ws/lingering?role=spectator");
        let lobby_url = format!("ws://{addr}/lobby");

        let (mut red, _) = tungstenite::connect(&game_url)?;
        drain_welcome(&mut red)?;
        let (mut black, _) = tungstenite::connect(&game_url)?;
        drain_welcome(&mut black)?;
        let (mut watcher, _) = tungstenite::connect(&watch_url)?;
        drain_welcome(&mut watcher)?;

        let (mut lobby, _) = tungstenite::connect(&lobby_url)?;
        let _ = read_rooms_until(&mut lobby, |rs| {
            rs.iter().any(|r| r.id == "lingering" && r.seats == 2 && r.spectators == 1)
        })?;

        drop(red);
        drop(black);

        // Room should drop to seats=0, spectators=1 — alive only because
        // of the watcher.
        let snap = read_rooms_until(&mut lobby, |rs| {
            rs.iter().any(|r| r.id == "lingering" && r.seats == 0 && r.spectators == 1)
        })?;
        assert!(snap.iter().any(|r| r.id == "lingering"));

        drop(watcher);

        // Now the room must GC (it's not `main`).
        let snap = read_rooms_until(&mut lobby, |rs| !rs.iter().any(|r| r.id == "lingering"))?;
        assert!(!snap.iter().any(|r| r.id == "lingering"));
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn cannot_spectate_room_that_does_not_exist() -> Result<()> {
    // `?role=spectator` against a never-created room must fail with a
    // clear error rather than silently auto-creating an empty room. This
    // matches the design choice in handle_room_socket: spectators don't
    // get to summon rooms (the UX would be confusing — empty board with
    // no players).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let url = format!("ws://{addr}/ws/ghost?role=spectator");
        let (mut watcher, _) = tungstenite::connect(&url)?;
        match read_json(&mut watcher)? {
            ServerMsg::Error { message } => {
                assert!(message.contains("no such room to spectate"), "{message}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_history_survives_partial_disconnect_but_not_gc() -> Result<()> {
    // Two-stage assertion:
    //
    // 1. While someone is still in the room (player or spectator), the
    //    chat ring buffer survives any number of disconnects.
    // 2. Once the room GCs (last connection drops), the buffer is gone —
    //    a fresh joiner sees empty ChatHistory.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let game_url = format!("ws://{addr}/ws/memory");
        let watch_url = format!("ws://{addr}/ws/memory?role=spectator");

        let (mut red, _) = tungstenite::connect(&game_url)?;
        drain_welcome(&mut red)?;
        let (mut black, _) = tungstenite::connect(&game_url)?;
        drain_welcome(&mut black)?;
        let (mut watcher, _) = tungstenite::connect(&watch_url)?;
        drain_welcome(&mut watcher)?;

        // Red speaks; everyone receives the broadcast.
        send_json(&mut red, &ClientMsg::Chat { text: "first".into() })?;
        assert!(matches!(read_json(&mut red)?, ServerMsg::Chat { .. }));
        assert!(matches!(read_json(&mut black)?, ServerMsg::Chat { .. }));
        assert!(matches!(read_json(&mut watcher)?, ServerMsg::Chat { .. }));

        // Red leaves. Black + spectator keep the room alive.
        drop(red);
        let _ = read_msgs_until(&mut black, |m| matches!(m, ServerMsg::Error { .. }))?;

        // A late spectator still sees Red's message in history.
        let (mut late, _) = tungstenite::connect(&watch_url)?;
        let history = drain_welcome(&mut late)?;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "first");

        // Now drain everyone — room GCs.
        drop(black);
        drop(watcher);
        drop(late);
        std::thread::sleep(Duration::from_millis(150));

        // Fresh joiner gets a brand-new room with no chat history.
        let (mut fresh, _) = tungstenite::connect(&game_url)?;
        let history = drain_welcome(&mut fresh)?;
        assert!(history.is_empty(), "history should clear at GC, got {history:?}");
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn rematch_request_clears_when_requester_disconnects() -> Result<()> {
    // Red+Black play; Red resigns (Black wins). Red asks for a rematch
    // → server pings Black ("opponent wants rematch"). Red disconnects
    // before Black responds. The rematch flag must be cleared so a
    // future Red joiner doesn't inherit a stale "rematch pending" state.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let url = format!("ws://{addr}/ws/rematchcleanup");
        let (mut red, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut red)?;
        let (mut black, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut black)?;

        send_json(&mut red, &ClientMsg::Resign)?;
        // Both seats see the resignation update.
        assert!(matches!(read_json(&mut red)?, ServerMsg::Update { .. }));
        assert!(matches!(read_json(&mut black)?, ServerMsg::Update { .. }));

        send_json(&mut red, &ClientMsg::Rematch)?;
        // Drain the request/notify pair.
        let _ = read_msgs_until(
            &mut red,
            |m| matches!(m, ServerMsg::Error { message } if message.contains("Waiting")),
        )?;
        let _ = read_msgs_until(
            &mut black,
            |m| matches!(m, ServerMsg::Error { message } if message.contains("Opponent wants")),
        )?;

        drop(red);
        // The game is already Won{Resignation}, so the disconnect handler
        // does NOT push "opponent disconnected" (that's gated on Ongoing).
        // Just give the seat-removal + rematch-flag-cleanup a tick to land.
        std::thread::sleep(Duration::from_millis(80));

        // New Red joins. The status is still Won{Resignation} (game state
        // survives Red's disconnect because Black is still seated). Black
        // requests a rematch — without the cleanup, this would already
        // count as "both ready" because Red's stale flag persisted.
        // Instead, Black should see "Waiting…" (still pending — only one
        // side has voted now that Red's flag was cleared on disconnect).
        let (mut new_red, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut new_red)?;
        send_json(&mut black, &ClientMsg::Rematch)?;
        let msg = read_msgs_until(&mut black, |m| matches!(m, ServerMsg::Error { .. }))?;
        match msg {
            ServerMsg::Error { message } => {
                assert!(
                    message.contains("Waiting"),
                    "expected 'Waiting…' (rematch flag cleared on Red disconnect); got: {message}"
                );
            }
            _ => unreachable!(),
        }
        // new_red, on the other hand, should see "Opponent wants a rematch."
        // — because Black is the lone voter, the server pings the other seat.
        let msg = read_msgs_until(&mut new_red, |m| matches!(m, ServerMsg::Error { .. }))?;
        match msg {
            ServerMsg::Error { message } => {
                assert!(message.contains("Opponent wants"), "got: {message}");
            }
            _ => unreachable!(),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn spectator_count_drops_when_spectator_leaves() -> Result<()> {
    // Drives the lobby push: spec count goes from 0 → 1 → 0 as a
    // spectator joins and leaves while the seats stay full. Catches a
    // bug where the disconnect handler's `g.spectators.retain(...)`
    // somehow misses the same-channel match (e.g. forgets to call
    // refresh_summary after the retain).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let game_url = format!("ws://{addr}/ws/spec_drop");
        let watch_url = format!("ws://{addr}/ws/spec_drop?role=spectator");
        let lobby_url = format!("ws://{addr}/lobby");

        let (mut red, _) = tungstenite::connect(&game_url)?;
        drain_welcome(&mut red)?;
        let (mut black, _) = tungstenite::connect(&game_url)?;
        drain_welcome(&mut black)?;

        let (mut lobby, _) = tungstenite::connect(&lobby_url)?;
        let _ = read_rooms_until(&mut lobby, |rs| {
            rs.iter().any(|r| r.id == "spec_drop" && r.spectators == 0 && r.seats == 2)
        })?;

        let (mut watcher, _) = tungstenite::connect(&watch_url)?;
        drain_welcome(&mut watcher)?;
        let _ = read_rooms_until(&mut lobby, |rs| {
            rs.iter().any(|r| r.id == "spec_drop" && r.spectators == 1)
        })?;

        drop(watcher);
        let snap = read_rooms_until(&mut lobby, |rs| {
            rs.iter().any(|r| r.id == "spec_drop" && r.spectators == 0)
        })?;
        let row = snap.iter().find(|r| r.id == "spec_drop").unwrap();
        assert_eq!(row.seats, 2, "seats should be unchanged");
        assert_eq!(row.status, RoomStatus::Playing);
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn main_room_survives_full_drain_unlike_named_rooms() -> Result<()> {
    // The `main` room must NEVER GC. Drain it completely (seat + any
    // spectator) and confirm a subsequent `ListRooms` / `Rooms` push
    // still includes "main" with seats=0.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let main_url = format!("ws://{addr}/ws");
        let lobby_url = format!("ws://{addr}/lobby");

        let (mut red, _) = tungstenite::connect(&main_url)?;
        drain_welcome(&mut red)?;

        let (mut lobby, _) = tungstenite::connect(&lobby_url)?;
        let _ = read_rooms_until(&mut lobby, |rs| rs.iter().any(|r| r.id == "main"))?;

        drop(red);
        std::thread::sleep(Duration::from_millis(120));

        // Force a fresh snapshot — the auto-push after the disconnect
        // should already have fired, but ListRooms makes the test
        // deterministic.
        send_json(&mut lobby, &ClientMsg::ListRooms)?;
        let snap =
            read_rooms_until(&mut lobby, |rs| rs.iter().any(|r| r.id == "main" && r.seats == 0))?;
        assert!(snap.iter().any(|r| r.id == "main" && r.seats == 0));
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}
