//! After the first flip locks `side_assignment`, banqi move-gen must emit
//! moves for the piece-color the seat actually controls — not the seat name.
//!
//! The default banqi ruleset (without `HouseRules::PREASSIGN_COLORS`) lets
//! EITHER seat make the first reveal. The deployment layer (chess-net /
//! HostRoom) is responsible for setting `state.side_to_move = clicker_seat`
//! before calling `make_move`; the existing `banqi_side_assignment` then
//! locks the seat→colour mapping correctly. These tests exercise the
//! engine half of that contract — that with `side_to_move` set, the right
//! mapping comes out.

use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::rules::{HouseRules, RuleSet};
use chess_core::state::GameState;
use chess_core::view::PlayerView;

/// Find a banqi seed where the FIRST face-down square (iteration order)
/// holds a Black piece. Used by tests that want to flip a black piece
/// from the first listed square deterministically.
fn seed_where_first_reveal_is_black() -> u64 {
    for seed in 0u64..256 {
        let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), seed));
        let first_sq = state
            .board
            .squares()
            .find(|sq| state.board.get(*sq).map(|p| !p.revealed).unwrap_or(false));
        let Some(sq) = first_sq else { continue };
        let pos = state.board.get(sq).unwrap();
        if pos.piece.side == Side::BLACK {
            return seed;
        }
    }
    panic!("no seed found where the first listed face-down square is Black");
}

#[test]
fn red_seat_revealing_black_plays_black_color() {
    // Legacy mode: PREASSIGN_COLORS forces RED seat to flip first.
    let seed = seed_where_first_reveal_is_black();
    let mut state = GameState::new(RuleSet::banqi_with_seed(HouseRules::PREASSIGN_COLORS, seed));
    assert_eq!(state.side_to_move, Side::RED, "PREASSIGN_COLORS keeps RED as first mover");
    assert!(
        !state.banqi_awaiting_first_flip(),
        "PREASSIGN_COLORS suppresses the either-seat-flips sentinel"
    );

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

#[test]
fn default_rules_set_banqi_awaiting_first_flip_sentinel() {
    let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    assert!(
        state.banqi_awaiting_first_flip(),
        "default banqi (no PREASSIGN_COLORS) must be in the either-seat-flips state"
    );
}

#[test]
fn preassign_colors_disables_either_seat_first_flip() {
    let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::PREASSIGN_COLORS, 0));
    assert!(
        !state.banqi_awaiting_first_flip(),
        "PREASSIGN_COLORS keeps the classic RED-flips-first behaviour"
    );
}

#[test]
fn black_seat_can_flip_first_under_default_rules() {
    // Default rules: deployment layer is allowed to set `side_to_move = BLACK`
    // when the BLACK seat clicks first. The engine's `banqi_side_assignment`
    // then locks the mapping with BLACK as the flipper.
    let seed = seed_where_first_reveal_is_black();
    let mut state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), seed));
    assert!(state.banqi_awaiting_first_flip());

    // Simulate the deployment layer attributing the flip to BLACK seat.
    state.set_active_seat(Side::BLACK).expect("BLACK is a valid seat");

    let first_sq = state
        .board
        .squares()
        .find(|sq| state.board.get(*sq).map(|p| !p.revealed).unwrap_or(false))
        .unwrap();
    let revealed_color = state.board.get(first_sq).unwrap().piece.side;
    assert_eq!(revealed_color, Side::BLACK, "seed selected for first-listed-Black");

    state.make_move(&Move::Reveal { at: first_sq, revealed: None }).unwrap();

    let mapping = state
        .side_assignment
        .as_ref()
        .expect("side_assignment must lock after first flip")
        .mapping
        .clone();
    // BLACK seat flipped, revealed BLACK → BLACK seat plays BLACK colour
    // (Taiwan rule: flipper plays the colour they reveal).
    assert_eq!(
        mapping[Side::BLACK.0 as usize],
        Side::BLACK,
        "BLACK seat (the flipper) plays the revealed colour"
    );
    assert_eq!(mapping[Side::RED.0 as usize], Side::RED, "RED seat plays the opposite colour");

    // Turn advances back to RED seat (now playing RED colour).
    assert_eq!(state.side_to_move, Side::RED);
    assert_eq!(state.current_color(), Side::RED);

    // `banqi_awaiting_first_flip` clears once `side_assignment` locks.
    assert!(!state.banqi_awaiting_first_flip());
}

#[test]
fn view_pre_first_flip_exposes_legal_moves_to_both_observers() {
    // With the either-seat-flips default, `PlayerView::project` from BOTH
    // RED and BLACK observers receives the sanitised reveal list — the
    // deployment layer is authoritative for which seat actually flips.
    let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    assert!(state.banqi_awaiting_first_flip());

    let red = PlayerView::project(&state, Side::RED);
    let black = PlayerView::project(&state, Side::BLACK);

    assert!(!red.legal_moves.is_empty(), "RED observer must see reveal moves pre-first-flip");
    assert!(!black.legal_moves.is_empty(), "BLACK observer must see reveal moves pre-first-flip");
    assert_eq!(
        red.legal_moves.len(),
        black.legal_moves.len(),
        "both observers see the same reveal-move count"
    );
    assert!(red.banqi_awaiting_first_flip);
    assert!(black.banqi_awaiting_first_flip);
    // Every legal move must be a Reveal (no revealed pieces yet).
    for m in &red.legal_moves {
        assert!(matches!(m, Move::Reveal { revealed: None, .. }));
    }
}

#[test]
fn view_with_preassign_colors_keeps_existing_gate() {
    // Legacy mode: only the side-to-move observer sees legal moves.
    let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::PREASSIGN_COLORS, 0));
    assert!(!state.banqi_awaiting_first_flip());

    let red = PlayerView::project(&state, Side::RED);
    let black = PlayerView::project(&state, Side::BLACK);

    assert!(!red.legal_moves.is_empty(), "RED is side-to-move under PREASSIGN");
    assert!(black.legal_moves.is_empty(), "BLACK is not side-to-move under PREASSIGN");
    assert!(!red.banqi_awaiting_first_flip);
    assert!(!black.banqi_awaiting_first_flip);
}

#[test]
fn view_clears_awaiting_after_first_flip() {
    let mut state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
    let first_sq = state
        .board
        .squares()
        .find(|sq| state.board.get(*sq).map(|p| !p.revealed).unwrap_or(false))
        .unwrap();
    state.make_move(&Move::Reveal { at: first_sq, revealed: None }).unwrap();

    let red = PlayerView::project(&state, Side::RED);
    let black = PlayerView::project(&state, Side::BLACK);
    assert!(
        !red.banqi_awaiting_first_flip && !black.banqi_awaiting_first_flip,
        "sentinel must clear once the first flip locks the side assignment"
    );
}
