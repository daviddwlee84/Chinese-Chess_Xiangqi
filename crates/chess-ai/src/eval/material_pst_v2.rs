//! v2 evaluator: material baseline + piece-square tables.
//!
//! Differentiates the static eval beyond raw material so the opening
//! stops looking random. Each piece kind gets one 9×10 table of
//! centipawn deltas keyed by Red's perspective; Black squares are
//! mirrored vertically by [`mirror_rank`].
//!
//! Tables are *clean-room hand-derived* from xiangqi opening principles
//! (centre files for chariot/cannon, advanced + central squares for
//! horse, palace-bound for general/advisor/elephant, graduated soldier
//! ranks). They are NOT copied from any GPL engine — see
//! `docs/ai/v2-material-pst.md` for the per-table heuristic and the
//! decision log on why we chose ±15..30 cp deltas.
//!
//! Future tuning is contained here — the [`Evaluator`] trait isolates
//! the search layer from any change to these numbers.

use chess_core::coord::Square;
use chess_core::piece::{Piece, PieceKind, Side};
use chess_core::state::GameState;

use super::material_v1::piece_value_v1;
use super::Evaluator;

/// Material + PST scorer. Strictly extends [`MaterialV1`](super::material_v1::MaterialV1):
/// at every square `score_v2 = score_v1 + pst_delta(piece, square)`.
#[derive(Default, Clone, Copy, Debug)]
pub struct MaterialPstV2;

impl Evaluator for MaterialPstV2 {
    fn evaluate(&self, state: &GameState) -> i32 {
        let me = state.side_to_move;
        let mut score = 0i32;
        for sq in state.board.squares() {
            let Some(pos) = state.board.get(sq) else { continue };
            if !pos.revealed {
                continue;
            }
            let v = piece_value_v1(state, pos.piece, sq) + pst_delta(state, pos.piece, sq);
            if pos.piece.side == me {
                score += v;
            } else {
                score -= v;
            }
        }
        score
    }

    fn name(&self) -> &'static str {
        "material-pst-v2"
    }
}

/// Centipawn bonus/penalty for placing piece `p` on square `sq`. Looked
/// up from the per-kind table after mirroring the rank for Black so a
/// single Red-perspective table covers both sides.
pub fn pst_delta(state: &GameState, p: Piece, sq: Square) -> i32 {
    let (file, rank) = state.board.file_rank(sq);
    let f = file.0 as usize;
    // Mirror for Black so the same Red-perspective table works either way.
    let r = match p.side {
        Side::RED => rank.0 as usize,
        _ => mirror_rank(rank.0) as usize,
    };
    let table: &[[i8; 9]; 10] = match p.kind {
        PieceKind::General => &PST_GENERAL,
        PieceKind::Advisor => &PST_ADVISOR,
        PieceKind::Elephant => &PST_ELEPHANT,
        PieceKind::Chariot => &PST_CHARIOT,
        PieceKind::Horse => &PST_HORSE,
        PieceKind::Cannon => &PST_CANNON,
        PieceKind::Soldier => &PST_SOLDIER,
    };
    table[r][f] as i32
}

/// Vertical flip of a xiangqi rank (Red rank 0 = Black rank 9, …).
#[inline]
pub const fn mirror_rank(r: u8) -> u8 {
    9 - r
}

// ---------------------------------------------------------------------
// Hand-derived piece-square tables (Red's perspective; rank 0 = Red home,
// rank 9 = Black home). Files left-to-right 0..9 (file 4 = central column).
// Values are *deltas* on top of the v1 material baseline, in centipawns.
// Soldier values DO NOT double-count the v1 +100 "crossed river" bonus —
// these deltas are shaped to layer cleanly on top.
// ---------------------------------------------------------------------

