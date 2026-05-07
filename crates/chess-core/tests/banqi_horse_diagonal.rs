//! 馬斜 (HORSE_DIAGONAL): horse adds 4 diagonal one-step moves; diagonal
//! captures ignore rank.

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

#[test]
fn horse_emits_diagonal_steps_only_with_flag() {
    // Without HORSE_DIAGONAL: horse at (1,1) sees only 4 orthogonal Steps.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    let h = state.board.sq(File(1), Rank(1));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    let steps_no_flag = state
        .legal_moves()
        .into_iter()
        .filter(|m| matches!(m, Move::Step { from, .. } if *from == h))
        .count();
    assert_eq!(steps_no_flag, 4);

    // With HORSE_DIAGONAL: 4 ortho + 4 diag = 8 Steps.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::HORSE_DIAGONAL, 0));
    let h = state.board.sq(File(1), Rank(1));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    let steps_with_flag = state
        .legal_moves()
        .into_iter()
        .filter(|m| matches!(m, Move::Step { from, .. } if *from == h))
        .count();
    assert_eq!(steps_with_flag, 8, "horse with diagonal should have 8 step moves");
}

#[test]
fn horse_diagonal_captures_higher_rank() {
    // Horse rank = 2, General rank = 6. Without rank-ignore, horse cannot
    // capture General. With HORSE_DIAGONAL on a diagonal target, it can.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::HORSE_DIAGONAL, 0));
    let h = state.board.sq(File(1), Rank(1));
    let g_sq = state.board.sq(File(2), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_revealed(&mut state.board, g_sq, Side::BLACK, PieceKind::General);

    let captures: Vec<_> = state
        .legal_moves()
        .into_iter()
        .filter_map(|m| match m {
            Move::Capture { from, to, captured } if from == h && to == g_sq => Some(captured),
            _ => None,
        })
        .collect();
    assert!(
        !captures.is_empty(),
        "馬斜 should let horse capture diagonally adjacent general (rank ignored)"
    );
    assert_eq!(captures[0].kind, PieceKind::General);
}

#[test]
fn horse_orthogonal_captures_still_follow_rank_rules() {
    // Without HORSE_DIAGONAL, horse vs General orthogonal: horse(2) < general(6),
    // so no capture. Confirm we didn't accidentally rank-ignore orthogonal too.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::HORSE_DIAGONAL, 0));
    let h = state.board.sq(File(1), Rank(1));
    let g_sq = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_revealed(&mut state.board, g_sq, Side::BLACK, PieceKind::General);

    let captures = state
        .legal_moves()
        .into_iter()
        .filter(|m| matches!(m, Move::Capture { from, to, .. } if *from == h && *to == g_sq))
        .count();
    assert_eq!(captures, 0, "orthogonal horse-vs-general should still be rank-blocked");
}

#[test]
fn horse_diagonal_blocked_by_own_piece() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::HORSE_DIAGONAL, 0));
    let h = state.board.sq(File(1), Rank(1));
    let mate = state.board.sq(File(2), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_revealed(&mut state.board, mate, Side::RED, PieceKind::Soldier);

    let move_to_mate = state
        .legal_moves()
        .into_iter()
        .filter(|m| matches!(m, Move::Step { from, to } | Move::Capture { from, to, .. } if *from == h && *to == mate))
        .count();
    assert_eq!(move_to_mate, 0, "own piece blocks diagonal");
}

#[test]
fn horse_diagonal_dark_capture_emitted_with_both_flags() {
    let rules = RuleSet::banqi_with_seed(HouseRules::HORSE_DIAGONAL | HouseRules::DARK_CAPTURE, 0);
    let mut state = empty_banqi(rules);
    let h = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(2), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    state
        .board
        .set(target, Some(PieceOnSquare::hidden(Piece::new(Side::BLACK, PieceKind::Soldier))));

    let dark = state
        .legal_moves()
        .into_iter()
        .find(|m| matches!(m, Move::DarkCapture { from, to, .. } if *from == h && *to == target));
    assert!(dark.is_some(), "diagonal hidden target should produce DarkCapture");
}
