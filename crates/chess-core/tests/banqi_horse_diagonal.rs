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
fn horse_diagonal_does_not_emit_non_capturing_diagonal_steps() {
    // 馬斜 only allows the horse to *capture* diagonally — it does NOT
    // allow non-capturing diagonal slides. With or without the flag,
    // a horse on an open file in an empty board has exactly 4 Step
    // moves (orthogonal only). The flag adds Capture / DarkCapture
    // emissions for diagonals when there's an enemy / hidden tile —
    // never plain Steps.
    for flags in [HouseRules::empty(), HouseRules::HORSE_DIAGONAL] {
        let mut state = empty_banqi(RuleSet::banqi_with_seed(flags, 0));
        let h = state.board.sq(File(1), Rank(1));
        place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
        let steps = state
            .legal_moves()
            .into_iter()
            .filter(|m| matches!(m, Move::Step { from, .. } if *from == h))
            .count();
        assert_eq!(
            steps, 4,
            "horse with flags={flags:?} should only emit orthogonal Steps; got {steps}"
        );
    }
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

#[test]
fn horse_diagonal_dark_capture_against_higher_rank_succeeds() {
    // 馬斜 captures any piece (rank ignored) — including diagonally
    // when the target turns out to be a higher-ranked piece. Without
    // this, soldier-attacks-elephant-via-diagonal would probe instead
    // of capturing, contradicting the user-facing rule.
    let rules = RuleSet::banqi_with_seed(HouseRules::HORSE_DIAGONAL | HouseRules::DARK_CAPTURE, 0);
    let mut state = empty_banqi(rules);
    let h = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(2), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    state
        .board
        .set(target, Some(PieceOnSquare::hidden(Piece::new(Side::BLACK, PieceKind::General))));

    let mv = Move::DarkCapture { from: h, to: target, revealed: None, attacker: None };
    state.make_move(&mv).unwrap();

    // Horse landed on target square; general was captured.
    assert!(state.board.get(h).is_none(), "horse moved");
    let landed = state.board.get(target).unwrap();
    assert_eq!(landed.piece.kind, PieceKind::Horse);
    assert_eq!(landed.piece.side, Side::RED);
}

#[test]
fn horse_diagonal_capture_can_chain_via_orthogonal_followup() {
    // 馬斜 capture activates chain mode just like an orthogonal
    // capture: after landing, if the horse has another legal capture
    // (in any direction the engine allows), chain_lock stays set.
    // Setup: horse at (1,1) takes Black general at (2,2) diagonally
    // (rank-bypassed), then from (2,2) can orthogonally take a Black
    // soldier at (2,3).
    let rules = RuleSet::banqi_with_seed(HouseRules::HORSE_DIAGONAL | HouseRules::CHAIN_CAPTURE, 0);
    let mut state = empty_banqi(rules);
    let h = state.board.sq(File(1), Rank(1));
    let diag = state.board.sq(File(2), Rank(2));
    let next = state.board.sq(File(2), Rank(3));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    place_revealed(&mut state.board, diag, Side::BLACK, PieceKind::General);
    place_revealed(&mut state.board, next, Side::BLACK, PieceKind::Soldier);

    let cap =
        Move::Capture { from: h, to: diag, captured: Piece::new(Side::BLACK, PieceKind::General) };
    state.make_move(&cap).unwrap();

    assert_eq!(state.chain_lock, Some(diag), "馬斜 capture should chain when a follow-up exists");
    let legal = state.legal_moves();
    assert!(
        legal
            .iter()
            .any(|m| matches!(m, Move::Capture { from, to, .. } if *from == diag && *to == next)),
        "next-hop orthogonal capture must be in chain-mode legal_moves"
    );
}

#[test]
fn horse_orthogonal_dark_capture_still_obeys_rank() {
    // The rank bypass for diagonal must NOT leak into the horse's
    // orthogonal dark-capture. Soldier-rank horse vs hidden General
    // along a file should still probe (target gets revealed, attacker
    // stays put).
    let rules = RuleSet::banqi_with_seed(HouseRules::HORSE_DIAGONAL | HouseRules::DARK_CAPTURE, 0);
    let mut state = empty_banqi(rules);
    let h = state.board.sq(File(1), Rank(1));
    let target = state.board.sq(File(1), Rank(2));
    place_revealed(&mut state.board, h, Side::RED, PieceKind::Horse);
    state
        .board
        .set(target, Some(PieceOnSquare::hidden(Piece::new(Side::BLACK, PieceKind::General))));

    let mv = Move::DarkCapture { from: h, to: target, revealed: None, attacker: None };
    state.make_move(&mv).unwrap();

    // Probe: horse stayed at h, target revealed at target.
    let attacker = state.board.get(h).unwrap();
    assert_eq!(attacker.piece.kind, PieceKind::Horse);
    let revealed_target = state.board.get(target).unwrap();
    assert!(revealed_target.revealed);
    assert_eq!(revealed_target.piece.kind, PieceKind::General);
}
