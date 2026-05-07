//! Per-player view projection.
//!
//! `PlayerView` is the only state the network layer should serialize and
//! ship to clients — it has hidden-piece identities scrubbed and (for
//! non-side-to-move observers) no legal-move list to leak strategy.
//!
//! See ADR-0004.

use serde::{Deserialize, Serialize};

use crate::board::BoardShape;
use crate::coord::Square;
use crate::moves::{Move, MoveList};
use crate::piece::{Piece, PieceOnSquare, Side};
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
    /// The seat / turn-order index whose turn it currently is. For
    /// xiangqi this matches the piece-colour exactly; for banqi after
    /// the first flip it may differ from `current_color` (the seat name
    /// is fixed at room-join time, but who plays which colour is
    /// decided by the first reveal). Use `current_color` for
    /// "Red/Black to move" labels and `side_to_move` for seat-routing
    /// (e.g. is it MY turn).
    pub side_to_move: Side,
    pub status: GameStatus,
    /// Legal moves for the observer if it's their turn; empty otherwise.
    pub legal_moves: MoveList,
    /// Whether the observer's own general is currently under attack.
    /// Always `false` for banqi/three-kingdom (no in-check concept) and
    /// for spectator-style observers without a real seat. Added in
    /// protocol v4; older clients see the default via `serde(default)`.
    #[serde(default)]
    pub in_check: bool,
    /// Banqi 連吃 chain-mode lock: when `Some(sq)`, the player whose
    /// turn it is must continue capturing with the piece at `sq` or
    /// issue `Move::EndChain { at: sq }` to release the lock. The
    /// `legal_moves` list is already filtered for this case. Added in
    /// protocol v5; older clients see the default `None` via
    /// `serde(default)`.
    #[serde(default)]
    pub chain_lock: Option<Square>,
    /// Piece-colour the active seat actually controls. Equal to
    /// `side_to_move` until a banqi first-flip locks `side_assignment`;
    /// from then on it reflects the *colour* being played. UIs use this
    /// for "Red 紅 / Black 黑 to move" labels. Added in protocol v5;
    /// `serde(default = "default_red")` so older payloads (which lacked
    /// the field) still deserialise — Red was always the starter
    /// pre-v5, so the default is correct for fresh games.
    #[serde(default = "default_red_side")]
    pub current_color: Side,
    /// Pieces captured so far, in chronological (history) order.
    /// Clients sort/group as they wish for the sidebar graveyard panel.
    /// Added in protocol v5.1; older clients see the empty default via
    /// `serde(default)`.
    #[serde(default)]
    pub captured: Vec<Piece>,
}

fn default_red_side() -> Side {
    Side::RED
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

        // Only the observer's own general counts. Banqi/3K have no general,
        // so `is_in_check` returns false naturally — no variant gate needed.
        let in_check = state.is_in_check(observer);

        Self {
            observer,
            shape: board.shape(),
            width: board.width(),
            height: board.height(),
            cells,
            side_to_move: state.side_to_move,
            status: state.status,
            legal_moves,
            in_check,
            chain_lock: state.chain_lock,
            current_color: state.current_color(),
            captured: state.captured_pieces(),
        }
    }
}

/// Strip identity from `Reveal` / `DarkCapture` moves before they reach
/// the network. Even the side-to-move sees `revealed: None` because the
/// engine resolves the identity authoritatively when the move is applied.
fn sanitize_for_observer(moves: MoveList, _observer: Side) -> MoveList {
    moves
        .into_iter()
        .map(|m| match m {
            Move::Reveal { at, revealed: _ } => Move::Reveal { at, revealed: None },
            Move::DarkCapture { from, to, .. } => {
                Move::DarkCapture { from, to, revealed: None, attacker: None }
            }
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
    fn fresh_xiangqi_view_has_neither_side_in_check() {
        let state = GameState::new(RuleSet::xiangqi());
        let red = PlayerView::project(&state, Side::RED);
        let black = PlayerView::project(&state, Side::BLACK);
        assert!(!red.in_check, "starting xiangqi: red not in check");
        assert!(!black.in_check, "starting xiangqi: black not in check");
    }

    #[test]
    fn xiangqi_in_check_view_flags_observer() {
        // Three-chariot mating net (same as tests/fixtures/xiangqi/three-chariot-mate.pos):
        // Red rooks on d8/e8/f8, Black general on e9, Red general on e0.
        // Black is in check; red is not.
        let pos = "variant: xiangqi\nside_to_move: black\n\nboard:\n  . . . . k . . . .\n  . . . R R R . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . K . . . .\n";
        let state = GameState::from_pos_text(pos).expect("parse pos");
        let black_view = PlayerView::project(&state, Side::BLACK);
        let red_view = PlayerView::project(&state, Side::RED);
        assert!(black_view.in_check, "black observer must see in_check = true");
        assert!(!red_view.in_check, "red observer must see in_check = false");
    }

    #[test]
    fn banqi_view_never_flags_in_check() {
        let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 13));
        for side in [Side::RED, Side::BLACK] {
            let view = PlayerView::project(&state, side);
            assert!(!view.in_check, "banqi has no general; in_check must be false");
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
