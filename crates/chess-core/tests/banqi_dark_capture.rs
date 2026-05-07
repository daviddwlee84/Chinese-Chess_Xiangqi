//! Banqi `暗吃` (DARK_CAPTURE) move-gen + apply/unapply tests.
//! Covers both the probe variant (default) and the trade variant
//! (DARK_CAPTURE_TRADE).

use chess_core::board::Board;
use chess_core::coord::{File, Rank, Square};
use chess_core::moves::Move;
use chess_core::piece::{Piece, PieceKind, PieceOnSquare, Side};
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::{GameState, SideAssignment};
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
fn dark_capture_emitted_when_flag_on_target_face_down() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::DARK_CAPTURE, 0));
    let h = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Soldier);

    let dark: Vec<Move> =
        state.legal_moves().into_iter().filter(|m| matches!(m, Move::DarkCapture { .. })).collect();
    assert!(!dark.is_empty(), "expected at least one DarkCapture move");
    assert!(dark
        .iter()
        .any(|m| matches!(m, Move::DarkCapture { from, to, .. } if *from == h && *to == target)));
}

#[test]
fn no_dark_capture_when_flag_off() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    let h = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Soldier);

    let dark =
        state.legal_moves().into_iter().filter(|m| matches!(m, Move::DarkCapture { .. })).count();
    assert_eq!(dark, 0, "no DarkCapture moves without the flag");
}

#[test]
fn dark_capture_succeeds_when_attacker_outranks() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::DARK_CAPTURE, 0));
    let h = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Soldier);

    let m = Move::DarkCapture { from: h, to: target, revealed: None, attacker: None };
    state.make_move(&m).unwrap();
    // Horse at target, h is empty.
    assert!(state.board.get(h).is_none(), "attacker moved");
    let landed = state.board.get(target).unwrap();
    assert!(landed.revealed);
    assert_eq!(landed.piece.kind, PieceKind::Horse);
    assert_eq!(landed.piece.side, Side::RED);
}

#[test]
fn dark_capture_probe_keeps_attacker_when_outranked() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::DARK_CAPTURE, 0));
    let s = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, s, Side::RED, PieceKind::Soldier);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Elephant);

    let m = Move::DarkCapture { from: s, to: target, revealed: None, attacker: None };
    state.make_move(&m).unwrap();
    // Probe: attacker stays put; target is now revealed.
    let attacker = state.board.get(s).unwrap();
    assert!(attacker.revealed);
    assert_eq!(attacker.piece.kind, PieceKind::Soldier);
    let revealed_target = state.board.get(target).unwrap();
    assert!(revealed_target.revealed, "target must be revealed after probe");
    assert_eq!(revealed_target.piece.kind, PieceKind::Elephant);
}

#[test]
fn dark_capture_trade_kills_attacker_when_outranked() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(
        HouseRules::DARK_CAPTURE | HouseRules::DARK_CAPTURE_TRADE,
        0,
    ));
    let s = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, s, Side::RED, PieceKind::Soldier);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Elephant);

    let m = Move::DarkCapture { from: s, to: target, revealed: None, attacker: None };
    state.make_move(&m).unwrap();
    // Trade: attacker dies; target stays revealed in place.
    assert!(state.board.get(s).is_none(), "trade variant: attacker must be removed");
    let target_pos = state.board.get(target).unwrap();
    assert!(target_pos.revealed);
    assert_eq!(target_pos.piece.kind, PieceKind::Elephant);
    assert_eq!(target_pos.piece.side, Side::BLACK);
}

#[test]
fn dark_capture_make_unmake_round_trip_capture_path() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::DARK_CAPTURE, 0));
    let h = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Soldier);
    let snapshot = state.clone();

    let m = Move::DarkCapture { from: h, to: target, revealed: None, attacker: None };
    state.make_move(&m).unwrap();
    state.unmake_move().unwrap();
    assert_eq!(state.board, snapshot.board);
    assert_eq!(state.side_to_move, snapshot.side_to_move);
}

#[test]
fn dark_capture_make_unmake_round_trip_probe_path() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::DARK_CAPTURE, 0));
    let s = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, s, Side::RED, PieceKind::Soldier);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Elephant);
    let snapshot = state.clone();

    let m = Move::DarkCapture { from: s, to: target, revealed: None, attacker: None };
    state.make_move(&m).unwrap();
    state.unmake_move().unwrap();
    assert_eq!(state.board, snapshot.board);
}

#[test]
fn dark_capture_make_unmake_round_trip_trade_path() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(
        HouseRules::DARK_CAPTURE | HouseRules::DARK_CAPTURE_TRADE,
        0,
    ));
    let s = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, s, Side::RED, PieceKind::Soldier);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Elephant);
    let snapshot = state.clone();

    let m = Move::DarkCapture { from: s, to: target, revealed: None, attacker: None };
    state.make_move(&m).unwrap();
    state.unmake_move().unwrap();
    assert_eq!(state.board, snapshot.board);
}

