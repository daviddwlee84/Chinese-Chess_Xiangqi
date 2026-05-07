//! End-to-end smoke for the v3 spectator + chat additions. Boots a server
//! on an ephemeral port and drives sync `tungstenite` clients through the
//! flows that matter: spectator opt-in, capacity cap, chat broadcast,
//! ring-buffer history, and players-only gating.

use std::time::Duration;

use anyhow::Result;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_net::{ChatLine, ClientMsg, RoomStatus, ServerMsg};
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

/// Drain the welcome pair (Hello/Spectating + ChatHistory) for a fresh
/// joiner so subsequent reads see live traffic.
fn drain_welcome<S: std::io::Read + std::io::Write>(
    ws: &mut tungstenite::WebSocket<S>,
) -> Result<()> {
    let _welcome = read_json(ws)?;
    let _history = read_json(ws)?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn spectator_join_succeeds_with_role_param() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let game_url = format!("ws://{addr}/ws/spec");
    let watch_url = format!("ws://{addr}/ws/spec?role=spectator");
    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&game_url)?;
        drain_welcome(&mut red)?;

        let (mut watcher, _) = tungstenite::connect(&watch_url)?;
        match read_json(&mut watcher)? {
            ServerMsg::Spectating { protocol, .. } => {
                assert_eq!(protocol, chess_net::PROTOCOL_VERSION);
            }
            other => panic!("expected Spectating, got {other:?}"),
        }
        match read_json(&mut watcher)? {
            ServerMsg::ChatHistory { lines } => assert!(lines.is_empty()),
            other => panic!("expected ChatHistory, got {other:?}"),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn third_player_without_role_param_still_room_full() -> Result<()> {
    // Back-compat assertion: v2 clients keep getting "room full" because they
    // never set ?role=spectator. Only the explicit opt-in path upgrades to
    // spectator mode.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut a, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut a)?;
        let (mut b, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut b)?;
        let (mut c, _) = tungstenite::connect(&url)?;
        match read_json(&mut c)? {
            ServerMsg::Error { message } => assert!(message.contains("room full"), "{message}"),
            other => panic!("expected Error, got {other:?}"),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn chat_broadcasts_to_seats_and_spectators() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws/chatroom");
    let watch_url = format!("ws://{addr}/ws/chatroom?role=spectator");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut red)?;
        let (mut black, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut black)?;
        let (mut watcher, _) = tungstenite::connect(&watch_url)?;
        drain_welcome(&mut watcher)?;

        send_json(&mut red, &ClientMsg::Chat { text: "good luck".into() })?;
        for ws in [&mut red, &mut black, &mut watcher] {
            match read_json(ws)? {
                ServerMsg::Chat { line } => {
                    assert_eq!(line.from, Side::RED);
                    assert_eq!(line.text, "good luck");
                }
                other => panic!("expected Chat, got {other:?}"),
            }
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn spectator_chat_rejected_with_error() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws/silent");
    let watch_url = format!("ws://{addr}/ws/silent?role=spectator");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut red)?;
        let (mut watcher, _) = tungstenite::connect(&watch_url)?;
        drain_welcome(&mut watcher)?;

        send_json(&mut watcher, &ClientMsg::Chat { text: "hello?".into() })?;
        match read_json(&mut watcher)? {
            ServerMsg::Error { message } => {
                assert!(message.contains("spectators cannot chat"), "{message}");
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
async fn chat_history_capped_and_replayed_to_late_joiner() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws/loud");
    let watch_url = format!("ws://{addr}/ws/loud?role=spectator");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut red)?;
        let (mut black, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut black)?;

        // Send 60 lines (alternating sender). After processing, the ring
        // buffer should hold the last 50.
        for i in 0..60 {
            let sender = if i % 2 == 0 { &mut red } else { &mut black };
            send_json(sender, &ClientMsg::Chat { text: format!("msg {}", i) })?;
            // Drain the broadcast for both seats so reads stay in sync.
            assert!(matches!(read_json(&mut red)?, ServerMsg::Chat { .. }));
            assert!(matches!(read_json(&mut black)?, ServerMsg::Chat { .. }));
        }

        let (mut watcher, _) = tungstenite::connect(&watch_url)?;
        let _spec = read_json(&mut watcher)?;
        let lines: Vec<ChatLine> = match read_json(&mut watcher)? {
            ServerMsg::ChatHistory { lines } => lines,
            other => panic!("expected ChatHistory, got {other:?}"),
        };
        assert_eq!(lines.len(), 50, "ring buffer should cap at 50");
        // The first line in history should be msg 10 (indices 0..9 dropped).
        assert_eq!(lines.first().unwrap().text, "msg 10");
        assert_eq!(lines.last().unwrap().text, "msg 59");
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn spectator_capacity_enforced() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let opts = chess_net::ServeOpts::new(RuleSet::xiangqi_casual()).with_max_spectators(2);
    let server = tokio::spawn(chess_net::serve_with(listener, opts));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws/cap");
    let watch_url = format!("ws://{addr}/ws/cap?role=spectator");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut red)?;

        let (mut w1, _) = tungstenite::connect(&watch_url)?;
        drain_welcome(&mut w1)?;
        let (mut w2, _) = tungstenite::connect(&watch_url)?;
        drain_welcome(&mut w2)?;
        let (mut w3, _) = tungstenite::connect(&watch_url)?;
        match read_json(&mut w3)? {
            ServerMsg::Error { message } => {
                assert!(message.contains("watch capacity"), "{message}");
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
async fn lobby_summary_includes_spectator_count() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws/lobbyspec");
    let watch_url = format!("ws://{addr}/ws/lobbyspec?role=spectator");
    let lobby_url = format!("ws://{addr}/lobby");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut red)?;
        let (mut watcher, _) = tungstenite::connect(&watch_url)?;
        drain_welcome(&mut watcher)?;

        let (mut lobby, _) = tungstenite::connect(&lobby_url)?;
        // Drain pushes until the room shows spectators=1.
        for _ in 0..16 {
            match read_json(&mut lobby)? {
                ServerMsg::Rooms { rooms } => {
                    if let Some(r) = rooms.iter().find(|r| r.id == "lobbyspec") {
                        if r.spectators == 1 {
                            assert_eq!(r.seats, 1);
                            assert_eq!(r.status, RoomStatus::Lobby);
                            return Ok(());
                        }
                    }
                }
                other => panic!("unexpected: {other:?}"),
            }
        }
        anyhow::bail!("never saw spectators=1 in any Rooms push")
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn spectator_sees_live_updates_from_red_pov() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws/live");
    let watch_url = format!("ws://{addr}/ws/live?role=spectator");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        let red_hello = read_json(&mut red)?;
        let mv = match red_hello {
            ServerMsg::Hello { view, .. } => view.legal_moves[0].clone(),
            other => panic!("expected Hello, got {other:?}"),
        };
        let _ = read_json(&mut red)?; // ChatHistory

        let (mut black, _) = tungstenite::connect(&url)?;
        drain_welcome(&mut black)?;
        let (mut watcher, _) = tungstenite::connect(&watch_url)?;
        drain_welcome(&mut watcher)?;

        send_json(&mut red, &ClientMsg::Move { mv })?;
        for ws in [&mut red, &mut black, &mut watcher] {
            assert!(matches!(read_json(ws)?, ServerMsg::Update { .. }));
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}
