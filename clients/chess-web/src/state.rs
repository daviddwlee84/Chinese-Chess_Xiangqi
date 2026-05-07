//! Pure-logic helpers for the client's view of a game. Native-testable —
//! no Leptos signals or browser deps live here.

use chess_core::coord::Square;
use chess_core::moves::Move;
use chess_core::piece::Side;
use chess_core::view::PlayerView;

/// Role assigned by the server on `Hello` (player) or `Spectating`
/// (read-only). Drives whether the play page renders move/resign/rematch
/// affordances and whether the chat input is enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientRole {
    Player(Side),
    Spectator,
}

impl ClientRole {
    pub fn is_player(self) -> bool {
        matches!(self, ClientRole::Player(_))
    }

    pub fn is_spectator(self) -> bool {
        matches!(self, ClientRole::Spectator)
    }

    pub fn observer(self) -> Side {
        // Spectators render from RED's POV — matches what chess-net's
        // `broadcast_update` projects for spectator updates.
        match self {
            ClientRole::Player(s) => s,
            ClientRole::Spectator => Side::RED,
        }
    }
}

/// Trim a vec from the front so it holds at most `max` entries. Used for
/// the per-page chat ring buffer that mirrors the server's 50-line cap.
pub fn truncate_front<T>(buf: &mut Vec<T>, max: usize) {
    if buf.len() > max {
        let drop_count = buf.len() - max;
        buf.drain(0..drop_count);
    }
}

/// Find the legal `Move` (if any) whose origin is `from` and whose final
/// destination is `to`. Reveal moves match when `from == to == at`.
pub fn find_move(view: &PlayerView, from: Square, to: Square) -> Option<Move> {
    view.legal_moves.iter().find(|m| matches_endpoints(m, from, to)).cloned()
}

fn matches_endpoints(mv: &Move, from: Square, to: Square) -> bool {
    if mv.origin_square() != from {
        return false;
    }
    match mv.to_square() {
        Some(t) => t == to,
        None => matches!(mv, Move::Reveal { at, .. } if *at == to),
    }
}

/// All legal destination squares for a piece on `from` (used to render dots).
pub fn legal_targets(view: &PlayerView, from: Square) -> Vec<Square> {
    view.legal_moves
        .iter()
        .filter(|m| m.origin_square() == from)
        .map(|m| match m.to_square() {
            Some(t) => t,
            None => match m {
                Move::Reveal { at, .. } => *at,
                _ => unreachable!("Move::to_square() returned None for a non-Reveal move"),
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_core::rules::RuleSet;
    use chess_core::state::GameState;

    #[test]
    fn find_move_locates_xiangqi_step() {
        let state = GameState::new(RuleSet::xiangqi_casual());
        let view = PlayerView::project(&state, state.side_to_move);
        // Red soldier at file 0 rank 3 should have a step forward to rank 4.
        let from = Square(3 * 9); // rank 3, file 0
        let to = Square(4 * 9); // rank 4, file 0
        let mv = find_move(&view, from, to);
        assert!(matches!(mv, Some(Move::Step { .. })), "expected step from soldier, got {:?}", mv);
    }

    #[test]
    fn legal_targets_for_chariot_in_corner() {
        let state = GameState::new(RuleSet::xiangqi_casual());
        let view = PlayerView::project(&state, state.side_to_move);
        // Red chariot at (file 0, rank 0) — Square(0). Should have several
        // legal destinations along its file (rank moves blocked by horse).
        let targets = legal_targets(&view, Square(0));
        assert!(!targets.is_empty(), "chariot at corner must have legal moves");
    }

    #[test]
    fn legal_targets_empty_for_non_piece_square() {
        let state = GameState::new(RuleSet::xiangqi_casual());
        let view = PlayerView::project(&state, state.side_to_move);
        // Square(5*9 + 4) — a river-ish empty square. Empty piece → no targets.
        let targets = legal_targets(&view, Square(5 * 9 + 4));
        assert!(targets.is_empty());
    }

    #[test]
    fn truncate_front_drops_oldest_when_over_cap() {
        let mut v = vec![1, 2, 3, 4, 5];
        truncate_front(&mut v, 3);
        assert_eq!(v, vec![3, 4, 5]);
    }

    #[test]
    fn truncate_front_noop_when_under_cap() {
        let mut v = vec![1, 2];
        truncate_front(&mut v, 5);
        assert_eq!(v, vec![1, 2]);
    }

    #[test]
    fn client_role_observer_defaults_red_for_spectator() {
        assert_eq!(ClientRole::Player(Side::BLACK).observer(), Side::BLACK);
        assert_eq!(ClientRole::Spectator.observer(), Side::RED);
        assert!(ClientRole::Spectator.is_spectator());
        assert!(ClientRole::Player(Side::RED).is_player());
    }
}
