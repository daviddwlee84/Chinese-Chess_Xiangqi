//! 連吃 chain mode (engine-state, not just atomic): after a chain-eligible
//! capture, `chain_lock` activates and the turn does NOT auto-advance until
//! the player either continues capturing in any direction or issues
//! `Move::EndChain`.

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
fn capture_into_chain_keeps_turn_with_same_player() {
    // Red horse at a1 captures a2 (black soldier), and from a2 can keep
    // capturing a3 (another black soldier) — chain mode activates.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    let h = state.board.sq(File(0), Rank(1));
    let s1 = state.board.sq(File(0), Rank(2));
    let s2 = state.board.sq(File(0), Rank(3));
    place(&mut state.board, h, Side::RED, PieceKind::Horse);
    place(&mut state.board, s1, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, s2, Side::BLACK, PieceKind::Soldier);

    let single = Move::Capture {
        from: h,
        to: s1,
        captured: Piece::new(Side::BLACK, PieceKind::Soldier),
    };
    state.make_move(&single).unwrap();

    assert_eq!(state.chain_lock, Some(s1), "chain_lock activates on capturing piece's new square");
    assert_eq!(state.side_to_move, Side::RED, "turn does NOT advance — still RED");
    assert_eq!(state.current_color(), Side::RED);
}

#[test]
fn chain_legal_moves_filter_to_captures_from_locked_square_plus_end_chain() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    let h = state.board.sq(File(1), Rank(1));
    let s1 = state.board.sq(File(1), Rank(2));
    let s2 = state.board.sq(File(1), Rank(3));
    let s_side = state.board.sq(File(2), Rank(2));
    let other = state.board.sq(File(3), Rank(0));
    place(&mut state.board, h, Side::RED, PieceKind::Horse);
    place(&mut state.board, s1, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, s2, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, s_side, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, other, Side::RED, PieceKind::Chariot);

    let cap = Move::Capture {
        from: h,
        to: s1,
        captured: Piece::new(Side::BLACK, PieceKind::Soldier),
    };
    state.make_move(&cap).unwrap();

    let legal = state.legal_moves();
    // Only moves originating at s1 (the chain-locked square) + EndChain.
    for m in legal.iter() {
        match m {
            Move::EndChain { at } => assert_eq!(*at, s1),
            other => assert_eq!(other.origin_square(), s1, "all locked legal moves must start at s1"),
        }
    }
    // Specifically, the other Red chariot at `other` must NOT be movable.
    assert!(
        !legal.iter().any(|m| m.origin_square() == other && !matches!(m, Move::EndChain { .. })),
        "non-locked piece must not be in legal_moves"
    );
    // EndChain must always be available.
    assert!(legal.iter().any(|m| matches!(m, Move::EndChain { .. })));
    // Captures in the SAME line (s2) AND in a perpendicular direction
    // (s_side) should both be legal.
    assert!(legal
        .iter()
        .any(|m| matches!(m, Move::Capture { to, .. } if *to == s2)));
    assert!(legal
        .iter()
        .any(|m| matches!(m, Move::Capture { to, .. } if *to == s_side)));
}

#[test]
fn end_chain_advances_turn_and_clears_lock() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    let h = state.board.sq(File(1), Rank(1));
    let s1 = state.board.sq(File(1), Rank(2));
    let s2 = state.board.sq(File(1), Rank(3));
    place(&mut state.board, h, Side::RED, PieceKind::Horse);
    place(&mut state.board, s1, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, s2, Side::BLACK, PieceKind::Soldier);

    let cap = Move::Capture {
        from: h,
        to: s1,
        captured: Piece::new(Side::BLACK, PieceKind::Soldier),
    };
    state.make_move(&cap).unwrap();
    assert!(state.chain_lock.is_some());

    state.make_move(&Move::EndChain { at: s1 }).unwrap();
    assert_eq!(state.chain_lock, None, "EndChain clears the lock");
    assert_eq!(state.side_to_move, Side::BLACK, "turn advances after EndChain");
}

#[test]
fn capture_with_no_followup_advances_turn_normally() {
    // Red horse at a1 captures a2, but no further captures available from a2.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    let h = state.board.sq(File(0), Rank(1));
    let s1 = state.board.sq(File(0), Rank(2));
    place(&mut state.board, h, Side::RED, PieceKind::Horse);
    place(&mut state.board, s1, Side::BLACK, PieceKind::Soldier);

    let cap = Move::Capture {
        from: h,
        to: s1,
        captured: Piece::new(Side::BLACK, PieceKind::Soldier),
    };
    state.make_move(&cap).unwrap();
    assert_eq!(state.chain_lock, None, "no further captures from a2 → no chain lock");
    assert_eq!(state.side_to_move, Side::BLACK, "turn advanced normally");
}

