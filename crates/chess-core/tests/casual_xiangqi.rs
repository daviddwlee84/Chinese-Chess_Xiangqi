//! Casual xiangqi mode (`xiangqi_allow_self_check`): the legality filter
//! does NOT reject moves that leave your own general capturable; the game
//! ends when the general is physically captured.

use chess_core::board::{Board, BoardShape};
use chess_core::coord::{File, Rank};
use chess_core::piece::{Piece, PieceKind, PieceOnSquare, Side};
use chess_core::rules::RuleSet;
use chess_core::state::{GameState, GameStatus, TurnOrder, WinReason};
use smallvec::smallvec;

#[test]
fn casual_mode_default_is_off() {
    let r = RuleSet::xiangqi();
    assert!(!r.xiangqi_allow_self_check);
    let r = RuleSet::xiangqi_casual();
    assert!(r.xiangqi_allow_self_check);
}

#[test]
fn casual_mode_exposes_moves_in_a_standard_checkmate_position() {
    let text = std::fs::read_to_string("tests/fixtures/xiangqi/three-chariot-mate.pos")
        .expect("read fixture");
    let mut state = GameState::from_pos_text(&text).expect("parse fixture");

    // Standard rules: checkmate → 0 legal moves.
    assert_eq!(state.legal_moves().len(), 0, "standard rules: checkmate");

    // Switch to casual: pseudo-legal moves become legal.
    state.rules.xiangqi_allow_self_check = true;
    let casual_moves = state.legal_moves();
    assert!(
        !casual_moves.is_empty(),
        "casual mode should expose pseudo-legal moves even when standard says checkmate"
    );
}

#[test]
fn general_captured_triggers_won_by_general_captured() {
    // Build a contrived end-state:
    //   Red has only a general at e0.
    //   Black has a general at e9 and a chariot at e3.
    //   Black to move; the chariot captures Red's general.
    //
    // refresh_status should report Won { winner: BLACK, reason: GeneralCaptured }.
    let mut board = Board::new(BoardShape::Xiangqi9x10);
    let red_gen = board.sq(File(4), Rank(0));
    let blk_gen = board.sq(File(4), Rank(9));
    let blk_chariot = board.sq(File(4), Rank(3));

    board.set(red_gen, Some(PieceOnSquare::revealed(Piece::new(Side::RED, PieceKind::General))));
    board.set(blk_gen, Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::General))));
    board.set(
        blk_chariot,
        Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::Chariot))),
    );

    let mut state = GameState {
        rules: RuleSet::xiangqi_casual(),
        board,
        side_to_move: Side::BLACK,
        turn_order: TurnOrder { seats: smallvec![Side::RED, Side::BLACK], current: 1 },
        history: Vec::new(),
        status: GameStatus::Ongoing,
        side_assignment: None,
        no_progress_plies: 0,
        chain_lock: None,
        position_hash: 0,
        banqi_first_mover_locked: false,
    };
    state.recompute_position_hash();

    let capture = state
        .legal_moves()
        .into_iter()
        .find(|m| m.origin_square() == blk_chariot && m.to_square() == Some(red_gen))
        .expect("Black chariot should be able to capture the Red general");

    state.make_move(&capture).expect("apply capture");
    state.refresh_status();

    assert_eq!(
        state.status,
        GameStatus::Won { winner: Side::BLACK, reason: WinReason::GeneralCaptured },
        "expected GeneralCaptured win, got {:?}",
        state.status
    );
}

#[test]
fn standard_mode_still_rejects_self_check() {
    // Sanity: in a real opening, exposing the general should still be blocked.
    let mut state = GameState::new(RuleSet::xiangqi());
    let opening_count = state.legal_moves().len();
    state.refresh_status();
    assert_eq!(state.status, GameStatus::Ongoing);
    assert_eq!(opening_count, 44, "xiangqi opening has 44 legal moves under standard rules");
}
