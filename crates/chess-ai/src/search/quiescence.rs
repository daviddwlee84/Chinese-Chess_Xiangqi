//! Quiescence search.
//!
//! At horizon nodes (depth = 0 in the main search), instead of returning
//! a static eval — which can be wildly off mid-capture — recurse a
//! capture-only search until the position is "quiet" (no captures
//! available). Standard fix for the "horizon effect" blunder where the
//! main search saw "I take the chariot" but didn't see "you take my
//! chariot back next move".
//!
//! Implemented as a stand-pat negamax: at each node we first take the
//! static eval as a lower bound (the side to move can always *not*
//! capture and accept the current eval). If that already exceeds beta
//! we return immediately; otherwise we explore captures sorted by
//! MVV-LVA, alpha-betaing as we go.
//!
//! Bounded by [`Q_MAX_PLIES`] so a long capture chain doesn't blow the
//! stack or the wall-clock. In practice xiangqi midgames terminate
//! quiescence within 4-6 plies.

use chess_core::moves::Move;
use chess_core::state::GameState;

use crate::eval::Evaluator;
use crate::search::is_capture;
use crate::search::ordering::mvv_lva_score;

/// Hard cap on quiescence recursion depth. 12 plies is more than any
/// realistic capture chain in xiangqi (the longest 連吃 chains are
/// banqi-only and capped by piece count there too).
pub const Q_MAX_PLIES: u8 = 12;

/// Capture-only search starting from `state`. Side-to-move-relative
/// score, same convention as the main negamax.
///
/// `nodes` is the shared budget counter from the main search; we
/// increment it once per recursion. Quiescence respects the same
/// `NODE_BUDGET` cutoff as the main search.
pub fn quiescence<E: Evaluator>(
    state: &mut GameState,
    mut alpha: i32,
    beta: i32,
    nodes: &mut u32,
    eval: &E,
    plies_left: u8,
) -> i32 {
    *nodes = nodes.saturating_add(1);
    if *nodes >= crate::search::NODE_BUDGET {
        return eval.evaluate(state);
    }

    // Stand-pat: assume the side to move can opt out of all captures and
    // accept the static eval as a lower bound.
    let stand_pat = eval.evaluate(state);
    if stand_pat >= beta {
        return beta;
    }
    if stand_pat > alpha {
        alpha = stand_pat;
    }

    if plies_left == 0 {
        return alpha;
    }

    // Captures only, MVV-LVA ordered.
    let moves = state.legal_moves();
    let mut captures: Vec<(i32, Move)> =
        moves.into_iter().filter(is_capture).map(|m| (mvv_lva_score(state, &m), m)).collect();
    captures.sort_by_key(|(s, _)| std::cmp::Reverse(*s));

    for (_score, mv) in captures {
        if state.make_move(&mv).is_err() {
            continue;
        }
        let v = -quiescence(state, -beta, -alpha, nodes, eval, plies_left - 1);
        let _ = state.unmake_move();
        if v >= beta {
            return beta;
        }
        if v > alpha {
            alpha = v;
        }
    }
    alpha
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_core::board::Board;
    use chess_core::coord::{File, Rank, Square};
    use chess_core::piece::{Piece, PieceKind, PieceOnSquare, Side};
    use chess_core::rules::RuleSet;

    use crate::eval::material_king_safety_pst_v3::MaterialKingSafetyPstV3;

    fn empty_state() -> GameState {
        let mut state = GameState::new(RuleSet::xiangqi_casual());
        let board: Board = state.board.clone();
        let squares: Vec<Square> = board.squares().collect();
        for sq in squares {
            state.board.set(sq, None);
        }
        state
    }

    #[test]
    fn quiet_position_returns_static_eval() {
        // No captures available → quiescence == evaluate.
        let mut state = empty_state();
        let red_gen = state.board.sq(File(4), Rank(0));
        let blk_gen = state.board.sq(File(4), Rank(9));
        state
            .board
            .set(red_gen, Some(PieceOnSquare::revealed(Piece::new(Side::RED, PieceKind::General))));
        state.board.set(
            blk_gen,
            Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::General))),
        );
        let eval = MaterialKingSafetyPstV3;
        let static_eval = eval.evaluate(&state);
        let mut nodes = 0u32;
        let q = quiescence(&mut state, -1_000_000, 1_000_000, &mut nodes, &eval, Q_MAX_PLIES);
        assert_eq!(q, static_eval, "quiet position quiescence should equal static eval");
    }

    #[test]
    fn quiescence_sees_recapture_in_capture_chain() {
        // Red chariot (file 0, rank 4) attacks Black soldier (file 0, rank 5)
        // which is defended by Black chariot at (file 0, rank 6). Static
        // eval after Red plays the capture = "Red is +100" (won soldier).
        // Quiescence should follow up: Black recaptures with chariot,
        // ends with Red minus chariot, gain a soldier, net = -800 cp from
        // Red's POV. So quiescence(state with Red to move) should NOT
        // return +100; it should return ~ static_eval (Red declines to
        // initiate the bad trade) or some negative number.
        let mut state = empty_state();
        let red_gen = state.board.sq(File(4), Rank(0));
        let blk_gen = state.board.sq(File(4), Rank(9));
        state
            .board
            .set(red_gen, Some(PieceOnSquare::revealed(Piece::new(Side::RED, PieceKind::General))));
        state.board.set(
            blk_gen,
            Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::General))),
        );
        let red_chariot = state.board.sq(File(0), Rank(4));
        let blk_soldier = state.board.sq(File(0), Rank(5));
        let blk_chariot = state.board.sq(File(0), Rank(6));
        state.board.set(
            red_chariot,
            Some(PieceOnSquare::revealed(Piece::new(Side::RED, PieceKind::Chariot))),
        );
        state.board.set(
            blk_soldier,
            Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::Soldier))),
        );
        state.board.set(
            blk_chariot,
            Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::Chariot))),
        );

        let eval = MaterialKingSafetyPstV3;
        let static_eval = eval.evaluate(&state);
        let mut nodes = 0u32;
        let q = quiescence(&mut state, -1_000_000, 1_000_000, &mut nodes, &eval, Q_MAX_PLIES);
        // Should be at most the stand-pat (Red opts out of the bad
        // trade). Crucially: NOT +100 (the naive 1-ply view of the
        // capture).
        assert!(
            q <= static_eval + 50,
            "quiescence should see the recapture and decline; got {} vs static {}",
            q,
            static_eval
        );
    }
}