#[test]
fn chain_mode_does_not_activate_without_chain_capture_flag() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    let h = state.board.sq(File(1), Rank(1));
    let s1 = state.board.sq(File(1), Rank(2));
    let s2 = state.board.sq(File(1), Rank(3));
    place(&mut state.board, h, Side::RED, PieceKind::Horse);
    place(&mut state.board, s1, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, s2, Side::BLACK, PieceKind::Soldier);

    let cap = Move::Capture {
        from: h,
        to: s1,
        captured: Piece::new(Side::BLACK, PieceKind::Soldier),
    };
    state.make_move(&cap).unwrap();
    assert_eq!(state.chain_lock, None, "no chain mode without CHAIN_CAPTURE flag");
    assert_eq!(state.side_to_move, Side::BLACK, "turn advanced normally");
}

#[test]
fn perpendicular_capture_in_chain_keeps_lock() {
    // The user's image scenario: 相 captures 砲 east, then can capture
    // 卒 south in chain mode.
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    let elephant = state.board.sq(File(0), Rank(2));
    let cannon_target = state.board.sq(File(1), Rank(2));
    let south_soldier = state.board.sq(File(1), Rank(1));
    place(&mut state.board, elephant, Side::RED, PieceKind::Elephant);
    place(&mut state.board, cannon_target, Side::BLACK, PieceKind::Cannon);
    place(&mut state.board, south_soldier, Side::BLACK, PieceKind::Soldier);

    let cap1 = Move::Capture {
        from: elephant,
        to: cannon_target,
        captured: Piece::new(Side::BLACK, PieceKind::Cannon),
    };
    state.make_move(&cap1).unwrap();
    assert_eq!(state.chain_lock, Some(cannon_target));
    assert_eq!(state.side_to_move, Side::RED, "still red after first capture");

    // Now from cannon_target capture the south soldier — perpendicular
    // direction, but legal because rules permit any-direction capture
    // when chain_lock is active and the moving piece can capture there.
    let legal = state.legal_moves();
    let cap2 = legal
        .iter()
        .find(|m| matches!(m, Move::Capture { to, .. } if *to == south_soldier))
        .cloned()
        .expect("south soldier capture must be legal");
    state.make_move(&cap2).unwrap();

    // Lock either continues (more captures) or ends (none more) — here
    // there's nothing else, so it should end.
    assert_eq!(state.chain_lock, None);
    assert_eq!(state.side_to_move, Side::BLACK, "turn finally advances when chain dries up");
}

#[test]
fn chain_make_unmake_round_trip() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    let h = state.board.sq(File(1), Rank(1));
    let s1 = state.board.sq(File(1), Rank(2));
    let s2 = state.board.sq(File(1), Rank(3));
    place(&mut state.board, h, Side::RED, PieceKind::Horse);
    place(&mut state.board, s1, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, s2, Side::BLACK, PieceKind::Soldier);
    let snapshot = state.clone();

    let cap = Move::Capture {
        from: h,
        to: s1,
        captured: Piece::new(Side::BLACK, PieceKind::Soldier),
    };
    state.make_move(&cap).unwrap();
    assert!(state.chain_lock.is_some());

    state.unmake_move().unwrap();
    assert_eq!(state.board, snapshot.board);
    assert_eq!(state.side_to_move, snapshot.side_to_move);
    assert_eq!(state.chain_lock, snapshot.chain_lock);
}

#[test]
fn dark_capture_probe_against_own_piece_does_not_activate_chain_lock() {
    // User-reported bug: 兵 dark-captures a hidden tile that turns out
    // to be 帥 (same side). Outcome = Probe (attacker stays, target
    // revealed). Engine must NOT chain-lock onto the just-revealed
    // 帥 — that's not the attacker.
    use chess_core::moves::Move;
    use chess_core::piece::PieceOnSquare;
    let mut state = empty_banqi(RuleSet::banqi_with_seed(
        HouseRules::CHAIN_CAPTURE | HouseRules::DARK_CAPTURE,
        0,
    ));
    let attacker_sq = state.board.sq(File(1), Rank(1));
    let hidden_sq = state.board.sq(File(1), Rank(2));
    place(&mut state.board, attacker_sq, Side::RED, PieceKind::Soldier);
    state.board.set(
        hidden_sq,
        Some(PieceOnSquare::hidden(Piece::new(Side::RED, PieceKind::General))),
    );

    let mv = Move::DarkCapture {
        from: attacker_sq,
        to: hidden_sq,
        revealed: None,
        attacker: None,
    };
    state.make_move(&mv).unwrap();

    assert_eq!(
        state.chain_lock, None,
        "probe against own piece must not chain-lock the just-revealed friend"
    );
    assert_eq!(state.side_to_move, Side::BLACK, "turn advanced after probe");
    // The newly-revealed 帥 stayed at hidden_sq, attacker stayed at attacker_sq.
    assert_eq!(state.board.get(attacker_sq).unwrap().piece.kind, PieceKind::Soldier);
    let revealed_target = state.board.get(hidden_sq).unwrap();
    assert!(revealed_target.revealed);
    assert_eq!(revealed_target.piece.kind, PieceKind::General);
    assert_eq!(revealed_target.piece.side, Side::RED);
}