/// 將/帥. Confined to the 3×3 palace. Tiny incentive to stay home rank
/// rather than flying to the palace edge (where general-face exposes).
#[rustfmt::skip]
const PST_GENERAL: [[i8; 9]; 10] = [
    [0, 0, 0,  4,  6,  4, 0, 0, 0],
    [0, 0, 0,  2,  4,  2, 0, 0, 0],
    [0, 0, 0,  0,  2,  0, 0, 0, 0],
    [0; 9],
    [0; 9],
    [0; 9],
    [0; 9],
    [0; 9],
    [0; 9],
    [0; 9],
];

/// 仕/士. Stays in palace. Centre square slightly preferred for
/// double-defence of the general.
#[rustfmt::skip]
const PST_ADVISOR: [[i8; 9]; 10] = [
    [0, 0, 0,  3, 0,  3, 0, 0, 0],
    [0, 0, 0,  0, 5,  0, 0, 0, 0],
    [0, 0, 0,  3, 0,  3, 0, 0, 0],
    [0; 9],
    [0; 9],
    [0; 9],
    [0; 9],
    [0; 9],
    [0; 9],
    [0; 9],
];

/// 相/象. Three-step diagonals, can't cross river. Reward central elephant
/// (file 2/4/6) over flank-only (file 0/8).
#[rustfmt::skip]
const PST_ELEPHANT: [[i8; 9]; 10] = [
    [0, 0,  6, 0, 0, 0,  6, 0, 0],
    [0, 0,  0, 0, 0, 0,  0, 0, 0],
    [4, 0,  0, 0,10, 0,  0, 0, 4],
    [0, 0,  0, 0, 0, 0,  0, 0, 0],
    [0, 0,  6, 0, 0, 0,  6, 0, 0],
    [0; 9],
    [0; 9],
    [0; 9],
    [0; 9],
    [0; 9],
];

/// 車. Open-line piece. Reward presence on advanced ranks and on
/// central + flank files (file 0,4,8 are open or semi-open most often).
/// Modest values — the chariot's strength is mostly in its 900cp material.
#[rustfmt::skip]
const PST_CHARIOT: [[i8; 9]; 10] = [
    [-2, 10,  6, 14, 12, 14,  6, 10, -2],
    [ 8, 14, 12, 16, 14, 16, 12, 14,  8],
    [ 4, 10,  8, 14, 12, 14,  8, 10,  4],
    [ 6, 12, 10, 16, 14, 16, 10, 12,  6],
    [ 4, 10,  8, 14, 14, 14,  8, 10,  4],
    [ 4, 10,  8, 14, 14, 14,  8, 10,  4],
    [ 6, 12, 10, 16, 16, 16, 10, 12,  6],
    [ 8, 14, 12, 18, 16, 18, 12, 14,  8],
    [10, 16, 14, 20, 18, 20, 14, 16, 10],
    [ 4, 10,  8, 14, 12, 14,  8, 10,  4],
];

/// 馬. Knight wants centre + advanced. Heavy bonus for crossing river
/// (where it forks deep). Penalty for staying on rank 0 (passive).
#[rustfmt::skip]
const PST_HORSE: [[i8; 9]; 10] = [
    [-4,  0,  4,  6,  4,  6,  4,  0, -4],
    [ 0,  6, 10, 14, 10, 14, 10,  6,  0],
    [ 2,  8, 14, 18, 16, 18, 14,  8,  2],
    [ 4, 10, 16, 22, 20, 22, 16, 10,  4],
    [ 4, 12, 18, 24, 24, 24, 18, 12,  4],
    [ 6, 14, 20, 26, 28, 26, 20, 14,  6],
    [ 6, 14, 22, 28, 30, 28, 22, 14,  6],
    [ 4, 12, 18, 22, 22, 22, 18, 12,  4],
    [ 0,  6, 12, 16, 16, 16, 12,  6,  0],
    [-4, -2,  2,  4,  6,  4,  2, -2, -4],
];

