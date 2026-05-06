//! Per-player view projection.
//!
//! `PlayerView` is the only state the network layer should serialize and
//! ship to clients — it has hidden-piece identities scrubbed and (for
//! non-side-to-move observers) no legal-move list to leak strategy.
//!
//! See ADR-0004.

use serde::{Deserialize, Serialize};

use crate::board::BoardShape;
use crate::moves::{Move, MoveList};
use crate::piece::{PieceOnSquare, Side};
use crate::state::{GameState, GameStatus};

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum VisibleCell {
    Empty,
    /// Banqi face-down: identity hidden from this observer.
    Hidden,
    Revealed(PieceOnSquare),
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct PlayerView {
    pub observer: Side,
    pub shape: BoardShape,
    pub width: u8,
    pub height: u8,
    pub cells: Vec<VisibleCell>,
    pub side_to_move: Side,
    pub status: GameStatus,
    /// Legal moves for the observer if it's their turn; empty otherwise.
    pub legal_moves: MoveList,
}

impl PlayerView {
    /// Project `state` from `observer`'s vantage point. Hidden pieces stay
    /// hidden; for opponents, legal-move list is empty.
    pub fn project(state: &GameState, observer: Side) -> Self {
        let board = &state.board;
        let cells: Vec<VisibleCell> = (0..board.cell_count())
            .map(|i| {
                let sq = crate::coord::Square(i as u16);
                match board.get(sq) {
                    None => VisibleCell::Empty,
                    Some(pos) if pos.revealed => VisibleCell::Revealed(pos),
                    Some(_) => VisibleCell::Hidden,
                }
            })
            .collect();

        let legal_moves = if observer == state.side_to_move {
            sanitize_for_observer(state.legal_moves(), observer)
        } else {
            MoveList::new()
        };

        Self {
            observer,
            shape: board.shape(),
            width: board.width(),
            height: board.height(),
            cells,
            side_to_move: state.side_to_move,
            status: state.status,
            legal_moves,
        }
    }
}

/// Strip identity from `Reveal` moves before they reach the network.
/// Even the side-to-move sees `revealed: None` because the engine resolves
/// the identity authoritatively when the move is applied.
fn sanitize_for_observer(moves: MoveList, _observer: Side) -> MoveList {
    moves
        .into_iter()
        .map(|m| match m {
            Move::Reveal { at, revealed: _ } => Move::Reveal { at, revealed: None },
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{HouseRules, RuleSet};

    #[test]
    fn fresh_banqi_view_has_all_hidden_for_any_observer() {
        let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 7));
        let view = PlayerView::project(&state, Side::RED);
        assert_eq!(view.cells.len(), 32);
        assert!(view.cells.iter().all(|c| matches!(c, VisibleCell::Hidden)));
    }

    #[test]
    fn xiangqi_view_shows_revealed_pieces() {
        let state = GameState::new(RuleSet::xiangqi());
        let view = PlayerView::project(&state, Side::RED);
        let revealed: Vec<_> =
            view.cells.iter().filter(|c| matches!(c, VisibleCell::Revealed(_))).collect();
        assert_eq!(revealed.len(), 32);
    }

    #[test]
    fn opponent_view_has_empty_legal_moves() {
        let state = GameState::new(RuleSet::xiangqi());
        let red_view = PlayerView::project(&state, Side::RED);
        let black_view = PlayerView::project(&state, Side::BLACK);
        assert!(!red_view.legal_moves.is_empty(), "side-to-move sees moves");
        assert!(black_view.legal_moves.is_empty(), "opponent doesn't");
    }

    #[test]
    fn reveal_moves_have_no_identity_in_view() {
        let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 1));
        let view = PlayerView::project(&state, Side::RED);
        for m in &view.legal_moves {
            if let Move::Reveal { revealed, .. } = m {
                assert!(revealed.is_none(), "Reveal payload must be stripped in view");
            }
        }
    }

    #[test]
    fn no_hidden_identity_in_serialized_view() {
        // The smoke-level version of the proptest in tests/view_projection.rs.
        let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 42));
        let view = PlayerView::project(&state, Side::RED);
        let json = serde_json::to_string(&view).unwrap();
        // None of the piece-kind names should appear in any cell — they're all hidden.
        // (Empty cells trivially never contain a kind; we just confirm the doc-level
        // invariant.)
        for kind_name in ["General", "Advisor", "Elephant", "Chariot", "Horse", "Cannon", "Soldier"]
        {
            assert!(
                !json.contains(kind_name),
                "fresh banqi view must not leak any piece kind ({kind_name})"
            );
        }
    }
}
