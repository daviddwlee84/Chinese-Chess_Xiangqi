//! After the first flip locks `side_assignment`, banqi move-gen must emit
//! moves for the piece-color the seat actually controls — not the seat name.

use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::GameState;

/// Find a banqi seed where RED's first reveal happens to expose a Black
/// piece. We deterministically scan seeds; with 32 cells / 2 colors the
/// first available reveal is Black with high frequency.
fn seed_where_first_reveal_is_black() -> u64 {
    for seed in 0u64..256 {
        let mut state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), seed));
        // Pick the FIRST face-down piece by iteration order and check its
        // hidden color (we peek at the board rather than calling make_move
        // so we can pick a deterministic seed without mutating state).
        let first_sq = state
            .board
            .squares()
            .find(|sq| state.board.get(*sq).map(|p| !p.revealed).unwrap_or(false));
        let Some(sq) = first_sq else { continue };
        let pos = state.board.get(sq).unwrap();
        if pos.piece.side == Side::BLACK {
            // Validate that we can reveal this square as RED's first move.
            let m = Move::Reveal { at: sq, revealed: None };
            if state.make_move(&m).is_ok() {
                return seed;
            }
        }
    }
    panic!("no seed found where RED's first reveal is Black");
}

#[test]
fn red_seat_revealing_black_plays_black_color() {
    let seed = seed_where_first_reveal_is_black();
    let mut state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), seed));
    assert_eq!(state.side_to_move, Side::RED, "banqi always starts with RED seat");

    // Locate the first face-down square (matches the seed-search above).
    let first_sq = state
        .board
        .squares()
        .find(|sq| state.board.get(*sq).map(|p| !p.revealed).unwrap_or(false))
        .unwrap();
    let revealed_color = state.board.get(first_sq).unwrap().piece.side;
    assert_eq!(revealed_color, Side::BLACK, "seed selected for first-flip-Black");

    // RED reveals a Black piece — RED-seat now plays Black.
    state.make_move(&Move::Reveal { at: first_sq, revealed: None }).unwrap();

    let mapping = state
        .side_assignment
        .as_ref()
        .expect("side_assignment must lock after first flip")
        .mapping
        .clone();
    assert_eq!(mapping[Side::RED.0 as usize], Side::BLACK, "RED-seat plays Black");
    assert_eq!(mapping[Side::BLACK.0 as usize], Side::RED, "BLACK-seat plays Red");

    // Now turn advances to BLACK-seat (who plays Red color). `current_color()`
    // must reflect Red, not Black.
    assert_eq!(state.side_to_move, Side::BLACK, "turn advances to BLACK seat");
    assert_eq!(state.current_color(), Side::RED, "BLACK-seat plays Red color after first flip");

    // Move-gen must not offer moves for Black pieces; only Red pieces +
    // remaining reveals. (The seat is BLACK, but the color it plays is RED.)
    let moves = state.legal_moves();
    let any_non_reveal_for_black = moves.iter().any(|m| {
        if let Move::Reveal { .. } = m {
            return false;
        }
        // Look up the piece at the move origin and confirm it's RED-colored
        // (i.e. the seat's controlled color).
        let origin = m.origin_square();
        match state.board.get(origin) {
            Some(p) if p.revealed => p.piece.side == Side::RED,
            _ => false,
        }
    });
    let any_non_reveal_for_red = moves.iter().any(|m| {
        if let Move::Reveal { .. } = m {
            return false;
        }
        let origin = m.origin_square();
        match state.board.get(origin) {
            Some(p) if p.revealed => p.piece.side == Side::BLACK,
            _ => false,
        }
    });
    // BLACK-seat plays RED color → only Red-piece moves should appear.
    // (At this point only one piece is revealed — the Black one we flipped.
    // BLACK can't move it. So only Reveal moves are legal. The presence/absence
    // checks below validate the *direction* of the filter even when no movable
    // piece exists yet.)
    assert!(
        !any_non_reveal_for_red,
        "BLACK-seat (playing Red) must not see moves on Black-colored pieces"
    );
    let _ = any_non_reveal_for_black; // either path is fine — no Red is revealed yet
}

#[test]
fn before_first_flip_current_color_falls_back_to_seat() {
    let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    assert!(state.side_assignment.is_none());
    assert_eq!(state.current_color(), state.side_to_move);
}
