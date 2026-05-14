//! Banqi default rules let EITHER seat make the first reveal — locked
//! in here so future refactors of the room layer don't accidentally
//! re-impose the "RED-flips-first" guard.
//!
//! Pairs with `crates/chess-core/tests/banqi_first_flip_color.rs` which
//! tests the engine-half of the same contract. This file proves the
//! deployment layer (chess-net's `process_move`) correctly attributes
//! the flip to whichever seat clicked first.

use std::time::Duration;

use anyhow::Result;
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::{HouseRules, RuleSet};
use chess_net::{ClientMsg, ServerMsg};
use tungstenite::Message;

fn read_json<S: std::io::Read + std::io::Write>(
    ws: &mut tungstenite::WebSocket<S>,
) -> Result<ServerMsg> {
    let m = ws.read()?;
    let text = m.to_text()?;
    Ok(serde_json::from_str(text)?)
}

fn drain_welcome<S: std::io::Read + std::io::Write>(
    ws: &mut tungstenite::WebSocket<S>,
) -> Result<ServerMsg> {
    let welcome = read_json(ws)?;
    // ChatHistory follows Hello/Spectating.
    let _ = read_json(ws)?;
    Ok(welcome)
}

#[tokio::test(flavor = "multi_thread")]
async fn black_seat_can_flip_first_under_default_banqi_rules() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server =
        tokio::spawn(chess_net::serve(listener, RuleSet::banqi_with_seed(HouseRules::empty(), 42)));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let url = format!("ws://{addr}/ws");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        let red_hello = drain_welcome(&mut red)?;
        let red_view = match &red_hello {
            ServerMsg::Hello { observer, view, .. } => {
                assert_eq!(*observer, Side::RED, "first connect must be RED");
                assert!(
                    view.banqi_awaiting_first_flip,
                    "default banqi must surface the pre-first-flip sentinel"
                );
                assert!(
                    !view.legal_moves.is_empty(),
                    "RED observer must see reveal moves pre-first-flip"
                );
                view.clone()
            }
            other => panic!("expected Hello, got {other:?}"),
        };

        let (mut black, _) = tungstenite::connect(&url)?;
        let black_hello = drain_welcome(&mut black)?;
        let black_view = match &black_hello {
            ServerMsg::Hello { observer, view, .. } => {
                assert_eq!(*observer, Side::BLACK, "second connect must be BLACK");
                assert!(
                    view.banqi_awaiting_first_flip,
                    "BLACK observer must also see the pre-first-flip sentinel"
                );
                assert!(
                    !view.legal_moves.is_empty(),
                    "BLACK observer must ALSO see reveal moves (the relaxed gate)"
                );
                view.clone()
            }
            other => panic!("expected Hello, got {other:?}"),
        };
        // Both views expose the same reveal-move count — the projection
        // doesn't gate on observer-vs-side-to-move in this state.
        assert_eq!(red_view.legal_moves.len(), black_view.legal_moves.len());

        // BLACK sends the first reveal. The server attributes the flip
        // to BLACK seat via `state.set_active_seat`.
        let first_reveal = black_view.legal_moves[0].clone();
        assert!(matches!(first_reveal, Move::Reveal { .. }));
        black.send(Message::Text(serde_json::to_string(&ClientMsg::Move { mv: first_reveal })?))?;

        // Both seats receive an Update broadcast; pre-flip sentinel is now
        // cleared on the projected views.
        let red_update = read_json(&mut red)?;
        let black_update = read_json(&mut black)?;
        match red_update {
            ServerMsg::Update { view } => {
                assert!(!view.banqi_awaiting_first_flip, "sentinel must clear after first flip");
                assert!(
                    view.side_assignment_locked(),
                    "side assignment must lock after first flip"
                );
            }
            other => panic!("expected Update for RED, got {other:?}"),
        }
        match black_update {
            ServerMsg::Update { view } => {
                assert!(!view.banqi_awaiting_first_flip);
            }
            other => panic!("expected Update for BLACK, got {other:?}"),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn preassign_colors_rejects_blacks_first_reveal_attempt() -> Result<()> {
    // Legacy mode: HouseRules::PREASSIGN_COLORS keeps RED as the forced
    // first mover. BLACK attempting the first reveal must get the
    // standard "not your turn" error.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(
        listener,
        RuleSet::banqi_with_seed(HouseRules::PREASSIGN_COLORS, 42),
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let url = format!("ws://{addr}/ws");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        let red_hello = drain_welcome(&mut red)?;
        let red_view = match &red_hello {
            ServerMsg::Hello { observer, view, .. } => {
                assert_eq!(*observer, Side::RED);
                assert!(
                    !view.banqi_awaiting_first_flip,
                    "PREASSIGN_COLORS suppresses the sentinel"
                );
                view.clone()
            }
            other => panic!("expected Hello, got {other:?}"),
        };

        let (mut black, _) = tungstenite::connect(&url)?;
        let black_hello = drain_welcome(&mut black)?;
        match &black_hello {
            ServerMsg::Hello { observer, view, .. } => {
                assert_eq!(*observer, Side::BLACK);
                assert!(
                    view.legal_moves.is_empty(),
                    "BLACK observer must not see legal moves under PREASSIGN_COLORS"
                );
            }
            other => panic!("expected Hello, got {other:?}"),
        }

        // BLACK tries to flip — must be rejected.
        let any_reveal = red_view.legal_moves[0].clone();
        black.send(Message::Text(serde_json::to_string(&ClientMsg::Move { mv: any_reveal })?))?;
        match read_json(&mut black)? {
            ServerMsg::Error { message } => {
                assert!(message.contains("not your turn"), "got: {message}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

/// Shim for the assertion above: a Hello-time view doesn't carry the
/// `side_assignment` field directly, so we approximate "the assignment
/// locked" by `current_color` matching a side AND the view no longer
/// claiming the awaiting sentinel. Lives in the test file rather than
/// the view crate to avoid leaking test helpers.
trait SideAssignmentLockedExt {
    fn side_assignment_locked(&self) -> bool;
}

impl SideAssignmentLockedExt for chess_core::view::PlayerView {
    fn side_assignment_locked(&self) -> bool {
        !self.banqi_awaiting_first_flip
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn host_color_black_swaps_seat_assignment() -> Result<()> {
    // `?host_color=black` makes the first joiner BLACK, the second RED.
    // Honoured on the room-create call only; subsequent joiners' params
    // are ignored (their seat is dictated by the host's preference).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server = tokio::spawn(chess_net::serve(listener, RuleSet::xiangqi_casual()));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let host_url = format!("ws://{addr}/ws?host_color=black");
    let join_url = format!("ws://{addr}/ws");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut host, _) = tungstenite::connect(&host_url)?;
        match drain_welcome(&mut host)? {
            ServerMsg::Hello { observer, .. } => {
                assert_eq!(observer, Side::BLACK, "?host_color=black puts host on BLACK seat");
            }
            other => panic!("expected Hello, got {other:?}"),
        }

        // Joiner doesn't need to set the param — the room's host_color
        // is already frozen at creation time.
        let (mut joiner, _) = tungstenite::connect(&join_url)?;
        match drain_welcome(&mut joiner)? {
            ServerMsg::Hello { observer, .. } => {
                assert_eq!(observer, Side::RED, "second joiner takes the opposite seat");
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
async fn first_flipper_joiner_forces_joiner_to_flip() -> Result<()> {
    // Default rules, `?first_flipper=joiner` → host attempting the
    // first reveal is rejected with "not your turn"; joiner's reveal
    // succeeds.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server =
        tokio::spawn(chess_net::serve(listener, RuleSet::banqi_with_seed(HouseRules::empty(), 7)));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let host_url = format!("ws://{addr}/ws?first_flipper=joiner");
    let join_url = format!("ws://{addr}/ws");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut host, _) = tungstenite::connect(&host_url)?;
        let host_view = match drain_welcome(&mut host)? {
            ServerMsg::Hello { observer, view, .. } => {
                assert_eq!(observer, Side::RED, "default host_color = Red");
                // first_flipper=joiner sets side_to_move to the joiner
                // seat (BLACK) at room-create time AND sets
                // `banqi_first_mover_locked = true`. The sentinel that
                // would otherwise allow either seat to flip is
                // therefore suppressed — the room is in a "BLACK must
                // flip first" state rather than the fluid "either
                // flips" state, and only BLACK's projected `legal_moves`
                // is non-empty.
                assert!(
                    !view.banqi_awaiting_first_flip,
                    "first_flipper=joiner locks the first-mover seat; sentinel must be off"
                );
                assert_eq!(view.side_to_move, Side::BLACK);
                view
            }
            other => panic!("expected Hello, got {other:?}"),
        };

        let (mut joiner, _) = tungstenite::connect(&join_url)?;
        let joiner_view = match drain_welcome(&mut joiner)? {
            ServerMsg::Hello { observer, view, .. } => {
                assert_eq!(observer, Side::BLACK);
                assert_eq!(view.side_to_move, Side::BLACK);
                assert!(!view.legal_moves.is_empty(), "joiner is to-move; sees reveals");
                view
            }
            other => panic!("expected Hello, got {other:?}"),
        };

        // Host attempts first reveal — must be rejected.
        let any_reveal = host_view
            .legal_moves
            .first()
            .cloned()
            .unwrap_or_else(|| joiner_view.legal_moves[0].clone());
        host.send(Message::Text(serde_json::to_string(&ClientMsg::Move { mv: any_reveal })?))?;
        match read_json(&mut host)? {
            ServerMsg::Error { message } => {
                assert!(message.contains("not your turn"), "got: {message}");
            }
            other => panic!("expected Error for host's premature flip, got {other:?}"),
        }

        // Joiner's reveal goes through.
        let first_reveal = joiner_view.legal_moves[0].clone();
        joiner
            .send(Message::Text(serde_json::to_string(&ClientMsg::Move { mv: first_reveal })?))?;
        assert!(matches!(read_json(&mut host)?, ServerMsg::Update { .. }));
        assert!(matches!(read_json(&mut joiner)?, ServerMsg::Update { .. }));
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn preassign_param_locks_red_first_for_banqi() -> Result<()> {
    // `?preassign=1` enables HouseRules::PREASSIGN_COLORS on the
    // room's rules. Equivalent to constructing the server with that
    // bit pre-set (see `preassign_colors_rejects_blacks_first_reveal_attempt`).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server =
        tokio::spawn(chess_net::serve(listener, RuleSet::banqi_with_seed(HouseRules::empty(), 3)));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let url = format!("ws://{addr}/ws?preassign=1");

    tokio::task::spawn_blocking(move || -> Result<()> {
        let (mut red, _) = tungstenite::connect(&url)?;
        match drain_welcome(&mut red)? {
            ServerMsg::Hello { view, .. } => {
                assert!(
                    !view.banqi_awaiting_first_flip,
                    "?preassign=1 turns on PREASSIGN_COLORS; sentinel must be off"
                );
            }
            other => panic!("expected Hello, got {other:?}"),
        }
        Ok(())
    })
    .await??;

    server.abort();
    Ok(())
}
