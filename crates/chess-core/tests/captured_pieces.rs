//! `GameState::captured_pieces()` and `PlayerView::captured` cover the
//! sidebar "graveyard" panel. The list is computed from history every
//! time, so the same code path is responsible for `Capture`,
//! `CannonJump`, `ChainCapture` (atomic), and `DarkCapture` (probe /
//! trade / capture).

use chess_core::board::Board;
use chess_core::coord::{File, Rank, Square};
use chess_core::moves::{ChainHop, Move};
use chess_core::piece::{Piece, PieceKind, PieceOnSquare, Side};
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::{GameState, SideAssignment};
use chess_core::view::PlayerView;
use smallvec::smallvec;

fn empty_banqi(rules: RuleSet) -> GameState {
    let mut state = GameState::new(rules);
    let squares: Vec<Square> = state.board.squares().collect();
    for sq in squares {
        state.board.set(sq, None);
    }
    state.side_assignment = Some(SideAssignment { mapping: smallvec![Side::RED, Side::BLACK] });
    state
}

fn place_revealed(board: &mut Board, sq: Square, side: Side, kind: PieceKind) {
    board.set(sq, Some(PieceOnSquare::revealed(Piece::new(side, kind))));
}

fn place_hidden(board: &mut Board, sq: Square, side: Side, kind: PieceKind) {
    board.set(sq, Some(PieceOnSquare::hidden(Piece::new(side, kind))));
}

#[test]
fn fresh_state_has_no_captured_pieces() {
    let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 1));
    assert!(state.captured_pieces().is_empty());
    assert!(PlayerView::project(&state, Side::RED).captured.is_empty());
}

#[test]
fn fresh_xiangqi_has_no_captured_pieces() {
    let state = GameState::new(RuleSet::xiangqi());
    assert!(state.captured_pieces().is_empty());
}

#[test]
fn single_capture_records_the_dead_piece() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    let h = state.board.sq(File(0), Rank(1));
    let target = state.board.sq(File(0), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_revealed(&mut state.board, target, Side::BLACK, PieceKind::Soldier);

    let captured = Piece::new(Side::BLACK, PieceKind::Soldier);
    state.make_move(&Move::Capture { from: h, to: target, captured }).unwrap();

    assert_eq!(state.captured_pieces(), vec![captured]);
}

#[test]
fn chain_capture_records_each_hop_in_path_order() {
    // Atomic ChainCapture variant — three hops, each a different piece.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    let from = state.board.sq(File(0), Rank(0));
    let h1 = state.board.sq(File(0), Rank(1));
    let h2 = state.board.sq(File(0), Rank(2));
    let h3 = state.board.sq(File(0), Rank(3));
    place_revealed(&mut state.board, from, Side::RED, PieceKind::Chariot);
    place_revealed(&mut state.board, h1, Side::BLACK, PieceKind::Soldier);
    place_revealed(&mut state.board, h2, Side::BLACK, PieceKind::Cannon);
    place_revealed(&mut state.board, h3, Side::BLACK, PieceKind::Horse);

    let path = smallvec![
        ChainHop { to: h1, captured: Piece::new(Side::BLACK, PieceKind::Soldier) },
        ChainHop { to: h2, captured: Piece::new(Side::BLACK, PieceKind::Cannon) },
        ChainHop { to: h3, captured: Piece::new(Side::BLACK, PieceKind::Horse) },
    ];
    state.make_move(&Move::ChainCapture { from, path }).unwrap();

    let captured = state.captured_pieces();
    assert_eq!(captured.len(), 3);
    assert_eq!(captured[0].kind, PieceKind::Soldier);
    assert_eq!(captured[1].kind, PieceKind::Cannon);
    assert_eq!(captured[2].kind, PieceKind::Horse);
    assert!(captured.iter().all(|p| p.side == Side::BLACK));
}

#[test]
fn cannon_jump_records_the_captured_piece() {
    // Banqi 4×8 board is fine — Move::CannonJump apply just consults
    // squares, not the variant.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    let cannon = state.board.sq(File(0), Rank(0));
    let screen = state.board.sq(File(0), Rank(1));
    let target = state.board.sq(File(0), Rank(2));
    place_revealed(&mut state.board, cannon, Side::RED, PieceKind::Cannon);
    place_revealed(&mut state.board, screen, Side::RED, PieceKind::Soldier);
    place_revealed(&mut state.board, target, Side::BLACK, PieceKind::Horse);

    let captured = Piece::new(Side::BLACK, PieceKind::Horse);
    state.make_move(&Move::CannonJump { from: cannon, to: target, screen, captured }).unwrap();

    assert_eq!(state.captured_pieces(), vec![captured]);
}

#[test]
fn dark_capture_with_capture_outcome_records_revealed_defender() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::DARK_CAPTURE, 0));
    let h = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Soldier);

    state
        .make_move(&Move::DarkCapture { from: h, to: target, revealed: None, attacker: None })
        .unwrap();

    let captured = state.captured_pieces();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0], Piece::new(Side::BLACK, PieceKind::Soldier));
}

#[test]
fn dark_capture_probe_records_no_captured_piece() {
    // Soldier attacks hidden Elephant → probe (no DARK_CAPTURE_TRADE flag).
    // Both pieces stay; nothing dies; graveyard stays empty.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::DARK_CAPTURE, 0));
    let s = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, s, Side::RED, PieceKind::Soldier);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Elephant);

    state
        .make_move(&Move::DarkCapture { from: s, to: target, revealed: None, attacker: None })
        .unwrap();

    assert!(state.captured_pieces().is_empty());
}