/// 炮. Wants ranks where a screen exists. Centre file is classic 中炮.
/// Reward middle ranks; mild penalty for back-rank passivity.
#[rustfmt::skip]
const PST_CANNON: [[i8; 9]; 10] = [
    [ 4,  6,  6,  8,  8,  8,  6,  6,  4],
    [ 6,  8, 10, 12, 14, 12, 10,  8,  6],
    [ 4,  6, 10, 14, 18, 14, 10,  6,  4],
    [ 2,  4,  8, 12, 14, 12,  8,  4,  2],
    [ 0,  2,  4,  6,  8,  6,  4,  2,  0],
    [ 0,  2,  4,  6,  8,  6,  4,  2,  0],
    [ 2,  4,  6,  8, 10,  8,  6,  4,  2],
    [ 4,  6,  8, 10, 14, 10,  8,  6,  4],
    [ 6,  8, 10, 12, 16, 12, 10,  8,  6],
    [ 4,  4,  6,  8, 10,  8,  6,  4,  4],
];

/// 兵/卒. Pre-river soldiers are weakly placed (already covered by v1's
/// uncrossed +100); post-river they fan out and become valuable.
/// These deltas reward rank progress and the central files where a
/// soldier threatens the palace.
#[rustfmt::skip]
const PST_SOLDIER: [[i8; 9]; 10] = [
    [ 0; 9],
    [ 0; 9],
    [ 0; 9],
    [ 4,  0,  6,  0,  6,  0,  6,  0,  4],
    [ 6,  6,  8,  8, 10,  8,  8,  6,  6],
    [10, 14, 16, 18, 20, 18, 16, 14, 10],
    [14, 18, 22, 26, 28, 26, 22, 18, 14],
    [18, 22, 26, 30, 32, 30, 26, 22, 18],
    [20, 24, 28, 32, 34, 32, 28, 24, 20],
    [16, 20, 24, 28, 30, 28, 24, 20, 16],
];

#[cfg(test)]
mod tests {
    use super::*;
    use chess_core::board::Board;
    use chess_core::coord::{File, Rank};
    use chess_core::piece::{Piece, PieceKind};
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

    #[test]
    fn red_horse_central_advanced_better_than_corner() {
        let state = empty_state();
        let h = Piece::new(Side::RED, PieceKind::Horse);
        let central = state.board.sq(File(4), Rank(5));
        let corner = state.board.sq(File(0), Rank(0));
        assert!(pst_delta(&state, h, central) > pst_delta(&state, h, corner));
    }

    #[test]
    fn black_pst_mirrors_red() {
        let state = empty_state();
        let red_horse = Piece::new(Side::RED, PieceKind::Horse);
        let black_horse = Piece::new(Side::BLACK, PieceKind::Horse);
        let red_advanced = state.board.sq(File(4), Rank(6));
        let black_advanced = state.board.sq(File(4), Rank(3));
        assert_eq!(
            pst_delta(&state, red_horse, red_advanced),
            pst_delta(&state, black_horse, black_advanced)
        );
    }

    #[test]
    fn central_cannon_better_than_flank() {
        let state = empty_state();
        let c = Piece::new(Side::RED, PieceKind::Cannon);
        let central = state.board.sq(File(4), Rank(2));
        let flank = state.board.sq(File(0), Rank(2));
        assert!(pst_delta(&state, c, central) > pst_delta(&state, c, flank));
    }

    #[test]
    fn general_outside_palace_is_zero() {
        let state = empty_state();
        let g = Piece::new(Side::RED, PieceKind::General);
        // (file 0, rank 9) — outside palace, table should be 0.
        let sq = state.board.sq(File(0), Rank(9));
        assert_eq!(pst_delta(&state, g, sq), 0);
    }

    #[test]
    fn advanced_soldier_beats_river_soldier() {
        let state = empty_state();
        let s = Piece::new(Side::RED, PieceKind::Soldier);
        let river = state.board.sq(File(4), Rank(5));
        let advanced = state.board.sq(File(4), Rank(8));
        assert!(pst_delta(&state, s, advanced) > pst_delta(&state, s, river));
    }
}
