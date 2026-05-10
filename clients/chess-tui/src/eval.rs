//! Win-rate sample collection — chess-tui mirror of chess-web's
//! `clients/chess-web/src/eval.rs`. The two structs are byte-identical
//! by intent; promoting them to a shared client crate is tracked in the
//! `promote-client-shared` backlog. Both pull `cp_to_win_pct` from
//! chess-ai so the numerical result is consistent across the two
//! frontends.

use chess_core::piece::Side;

/// One AI evaluation snapshot taken after a specific ply was played.
///
/// `ply == 0` represents the **initial position** (no recorded sample
/// today — chess-tui doesn't run an opening analyze, so the first
/// sample is `ply == 1` after the first move). `side_to_move_at_pos`
/// is whoever is about to move at this position (i.e., the side the
/// AI was scoring).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EvalSample {
    pub ply: usize,
    pub side_to_move_at_pos: Side,
    pub cp_stm_pov: i32,
    /// Pre-computed Red-side win probability in `[0.01, 0.99]`.
    pub red_win_pct: f32,
}

impl EvalSample {
    pub fn new(ply: usize, side_to_move: Side, cp_stm_pov: i32) -> Self {
        let stm_pct = chess_ai::cp_to_win_pct(cp_stm_pov);
        let red_win_pct = match side_to_move {
            Side::RED => stm_pct,
            _ => 1.0 - stm_pct,
        };
        Self { ply, side_to_move_at_pos: side_to_move, cp_stm_pov, red_win_pct }
    }

    /// Black's win probability — derived (always `1 - red_win_pct`).
    /// Provided for symmetry with chess-web's struct, even though the
    /// chess-tui ASCII renderer prefers to compute the integer
    /// percentage directly from `red_win_pct`.
    #[allow(dead_code)]
    pub fn black_win_pct(&self) -> f32 {
        1.0 - self.red_win_pct
    }
}
