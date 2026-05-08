//! v3 evaluator: v2 (material + PSTs) + king safety.
//!
//! ## Why v3 exists
//!
//! v1 and v2 both evaluate the General as worth 0 cp on the assumption
//! that "checkmate is handled by the mate score in negamax". This works
//! in **strict** xiangqi rules (`xiangqi_allow_self_check = false`)
//! because the legality filter rejects any move that would leave the
//! mover's General in check, so the General is *physically* never
//! captured during search.
//!
//! In **casual** xiangqi rules (`xiangqi_allow_self_check = true`,
//! which is what the picker / `RuleSet::xiangqi_casual()` defaults to)
//! the legality filter is off. Capturing the General becomes a real
//! move, the game ends with `WinReason::GeneralCaptured`, and `legal_moves`
//! does NOT go empty after the capture (the side that just lost its
//! General still has chariots, horses, etc. to move). So the
//! "no legal moves → mate score" terminal in `search/mod.rs::negamax`
//! is never reached, and the AI happily makes moves that hand the
//! opponent a 1-ply mate — see
//! [`pitfalls/casual-xiangqi-king-blindness.md`](../../../../pitfalls/casual-xiangqi-king-blindness.md).
//!
//! Fix: give the General a piece value large enough to dominate any
//! plausible material configuration (50_000 cp >> Σ all xiangqi
//! material ≈ 9_600 cp) but small enough not to overflow into the
//! mate-score range used by negamax (MATE = 1_000_000 in
//! `search/mod.rs`). Now "Black is missing the General" is a
//! 50k-cp swing in Red's favour, propagates through negamax cleanly,
//! and the search avoids losing-General lines.
//!
//! Everything else is identical to v2 — same PSTs, same search,
//! same difficulty mapping. So v3 is a strict superset of v2 in
//! terms of strength: never weaker, fixes one specific class of
//! blunder.
//!
//! See `docs/ai/v3-king-safety-pst.md` for the full version doc.

use chess_core::coord::Square;
use chess_core::piece::{Piece, PieceKind, Side};
use chess_core::state::GameState;

use super::material_pst_v2::pst_delta;
use super::Evaluator;

/// Centipawn value of the General/帥. Chosen so:
/// - `KING_VALUE > Σ all other material on the board` — the search
///   prefers losing every other piece over losing the General.
/// - `KING_VALUE < MATE / 2` — leaves headroom for negamax's mate
///   scores (`-MATE + depth`) to remain distinguishable from "lost
///   the General" eval scores. With MATE = 1_000_000 and
///   KING_VALUE = 50_000, that's 20× headroom.
pub const KING_VALUE: i32 = 50_000;

/// Material baseline + PSTs (v2) + General has [`KING_VALUE`] instead
/// of 0. That's the entire delta from v2.
#[derive(Default, Clone, Copy, Debug)]
pub struct MaterialKingSafetyPstV3;

impl Evaluator for MaterialKingSafetyPstV3 {
    fn evaluate(&self, state: &GameState) -> i32 {
        let me = state.side_to_move;
        let mut score = 0i32;
        for sq in state.board.squares() {
            let Some(pos) = state.board.get(sq) else { continue };
            if !pos.revealed {
                continue;
            }
            let v = piece_value_v3(state, pos.piece, sq) + pst_delta(state, pos.piece, sq);
            if pos.piece.side == me {
                score += v;
            } else {
                score -= v;
            }
        }
        score
    }

    fn name(&self) -> &'static str {
        "material-king-safety-pst-v3"
    }
}

/// Same as v1's `piece_value_v1` except the General now has
/// [`KING_VALUE`] instead of 0. Free function so a hypothetical v4
/// (king-safety + tempo or similar) can compose this.
pub fn piece_value_v3(state: &GameState, p: Piece, sq: Square) -> i32 {
    match p.kind {
        PieceKind::General => KING_VALUE,
        PieceKind::Advisor => 200,
        PieceKind::Elephant => 200,
        PieceKind::Chariot => 900,
        PieceKind::Horse => 400,
        PieceKind::Cannon => 450,
        PieceKind::Soldier => {
            let (_, rank) = state.board.file_rank(sq);
            let crossed = match p.side {
                Side::RED => rank.0 >= 5,
                _ => rank.0 <= 4,
            };
            if crossed {
                200
            } else {
                100
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_core::board::Board;
    use chess_core::coord::{File, Rank, Square};
    use chess_core::piece::{Piece, PieceKind, PieceOnSquare};
    use chess_core::rules::RuleSet;

    fn empty_state() -> GameState {
        let mut state = GameState::new(RuleSet::xiangqi_casual());
        let board: Board = state.board.clone();
        let squares: Vec<Square> = board.squares().collect();
        for sq in squares {
            state.board.set(sq, None);
        }
        state
    }

    /// Sanity: KING_VALUE dominates the sum of all other xiangqi material.
    #[test]
    fn king_value_dominates_full_material() {
        // Total non-king material: 2 chariots (1800) + 2 cannons (900)
        // + 2 horses (800) + 2 advisors (400) + 2 elephants (400)
        // + 5 soldiers (500-1000 depending on rank). Worst case ≈ 5300/side.
        // KING_VALUE = 50_000 must be >> that.
        let max_side_material = 1800 + 900 + 800 + 400 + 400 + 1000;
        assert!(
            KING_VALUE > max_side_material * 4,
            "KING_VALUE={} should dominate side material {} by a wide margin",
            KING_VALUE,
            max_side_material
        );
    }

    /// Same position, only difference is one side missing the General.
    /// Eval should swing by approximately ±KING_VALUE.
    #[test]
    fn missing_general_swings_eval_by_king_value() {
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
        let both = MaterialKingSafetyPstV3.evaluate(&state);

        // Remove Black's General. side_to_move is Red, so Red just
        // gained KING_VALUE worth of material relative to Black.
        state.board.set(blk_gen, None);
        let only_red = MaterialKingSafetyPstV3.evaluate(&state);

        let swing = only_red - both;
        // Allow some PST slack but must be approximately KING_VALUE.
        assert!(
            (swing - KING_VALUE).abs() < 100,
            "expected ~{} swing from removing Black general; got {}",
            KING_VALUE,
            swing
        );
    }
}
