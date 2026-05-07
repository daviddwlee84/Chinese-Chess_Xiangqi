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

/// True if `view.chain_lock` is set and the click on `sq` should commit
/// `Move::EndChain` (e.g. user clicks the locked piece itself to release
/// the chain). Otherwise the click should attempt a further capture.
pub fn end_chain_move(view: &PlayerView) -> Option<Move> {
    view.chain_lock.map(|at| Move::EndChain { at })
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

    #[test]
    fn end_chain_move_returns_some_when_chain_lock_set() {
        use chess_core::board::Board;
        use chess_core::coord::{File, Rank, Square};
        use chess_core::piece::{Piece, PieceKind, PieceOnSquare};
        use chess_core::rules::{HouseRules, RuleSet};
        use chess_core::state::{GameState, SideAssignment};
        use smallvec::smallvec;

        let mut state = GameState::new(RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0));
        let squares: Vec<Square> = state.board.squares().collect();
        for sq in squares {
            state.board.set(sq, None);
        }
        state.side_assignment = Some(SideAssignment { mapping: smallvec![Side::RED, Side::BLACK] });
        let _ = Board::new(state.board.shape()); // sanity

        let h = state.board.sq(File(1), Rank(1));
        let s1 = state.board.sq(File(1), Rank(2));
        let s2 = state.board.sq(File(1), Rank(3));
        state.board.set(h, Some(PieceOnSquare::revealed(Piece::new(Side::RED, PieceKind::Horse))));
        state
            .board
            .set(s1, Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::Soldier))));
        state
            .board
            .set(s2, Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::Soldier))));

        let cap = Move::Capture {
            from: h,
            to: s1,
            captured: Piece::new(Side::BLACK, PieceKind::Soldier),
        };
        state.make_move(&cap).unwrap();
        assert!(state.chain_lock.is_some());

        let view = chess_core::view::PlayerView::project(&state, Side::RED);
        assert_eq!(view.chain_lock, Some(s1));
        assert!(matches!(end_chain_move(&view), Some(Move::EndChain { at }) if at == s1));
    }

    #[test]
    fn end_chain_move_returns_none_when_no_chain_lock() {
        let state = GameState::new(RuleSet::xiangqi_casual());
        let view = chess_core::view::PlayerView::project(&state, state.side_to_move);
        assert!(end_chain_move(&view).is_none());
    }
}