#[test]
fn dark_capture_trade_against_stronger_enemy_does_not_activate_chain_lock() {
    use chess_core::moves::Move;
    use chess_core::piece::PieceOnSquare;
    let mut state = empty_banqi(RuleSet::banqi_with_seed(
        HouseRules::CHAIN_CAPTURE | HouseRules::DARK_CAPTURE | HouseRules::DARK_CAPTURE_TRADE,
        0,
    ));
    let attacker_sq = state.board.sq(File(1), Rank(1));
    let hidden_sq = state.board.sq(File(1), Rank(2));
    place(&mut state.board, attacker_sq, Side::RED, PieceKind::Soldier);
    state.board.set(
        hidden_sq,
        Some(PieceOnSquare::hidden(Piece::new(Side::BLACK, PieceKind::Elephant))),
    );

    let mv = Move::DarkCapture {
        from: attacker_sq,
        to: hidden_sq,
        revealed: None,
        attacker: None,
    };
    state.make_move(&mv).unwrap();

    assert_eq!(state.chain_lock, None, "trade outcome (attacker died) must not chain-lock");
    assert_eq!(state.side_to_move, Side::BLACK);
    assert!(state.board.get(attacker_sq).is_none(), "attacker died");
}

#[test]
fn dark_capture_success_can_activate_chain_lock() {
    // Sanity: a successful DarkCapture (attacker outranks revealed target)
    // SHOULD chain-lock when more captures are available.
    use chess_core::moves::Move;
    use chess_core::piece::PieceOnSquare;
    let mut state = empty_banqi(RuleSet::banqi_with_seed(
        HouseRules::CHAIN_CAPTURE | HouseRules::DARK_CAPTURE,
        0,
    ));
    let h = state.board.sq(File(1), Rank(1));
    let hidden = state.board.sq(File(1), Rank(2));
    let next = state.board.sq(File(1), Rank(3));
    place(&mut state.board, h, Side::RED, PieceKind::Horse);
    state.board.set(
        hidden,
        Some(PieceOnSquare::hidden(Piece::new(Side::BLACK, PieceKind::Soldier))),
    );
    place(&mut state.board, next, Side::BLACK, PieceKind::Soldier);

    let mv = Move::DarkCapture { from: h, to: hidden, revealed: None, attacker: None };
    state.make_move(&mv).unwrap();
    assert_eq!(state.chain_lock, Some(hidden), "successful dark-capture chain-locks at landing");
    assert_eq!(state.side_to_move, Side::RED);
}

#[test]
fn end_chain_make_unmake_round_trip() {
    let mut state = empty_banqi(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
    let h = state.board.sq(File(1), Rank(1));
    let s1 = state.board.sq(File(1), Rank(2));
    let s2 = state.board.sq(File(1), Rank(3));
    place(&mut state.board, h, Side::RED, PieceKind::Horse);
    place(&mut state.board, s1, Side::BLACK, PieceKind::Soldier);
    place(&mut state.board, s2, Side::BLACK, PieceKind::Soldier);

    let cap = Move::Capture {
        from: h,
        to: s1,
        captured: Piece::new(Side::BLACK, PieceKind::Soldier),
    };
    state.make_move(&cap).unwrap();
    let snapshot_after_cap = state.clone();

    state.make_move(&Move::EndChain { at: s1 }).unwrap();
    assert_eq!(state.chain_lock, None);
    assert_eq!(state.side_to_move, Side::BLACK);

    state.unmake_move().unwrap();
    assert_eq!(state.board, snapshot_after_cap.board);
    assert_eq!(state.chain_lock, snapshot_after_cap.chain_lock);
    assert_eq!(state.side_to_move, snapshot_after_cap.side_to_move);
}
