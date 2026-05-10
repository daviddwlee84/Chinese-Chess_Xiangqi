//! End-condition checks: load hand-crafted `.pos` fixtures, run
//! `refresh_status`, and assert the engine recognises the win/draw.
//!
//! Each fixture is annotated with the position's intent in its header
//! comments. If a fixture's expected result drifts from these tests,
//! audit the rule edit before adjusting either side.

use chess_core::piece::Side;
use chess_core::state::{GameState, GameStatus, WinReason};

fn load(path: &str) -> GameState {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    GameState::from_pos_text(&text).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

#[test]
fn xiangqi_three_chariot_mate_is_checkmate() {
    let mut state = load("tests/fixtures/xiangqi/three-chariot-mate.pos");
    assert_eq!(state.side_to_move, Side::BLACK);
    assert!(state.is_in_check(Side::BLACK), "black general should be in check");
    state.refresh_status();
    assert_eq!(
        state.status,
        GameStatus::Won { winner: Side::RED, reason: WinReason::Checkmate },
        "three-chariot mate should produce Checkmate"
    );
}

#[test]
fn xiangqi_horse_stalemate_is_stalemate() {
    let mut state = load("tests/fixtures/xiangqi/horse-stalemate.pos");
    assert_eq!(state.side_to_move, Side::BLACK);
    assert!(
        !state.is_in_check(Side::BLACK),
        "black should NOT be in check (otherwise this would be checkmate, not stalemate)"
    );
    state.refresh_status();
    assert_eq!(
        state.status,
        GameStatus::Won { winner: Side::RED, reason: WinReason::Stalemate },
        "horse stalemate should produce Stalemate (Asian rules: stalemated side loses)"
    );
}

#[test]
fn banqi_red_wiped_out_loses() {
    let mut state = load("tests/fixtures/banqi/red-wiped-out.pos");
    assert_eq!(state.side_to_move, Side::RED);
    assert_eq!(state.legal_moves().len(), 0, "Red has no legal moves");
    state.refresh_status();
    assert_eq!(
        state.status,
        GameStatus::Won { winner: Side::BLACK, reason: WinReason::OnlyOneSideHasPieces },
        "banqi side with no moves loses"
    );
}

#[test]
fn xiangqi_resign_red_makes_black_win_by_resignation() {
    let mut state = GameState::new(chess_core::rules::RuleSet::xiangqi());
    state.resign(Side::RED);
    assert_eq!(
        state.status,
        GameStatus::Won { winner: Side::BLACK, reason: WinReason::Resignation },
    );
}

#[test]
fn resign_is_noop_when_game_already_won() {
    // Already-won state should not be overwritten by a stray resign() call.
    let mut state = load("tests/fixtures/xiangqi/three-chariot-mate.pos");
    state.refresh_status();
    let before = state.status;
    state.resign(Side::RED);
    assert_eq!(state.status, before, "resign() must not overwrite a finished game");
}

#[test]
fn ongoing_position_stays_ongoing() {
    // Sanity: the fresh xiangqi opening should not trip any end condition.
    let mut state = GameState::new(chess_core::rules::RuleSet::xiangqi());
    state.refresh_status();
    assert_eq!(state.status, GameStatus::Ongoing);
}
