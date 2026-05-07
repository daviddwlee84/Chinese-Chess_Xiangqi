//! End-to-end smoke: boot a server on an ephemeral port, sync `tungstenite`
//! clients connect, exchange messages, validate the full loop including the
//! multi-room + lobby + password additions.

use std::time::Duration;

use anyhow::Result;
use chess_core::piece::Side;
use chess_core::rules::RuleSet;
use chess_net::{ClientMsg, RoomStatus, ServerMsg};
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

fn set_nonblocking(
    ws: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
) -> Result<()> {
    match ws.get_mut() {
        tungstenite::stream::MaybeTlsStream::Plain(tcp) => Ok(tcp.set_nonblocking(true)?),
        _ => Ok(()),
    }
}

/// Drain the v3 `ChatHistory` frame that the server sends right after
/// every `Hello`. Tests written against the v2 protocol never expected
/// this, so each `read_json(...) // Hello` site grew a follow-up call to
/// this helper.
fn drain_chat_history<S: std::io::Read + std::io::Write>(
    ws: &mut tungstenite::WebSocket<S>,
) -> Result<()> {
    match read_json(ws)? {
        ServerMsg::ChatHistory { .. } => Ok(()),
        other => anyhow::bail!("expected ChatHistory after Hello, got {other:?}"),
    }
}