#[test]
fn dark_capture_trade_records_dead_attacker() {
    // Same probe setup but with DARK_CAPTURE_TRADE flag → attacker dies.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(
        HouseRules::DARK_CAPTURE | HouseRules::DARK_CAPTURE_TRADE,
        0,
    ));
    let s = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, s, Side::RED, PieceKind::Soldier);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Elephant);

    state
        .make_move(&Move::DarkCapture { from: s, to: target, revealed: None, attacker: None })
        .unwrap();

    let captured = state.captured_pieces();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0], Piece::new(Side::RED, PieceKind::Soldier));
}

#[test]
fn order_is_chronological_across_mixed_move_types() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    let r1 = state.board.sq(File(0), Rank(0));
    let r2 = state.board.sq(File(0), Rank(1));
    let r3 = state.board.sq(File(2), Rank(2));
    let b1 = state.board.sq(File(1), Rank(0));
    let b2 = state.board.sq(File(2), Rank(1));
    place_revealed(&mut state.board, r1, Side::RED, PieceKind::Horse);
    place_revealed(&mut state.board, r2, Side::RED, PieceKind::Chariot);
    place_revealed(&mut state.board, r3, Side::RED, PieceKind::Cannon);
    place_revealed(&mut state.board, b1, Side::BLACK, PieceKind::Soldier);
    place_revealed(&mut state.board, b2, Side::BLACK, PieceKind::Horse);

    // 1. Red horse takes Black soldier.
    let cap_b1 = Piece::new(Side::BLACK, PieceKind::Soldier);
    state.make_move(&Move::Capture { from: r1, to: b1, captured: cap_b1 }).unwrap();
    // After Red moves the seat advances to BLACK.
    state.side_to_move = Side::BLACK;
    state.turn_order.current = 1;
    // 2. Black horse takes Red chariot.
    let cap_r2 = Piece::new(Side::RED, PieceKind::Chariot);
    state.make_move(&Move::Capture { from: b2, to: r2, captured: cap_r2 }).unwrap();
    state.side_to_move = Side::RED;
    state.turn_order.current = 0;
    // 3. Red cannon-jumps Black horse (now at r2 from step 2) using r1's
    //    horse as screen.
    let cap_b2 = Piece::new(Side::BLACK, PieceKind::Horse);
    state.make_move(&Move::CannonJump { from: r3, to: r2, screen: b1, captured: cap_b2 }).unwrap();

    let captured = state.captured_pieces();
    assert_eq!(captured, vec![cap_b1, cap_r2, cap_b2], "chronological order preserved");
}

#[test]
fn unmake_removes_only_the_last_capture() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    let from = state.board.sq(File(0), Rank(0));
    let h1 = state.board.sq(File(0), Rank(1));
    let h2 = state.board.sq(File(0), Rank(2));
    place_revealed(&mut state.board, from, Side::RED, PieceKind::Chariot);
    place_revealed(&mut state.board, h1, Side::BLACK, PieceKind::Soldier);
    place_revealed(&mut state.board, h2, Side::BLACK, PieceKind::Cannon);

    // Two-hop atomic chain — undoing once removes BOTH dead pieces.
    let path = smallvec![
        ChainHop { to: h1, captured: Piece::new(Side::BLACK, PieceKind::Soldier) },
        ChainHop { to: h2, captured: Piece::new(Side::BLACK, PieceKind::Cannon) },
    ];
    state.make_move(&Move::ChainCapture { from, path }).unwrap();
    assert_eq!(state.captured_pieces().len(), 2);
    state.unmake_move().unwrap();
    assert!(state.captured_pieces().is_empty());
}

#[test]
fn player_view_exposes_captured_field_with_same_data() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    let h = state.board.sq(File(0), Rank(1));
    let target = state.board.sq(File(0), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_revealed(&mut state.board, target, Side::BLACK, PieceKind::Soldier);

    let captured = Piece::new(Side::BLACK, PieceKind::Soldier);
    state.make_move(&Move::Capture { from: h, to: target, captured }).unwrap();

    // Same list for both observers — capture history is public.
    let red_view = PlayerView::project(&state, Side::RED);
    let black_view = PlayerView::project(&state, Side::BLACK);
    assert_eq!(red_view.captured, vec![captured]);
    assert_eq!(black_view.captured, vec![captured]);
}

#[test]
fn captured_field_serde_roundtrips() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    let h = state.board.sq(File(0), Rank(1));
    let target = state.board.sq(File(0), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_revealed(&mut state.board, target, Side::BLACK, PieceKind::Soldier);
    let captured = Piece::new(Side::BLACK, PieceKind::Soldier);
    state.make_move(&Move::Capture { from: h, to: target, captured }).unwrap();

    let view = PlayerView::project(&state, Side::RED);
    let json = serde_json::to_string(&view).unwrap();
    let back: PlayerView = serde_json::from_str(&json).unwrap();
    assert_eq!(back.captured, view.captured);

    // Older clients without the `captured` field still deserialize cleanly.
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value.as_object_mut().unwrap().remove("captured");
    let trimmed = serde_json::to_string(&value).unwrap();
    let view_v5: PlayerView = serde_json::from_str(&trimmed).unwrap();
    assert!(view_v5.captured.is_empty(), "missing field defaults to empty Vec");
}
