//! End-to-end smoke: boot a server on an ephemeral port, two sync
//! `tungstenite` clients connect, exchange Hello, Red plays a legal move,
//! both clients next read an Update. Validates the full loop.

use std::time::Duration;

use anyhow::Result;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_net::{ClientMsg, ServerMsg};
use tungstenite::Message;

fn read_json<S: std::io::Read + std::io::Write>(
    ws: &mut tungstenite::WebSocket<S>,
) -> Result<ServerMsg> {
    let m = ws.read()?;
    let text = m.to_text()?;
    Ok(serde_json::from_str(text)?)
}

#[tokio::test(flavor = "multi_thread")]
async fn two_clients_play_one_move() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));

    // Tiny grace period to get past the bind→accept warm-up.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let url = format!("ws://{addr}/ws");
    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        let red_hello = read_json(&mut red)?;
        let mv = match &red_hello {
            ServerMsg::Hello { observer, view, .. } => {
                assert_eq!(*observer, Side::RED, "first connect must be RED");
                assert!(!view.legal_moves.is_empty(), "Red should have legal moves");
                view.legal_moves[0].clone()
            }
            other => panic!("expected Hello, got {other:?}"),
        };

        let (mut black, _) = tungstenite::connect(&url)?;
        let black_hello = read_json(&mut black)?;
        match &black_hello {
            ServerMsg::Hello { observer, .. } => {
                assert_eq!(*observer, Side::BLACK, "second connect must be BLACK");
            }
            other => panic!("expected Hello, got {other:?}"),
        }

        // Red plays a legal move; both clients should read an Update next.
        let payload = serde_json::to_string(&ClientMsg::Move { mv })?;
        red.send(Message::Text(payload))?;

        let red_update = read_json(&mut red)?;
        let black_update = read_json(&mut black)?;
        assert!(matches!(red_update, ServerMsg::Update { .. }), "got {red_update:?}");
        assert!(matches!(black_update, ServerMsg::Update { .. }), "got {black_update:?}");
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn rematch_resets_after_both_request() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        let _ = read_json(&mut red)?; // Hello
        let (mut black, _) = tungstenite::connect(&url)?;
        let _ = read_json(&mut black)?; // Hello

        // Red resigns → game over (Black wins).
        red.send(Message::Text(serde_json::to_string(&ClientMsg::Resign)?))?;
        // Both see the resignation update.
        assert!(matches!(read_json(&mut red)?, ServerMsg::Update { .. }));
        assert!(matches!(read_json(&mut black)?, ServerMsg::Update { .. }));

        // Red asks for a rematch — server should notify both sides
        // (informational Error: requester waiting / opponent prompted).
        red.send(Message::Text(serde_json::to_string(&ClientMsg::Rematch)?))?;
        match read_json(&mut red)? {
            ServerMsg::Error { message } => {
                assert!(message.contains("Waiting"), "got: {message}")
            }
            other => panic!("expected Error, got {other:?}"),
        }
        match read_json(&mut black)? {
            ServerMsg::Error { message } => {
                assert!(message.contains("Opponent wants"), "got: {message}")
            }
            other => panic!("expected Error, got {other:?}"),
        }

        // Black accepts → both receive a fresh Hello with a new GameState.
        black.send(Message::Text(serde_json::to_string(&ClientMsg::Rematch)?))?;
        let red_hello = read_json(&mut red)?;
        let black_hello = read_json(&mut black)?;
        let mv = match red_hello {
            ServerMsg::Hello { observer, view, .. } => {
                assert_eq!(observer, Side::RED);
                assert!(!view.legal_moves.is_empty(), "fresh game should have legal moves");
                view.legal_moves[0].clone()
            }
            other => panic!("expected Hello, got {other:?}"),
        };
        assert!(matches!(black_hello, ServerMsg::Hello { .. }));

        // Sanity: Red can play a move on the fresh game.
        red.send(Message::Text(serde_json::to_string(&ClientMsg::Move { mv })?))?;
        assert!(matches!(read_json(&mut red)?, ServerMsg::Update { .. }));
        assert!(matches!(read_json(&mut black)?, ServerMsg::Update { .. }));
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn rematch_rejected_while_game_in_progress() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        let _ = read_json(&mut red)?;
        let (mut black, _) = tungstenite::connect(&url)?;
        let _ = read_json(&mut black)?;

        red.send(Message::Text(serde_json::to_string(&ClientMsg::Rematch)?))?;
        match read_json(&mut red)? {
            ServerMsg::Error { message } => {
                assert!(message.contains("still in progress"), "got: {message}")
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
async fn third_client_gets_room_full() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));

    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut a, _) = tungstenite::connect(&url)?;
        let _ = read_json(&mut a)?;
        let (mut b, _) = tungstenite::connect(&url)?;
        let _ = read_json(&mut b)?;
        let (mut c, _) = tungstenite::connect(&url)?;
        match read_json(&mut c)? {
            ServerMsg::Error { message } => {
                assert!(message.contains("room full"), "expected room-full error, got {message:?}")
            }
            other => panic!("expected Error, got {other:?}"),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}
