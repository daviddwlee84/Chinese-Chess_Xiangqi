//! Banqi house-rule scenario tests.
//! Constructs hand-crafted positions to exercise CHAIN_CAPTURE and CHARIOT_RUSH.

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

fn place(board: &mut Board, sq: Square, side: Side, kind: PieceKind) {
    board.set(sq, Some(PieceOnSquare::revealed(Piece::new(side, kind))));
}

#[test]
fn chain_capture_three_in_a_row() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    let h = state.board.sq(File(0), Rank(0));
    let s1 = state.board.sq(File(0), Rank(1));
    let s2 = state.board.sq(File(0), Rank(2));
    let s3 = state.board.sq(File(0), Rank(3));
    place(&mut state.board, h, Side::RED, PieceKind::Horse);
    place(&mut state.board, s1, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, s2, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, s3, Side::BLACK, PieceKind::Soldier);

    let chains: Vec<Move> = state
        .legal_moves()
        .into_iter()
        .filter(|m| matches!(m, Move::ChainCapture { .. }))
        .collect();

    // Should have chains of length 2 and 3.
    let lengths: Vec<usize> = chains
        .iter()
        .filter_map(|m| match m {
            Move::ChainCapture { path, .. } => Some(path.len()),
            _ => None,
        })
        .collect();
    assert!(lengths.contains(&2), "expected length-2 chain");
    assert!(lengths.contains(&3), "expected length-3 chain");
}

#[test]
fn chain_capture_blocked_by_outranked_target() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    // Red soldier at a0, black soldier at a1 (capturable, equal rank),
    // black general at a2 (NOT capturable... wait, soldier beats general).
    // Use elephant instead — soldier(0) cannot capture elephant(4).
    let s = state.board.sq(File(0), Rank(0));
    let t1 = state.board.sq(File(0), Rank(1));
    let t2 = state.board.sq(File(0), Rank(2));
    place(&mut state.board, s, Side::RED, PieceKind::Soldier);
    place(&mut state.board, t1, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, t2, Side::BLACK, PieceKind::Elephant);

    let chains: Vec<Move> = state
        .legal_moves()
        .into_iter()
        .filter(|m| matches!(m, Move::ChainCapture { .. }))
        .collect();
    // The chain stops at the elephant — soldier can't take it.
    assert!(chains.is_empty(), "chain should not extend past outranked target; got {chains:?}");
}

#[test]
fn chariot_rush_emits_long_slides_along_file() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHARIOT_RUSH, 0));
    let c = state.board.sq(File(0), Rank(0));
    place(&mut state.board, c, Side::RED, PieceKind::Chariot);
    let moves = state.legal_moves();
    // Without a blocker, chariot can reach any of 7 squares N + 3 squares E
    // = 10 destinations (1-step also counted). Expect at least 8 Step moves.
    let steps = moves.iter().filter(|m| matches!(m, Move::Step { .. })).count();
    assert!(steps >= 8, "chariot rush should produce many slides; got {steps}");
}

#[test]
fn chain_capture_round_trip_via_make_unmake() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    let h = state.board.sq(File(1), Rank(1));
    let s1 = state.board.sq(File(1), Rank(2));
    let s2 = state.board.sq(File(1), Rank(3));
    place(&mut state.board, h, Side::RED, PieceKind::Horse);
    place(&mut state.board, s1, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, s2, Side::BLACK, PieceKind::Soldier);

    let snapshot = state.clone();
    let chain = state
        .legal_moves()
        .into_iter()
        .find(|m| matches!(m, Move::ChainCapture { path, .. } if path.len() == 2))
        .expect("chain should be available");

    state.make_move(&chain).unwrap();
    state.unmake_move().unwrap();
    assert_eq!(state.board, snapshot.board);
    assert_eq!(state.side_to_move, snapshot.side_to_move);
}
