//! v1 evaluator: material only.
//!
//! Original 2026-05-08 MVP eval. Preserved verbatim so `?engine=v1` still
//! reproduces the historical behaviour bit-for-bit. Subsequent versions
//! (v2 = + PSTs, v3 = + king-safety / mobility, …) are additive.
//!
//! See `docs/ai/v1-material.md` for the full spec.

use chess_core::coord::Square;
use chess_core::piece::{Piece, PieceKind, Side};
use chess_core::state::GameState;

use super::Evaluator;

/// Material-only scorer. No piece-square tables, no king safety, no
/// mobility — only `Σ piece_value`. Soldiers get a +100 bonus once they
/// cross the river (the only positional term in v1).
#[derive(Default, Clone, Copy, Debug)]
pub struct MaterialV1;

impl Evaluator for MaterialV1 {
    fn evaluate(&self, state: &GameState) -> i32 {
        evaluate_material_v1(state)
    }

    fn name(&self) -> &'static str {
        "material-v1"
    }
}

/// Original eval body from `chess-ai` 2026-05-08. Free function so the
/// search has a stable hot-path symbol and so v2 can compose this into
/// `material + pst` without re-deriving the material table.
pub fn evaluate_material_v1(state: &GameState) -> i32 {
    let me = state.side_to_move;
    let mut score = 0i32;
    for sq in state.board.squares() {
        let Some(pos) = state.board.get(sq) else { continue };
        if !pos.revealed {
            continue;
        }
        let v = piece_value_v1(state, pos.piece, sq);
        if pos.piece.side == me {
            score += v;
        } else {
            score -= v;
        }
    }
    score
}

/// Side-agnostic piece value for v1. Exported so v2 can reuse the
/// material baseline and only layer PST deltas on top.
pub fn piece_value_v1(state: &GameState, p: Piece, sq: Square) -> i32 {
    match p.kind {
        // General is excluded from material — checkmate is handled by the
        // mate score in negamax. Casual xiangqi (where capturing the
        // general is the loss condition) still works because legal_moves
        // going empty is the search recursion's terminal.
        PieceKind::General => 0,
        PieceKind::Advisor => 200,
        PieceKind::Elephant => 200,
        PieceKind::Chariot => 900,
        PieceKind::Horse => 400,
        PieceKind::Cannon => 450,
        PieceKind::Soldier => {
            let (_, rank) = state.board.file_rank(sq);
            // Xiangqi river: ranks 0-4 are Red half, 5-9 are Black half.
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
