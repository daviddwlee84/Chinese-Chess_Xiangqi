//! Banqi end-to-end smoke tests.

use chess_core::moves::Move;
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::GameState;
use chess_core::view::{PlayerView, VisibleCell};

#[test]
fn fresh_banqi_offers_32_reveals() {
    let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 1));
    let moves = state.legal_moves();
    assert_eq!(moves.len(), 32);
    assert!(moves.iter().all(|m| matches!(m, Move::Reveal { .. })));
}

#[test]
fn first_flip_locks_side_assignment() {
    let mut state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 5));
    assert!(state.side_assignment.is_none());
    let m = state.legal_moves().into_iter().next().unwrap();
    state.make_move(&m).unwrap();
    assert!(state.side_assignment.is_some(), "side assignment must lock after first flip");
}

#[test]
fn deterministic_seed_yields_same_setup() {
    let s1 = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 7));
    let s2 = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 7));
    assert_eq!(s1.board, s2.board);
}

#[test]
fn banqi_view_for_fresh_is_all_hidden() {
    let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 13));
    let view = PlayerView::project(&state, chess_core::piece::Side::RED);
    assert!(view.cells.iter().all(|c| matches!(c, VisibleCell::Hidden)));
}
