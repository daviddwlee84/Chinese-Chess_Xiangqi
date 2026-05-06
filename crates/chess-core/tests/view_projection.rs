//! Hidden-info-leak proptest for `PlayerView`.
//!
//! Property: for any banqi state and any observer, no face-down piece's
//! identity (kind name) appears in the JSON serialization of the view.

use chess_core::piece::Side;
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::GameState;
use chess_core::view::{PlayerView, VisibleCell};
use proptest::prelude::*;

const KIND_NAMES: &[&str] =
    &["General", "Advisor", "Elephant", "Chariot", "Horse", "Cannon", "Soldier"];

fn assert_hidden_is_opaque(view: &PlayerView) {
    let json = serde_json::to_string(view).expect("serialize view");
    let visible_kinds: Vec<&str> = view
        .cells
        .iter()
        .filter_map(|c| match c {
            VisibleCell::Revealed(p) => Some(format!("{:?}", p.piece.kind)),
            _ => None,
        })
        .map(|s| s.leak() as &str) // OK in test
        .collect();
    for kind in KIND_NAMES {
        if json.contains(kind) {
            // Allowed only if a revealed piece on the board is of this kind.
            let appears_revealed = visible_kinds.iter().any(|v| v == kind);
            assert!(
                appears_revealed,
                "hidden-info leak: kind {kind} appeared in view JSON without any revealed instance"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn fresh_banqi_no_leak(seed in 0u64..1_000_000) {
        let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), seed));
        let view = PlayerView::project(&state, Side::RED);
        assert_hidden_is_opaque(&view);
    }

    #[test]
    fn after_random_flips_no_leak(seed in 0u64..1_000_000, n_flips in 0u8..6) {
        let mut state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), seed));
        for _ in 0..n_flips {
            let moves = state.legal_moves();
            if let Some(reveal) = moves.iter().find(|m| matches!(m, chess_core::moves::Move::Reveal { .. })) {
                state.make_move(reveal).unwrap();
            } else {
                break;
            }
        }
        for &observer in &[Side::RED, Side::BLACK] {
            let view = PlayerView::project(&state, observer);
            assert_hidden_is_opaque(&view);
        }
    }
}