/// Drain Rooms pushes until one matches the predicate, or we hit a
/// reasonable read budget. The lobby socket can receive several stale
/// snapshots back-to-back during fast room churn (each refresh_summary
/// triggers its own push); the test cares about *eventually consistent*
/// state, not every intermediate frame.
fn read_rooms_until<S, F>(
    ws: &mut tungstenite::WebSocket<S>,
    mut pred: F,
) -> Result<Vec<chess_net::RoomSummary>>
where
    S: std::io::Read + std::io::Write,
    F: FnMut(&[chess_net::RoomSummary]) -> bool,
{
    for _ in 0..16 {
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
    anyhow::bail!("predicate never matched within 16 lobby pushes")
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
        drain_chat_history(&mut red)?;

        let (mut black, _) = tungstenite::connect(&url)?;
        let black_hello = read_json(&mut black)?;
        match &black_hello {
            ServerMsg::Hello { observer, .. } => {
                assert_eq!(*observer, Side::BLACK, "second connect must be BLACK");
            }
            other => panic!("expected Hello, got {other:?}"),
        }
        drain_chat_history(&mut black)?;

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
        drain_chat_history(&mut red)?;
        let (mut black, _) = tungstenite::connect(&url)?;
        let _ = read_json(&mut black)?; // Hello
        drain_chat_history(&mut black)?;

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
        drain_chat_history(&mut red)?;
        let (mut black, _) = tungstenite::connect(&url)?;
        let _ = read_json(&mut black)?;
        drain_chat_history(&mut black)?;

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
        drain_chat_history(&mut a)?;
        let (mut b, _) = tungstenite::connect(&url)?;
        let _ = read_json(&mut b)?;
        drain_chat_history(&mut b)?;
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

// --- Lobby + multi-room + password tests --------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn lobby_lists_empty_initially() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let lobby_url = format!("ws://{addr}/lobby");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut lobby, _) = tungstenite::connect(&lobby_url)?;
        let rooms = read_rooms_until(&mut lobby, |_| true)?;
        assert!(rooms.is_empty(), "fresh server should report no rooms, got {rooms:?}");
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn lobby_sees_room_after_join() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let lobby_url = format!("ws://{addr}/lobby");
    let game_url = format!("ws://{addr}/ws/foo");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut lobby, _) = tungstenite::connect(&lobby_url)?;
        let _ = read_rooms_until(&mut lobby, |_| true)?; // initial empty snapshot

        let (mut red, _) = tungstenite::connect(&game_url)?;
        let _ = read_json(&mut red)?; // Hello
        drain_chat_history(&mut red)?;

        let rooms = read_rooms_until(&mut lobby, |rs| rs.iter().any(|r| r.id == "foo"))?;
        let foo = rooms.iter().find(|r| r.id == "foo").unwrap();
        assert_eq!(foo.seats, 1);
        assert_eq!(foo.status, RoomStatus::Lobby);
        assert!(!foo.has_password);
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn lobby_sees_seat_fill_and_finish() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let lobby_url = format!("ws://{addr}/lobby");
    let game_url = format!("ws://{addr}/ws/foo");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut lobby, _) = tungstenite::connect(&lobby_url)?;
        let _ = read_rooms_until(&mut lobby, |_| true)?;

        let (mut red, _) = tungstenite::connect(&game_url)?;
        let _ = read_json(&mut red)?;
        drain_chat_history(&mut red)?;
        let (mut black, _) = tungstenite::connect(&game_url)?;
        let _ = read_json(&mut black)?;
        drain_chat_history(&mut black)?;

        let rooms = read_rooms_until(&mut lobby, |rs| {
            rs.iter().any(|r| r.id == "foo" && r.seats == 2 && r.status == RoomStatus::Playing)
        })?;
        assert!(rooms.iter().any(|r| r.id == "foo"));

        send_json(&mut red, &ClientMsg::Resign)?;
        // Drain the resignation Updates from both game sockets first so
        // they don't pile up.
        assert!(matches!(read_json(&mut red)?, ServerMsg::Update { .. }));
        assert!(matches!(read_json(&mut black)?, ServerMsg::Update { .. }));

        let _ = read_rooms_until(&mut lobby, |rs| {
            rs.iter().any(|r| r.id == "foo" && r.status == RoomStatus::Finished)
        })?;
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn password_rejected_on_wrong() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let red_url = format!("ws://{addr}/ws/locked?password=alpha");
    let bad_url = format!("ws://{addr}/ws/locked?password=beta");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&red_url)?;
        let red_hello = read_json(&mut red)?;
        assert!(matches!(red_hello, ServerMsg::Hello { .. }), "got {red_hello:?}");
        drain_chat_history(&mut red)?;

        let (mut bad, _) = tungstenite::connect(&bad_url)?;
        match read_json(&mut bad)? {
            ServerMsg::Error { message } => {
                assert!(message.contains("bad password"), "got: {message}")
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
async fn password_accepted_on_right() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws/locked?password=alpha");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        match read_json(&mut red)? {
            ServerMsg::Hello { observer, .. } => assert_eq!(observer, Side::RED),
            other => panic!("expected Hello, got {other:?}"),
        }
        drain_chat_history(&mut red)?;
        let (mut black, _) = tungstenite::connect(&url)?;
        match read_json(&mut black)? {
            ServerMsg::Hello { observer, .. } => assert_eq!(observer, Side::BLACK),
            other => panic!("expected Hello, got {other:?}"),
        }
        drain_chat_history(&mut black)?;
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn two_rooms_isolated_no_crosstalk() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut foo_red, _) = tungstenite::connect(format!("ws://{addr}/ws/foo"))?;
        let foo_hello = read_json(&mut foo_red)?;
        let mv = match foo_hello {
            ServerMsg::Hello { view, .. } => view.legal_moves[0].clone(),
            other => panic!("expected Hello, got {other:?}"),
        };
        drain_chat_history(&mut foo_red)?;
        let (mut foo_black, _) = tungstenite::connect(format!("ws://{addr}/ws/foo"))?;
        let _ = read_json(&mut foo_black)?;
        drain_chat_history(&mut foo_black)?;

        let (mut bar_red, _) = tungstenite::connect(format!("ws://{addr}/ws/bar"))?;
        let _ = read_json(&mut bar_red)?;
        drain_chat_history(&mut bar_red)?;
        let (mut bar_black, _) = tungstenite::connect(format!("ws://{addr}/ws/bar"))?;
        let _ = read_json(&mut bar_black)?;
        drain_chat_history(&mut bar_black)?;

        send_json(&mut foo_red, &ClientMsg::Move { mv })?;
        // Both foo seats see the Update.
        assert!(matches!(read_json(&mut foo_red)?, ServerMsg::Update { .. }));
        assert!(matches!(read_json(&mut foo_black)?, ServerMsg::Update { .. }));

        // bar sockets must NOT see anything within the next ~150ms.
        set_nonblocking(&mut bar_red)?;
        set_nonblocking(&mut bar_black)?;
        std::thread::sleep(Duration::from_millis(150));
        match bar_red.read() {
            Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            other => panic!("bar_red saw cross-room traffic: {other:?}"),
        }
        match bar_black.read() {
            Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            other => panic!("bar_black saw cross-room traffic: {other:?}"),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn room_gc_after_last_seat_leaves() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let lobby_url = format!("ws://{addr}/lobby");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut lobby, _) = tungstenite::connect(&lobby_url)?;
        let _ = read_rooms_until(&mut lobby, |_| true)?;

        let (mut temp, _) = tungstenite::connect(format!("ws://{addr}/ws/temp"))?;
        let _ = read_json(&mut temp)?;
        drain_chat_history(&mut temp)?;

        let _ = read_rooms_until(&mut lobby, |rs| rs.iter().any(|r| r.id == "temp"))?;

        // Disconnect → room should be GC'd (it's not "main").
        drop(temp);
        let rooms = read_rooms_until(&mut lobby, |rs| !rs.iter().any(|r| r.id == "temp"))?;
        assert!(!rooms.iter().any(|r| r.id == "temp"));
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn default_room_main_persists_after_empty() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let lobby_url = format!("ws://{addr}/lobby");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut lobby, _) = tungstenite::connect(&lobby_url)?;
        let _ = read_rooms_until(&mut lobby, |_| true)?;

        let (mut a, _) = tungstenite::connect(format!("ws://{addr}/ws"))?;
        let _ = read_json(&mut a)?;
        drain_chat_history(&mut a)?;
        let _ = read_rooms_until(&mut lobby, |rs| rs.iter().any(|r| r.id == "main"))?;

        drop(a);
        // After the only seat leaves, "main" must still exist (just with seats=0)
        // — never GC'd. Allow the lobby to settle, then ask for an explicit list.
        std::thread::sleep(Duration::from_millis(100));
        send_json(&mut lobby, &ClientMsg::ListRooms)?;
        let rooms =
            read_rooms_until(&mut lobby, |rs| rs.iter().any(|r| r.id == "main" && r.seats == 0))?;
        assert!(rooms.iter().any(|r| r.id == "main"));
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn default_room_back_compat() -> Result<()> {
    // Old (v1-style) clients connect to /ws (no room id) and land in "main".
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let url = format!("ws://{addr}/ws");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut a, _) = tungstenite::connect(&url)?;
        match read_json(&mut a)? {
            ServerMsg::Hello { observer, protocol, .. } => {
                assert_eq!(observer, Side::RED);
                assert_eq!(protocol, chess_net::PROTOCOL_VERSION);
            }
            other => panic!("expected Hello, got {other:?}"),
        }
        drain_chat_history(&mut a)?;
        let (mut b, _) = tungstenite::connect(&url)?;
        match read_json(&mut b)? {
            ServerMsg::Hello { observer, .. } => assert_eq!(observer, Side::BLACK),
            other => panic!("expected Hello, got {other:?}"),
        }
        drain_chat_history(&mut b)?;
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn rooms_json_endpoint_returns_snapshot() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let game_url = format!("ws://{addr}/ws/curl-test");
    let rooms_url = format!("http://{addr}/rooms");
    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&game_url)?;
        let _ = read_json(&mut red)?;
        drain_chat_history(&mut red)?;

        let body = std::thread::spawn(move || -> Result<String> {
            // Plain HTTP/1.1 GET on the same listener — easier than pulling
            // reqwest into dev-deps just for one assertion.
            use std::io::{Read, Write};
            let stream = std::net::TcpStream::connect(
                rooms_url.trim_start_matches("http://").trim_end_matches("/rooms"),
            )?;
            let mut stream = stream;
            stream.write_all(b"GET /rooms HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")?;
            let mut buf = String::new();
            stream.read_to_string(&mut buf)?;
            Ok(buf)
        })
        .join()
        .map_err(|_| anyhow::anyhow!("http thread panicked"))??;

        assert!(body.contains("curl-test"), "expected curl-test in body, got: {body}");
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}