#[test]
fn cannon_does_not_emit_adjacent_dark_capture() {
    // Cannons capture only via jump-over-screen in standard banqi.
    // A 1-step DarkCapture against an adjacent face-down tile would
    // (a) bypass the jump rule and (b) always probe under the old
    // rank-check resolver. We refuse to emit it.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::DARK_CAPTURE, 0));
    let c = state.board.sq(File(1), Rank(1));
    let adj = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, c, Side::RED, PieceKind::Cannon);
    place_hidden(&mut state.board, adj, Side::BLACK, PieceKind::Soldier);

    let dark = state
        .legal_moves()
        .into_iter()
        .filter(|m| matches!(m, Move::DarkCapture { from, to, .. } if *from == c && *to == adj))
        .count();
    assert_eq!(dark, 0, "cannon must NOT emit adjacent DarkCapture");
}

#[test]
fn cannon_jump_emits_dark_capture_against_hidden_target() {
    // Cannon at file 0 rank 0, screen (any piece) at rank 1, hidden tile
    // at rank 2. The cannon may jump, and the hidden tile becomes a
    // DarkCapture target under DARK_CAPTURE.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::DARK_CAPTURE, 0));
    let c = state.board.sq(File(0), Rank(0));
    let screen = state.board.sq(File(0), Rank(1));
    let target = state.board.sq(File(0), Rank(2));
    place_revealed(&mut state.board, c, Side::RED, PieceKind::Cannon);
    place_revealed(&mut state.board, screen, Side::RED, PieceKind::Soldier);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Horse);

    let dark = state
        .legal_moves()
        .into_iter()
        .find(|m| matches!(m, Move::DarkCapture { from, to, .. } if *from == c && *to == target));
    assert!(dark.is_some(), "炮暗吃 jump-over-screen should be emitted");
}

#[test]
fn cannon_dark_capture_via_jump_always_captures_regardless_of_rank() {
    // Cannons capture any piece via jump (no rank). The dark-capture
    // path must mirror that: regardless of what the hidden target
    // turns out to be, the cannon takes it.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::DARK_CAPTURE, 0));
    let c = state.board.sq(File(0), Rank(0));
    let screen = state.board.sq(File(0), Rank(1));
    let target = state.board.sq(File(0), Rank(2));
    place_revealed(&mut state.board, c, Side::RED, PieceKind::Cannon);
    place_revealed(&mut state.board, screen, Side::RED, PieceKind::Soldier);
    // Higher-rank target — a standard rank check would probe; cannon
    // bypasses rank.
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::General);

    let mv = Move::DarkCapture { from: c, to: target, revealed: None, attacker: None };
    state.make_move(&mv).unwrap();

    assert!(state.board.get(c).is_none(), "cannon vacated origin");
    let landed = state.board.get(target).unwrap();
    assert_eq!(landed.piece.kind, PieceKind::Cannon);
    assert_eq!(landed.piece.side, Side::RED);
}

#[test]
fn cannon_dark_capture_via_jump_round_trips() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::DARK_CAPTURE, 0));
    let c = state.board.sq(File(0), Rank(0));
    let screen = state.board.sq(File(0), Rank(1));
    let target = state.board.sq(File(0), Rank(2));
    place_revealed(&mut state.board, c, Side::RED, PieceKind::Cannon);
    place_revealed(&mut state.board, screen, Side::RED, PieceKind::Soldier);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::General);
    let snapshot = state.clone();

    let mv = Move::DarkCapture { from: c, to: target, revealed: None, attacker: None };
    state.make_move(&mv).unwrap();
    state.unmake_move().unwrap();

    assert_eq!(state.board, snapshot.board);
    assert_eq!(state.side_to_move, snapshot.side_to_move);
}

#[test]
fn chariot_rush_emits_dark_capture_against_hidden_blocker_with_gap() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(
        HouseRules::CHARIOT_RUSH | HouseRules::DARK_CAPTURE,
        0,
    ));
    let c = state.board.sq(File(0), Rank(0));
    let target = state.board.sq(File(0), Rank(3));
    place_revealed(&mut state.board, c, Side::RED, PieceKind::Chariot);
    place_hidden(&mut state.board, target, Side::BLACK, PieceKind::Soldier);

    let dark = state
        .legal_moves()
        .into_iter()
        .find(|m| matches!(m, Move::DarkCapture { from, to, .. } if *from == c && *to == target));
    assert!(dark.is_some(), "車衝暗吃 should be emitted across an empty gap");
}
