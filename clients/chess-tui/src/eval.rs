//! Win-rate sample collection — chess-tui mirror of chess-web's
//! `clients/chess-web/src/eval.rs`. The two structs are byte-identical
//! by intent; promoting them to a shared client crate is tracked in the
//! `promote-client-shared` backlog. Both pull `cp_to_win_pct` from
//! chess-ai so the numerical result is consistent across the two
//! frontends.

use chess_core::piece::Side;
use chess_core::state::GameStatus;

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

    /// Definitive end-of-game sample. Mirrors chess-web's
    /// `EvalSample::final_outcome`: the per-move sample writers in
    /// `app.rs` (apply_move / ai_reply) bail when the game ends, so
    /// the last sample they record is from the position **before**
    /// the game-ending move. For a Red-wins-by-general-capture finish
    /// that leaves the headline / chart frozen at the AI's pre-loss
    /// optimism. Pushing one of these on the Ongoing→ended transition
    /// jumps the panel to the actual outcome (100/0 or 50/50).
    ///
    /// `cp_stm_pov` is set to ±MATE-ish (matching `cp_to_win_pct`'s
    /// early-out band) so a consumer that re-derives `red_win_pct`
    /// from `cp_stm_pov` agrees with our explicit override.
    pub fn final_outcome(ply: usize, side_to_move: Side, status: &GameStatus) -> Option<Self> {
        const TERMINAL_CP: i32 = 30_000;
        let (cp_stm_pov, red_win_pct) = match status {
            GameStatus::Ongoing => return None,
            GameStatus::Drawn { .. } => (0, 0.5),
            GameStatus::Won { winner, .. } => match (*winner, side_to_move) {
                (Side::RED, Side::RED) => (TERMINAL_CP, 0.99),
                (Side::RED, _) => (-TERMINAL_CP, 0.99),
                (Side::BLACK, Side::BLACK) => (TERMINAL_CP, 0.01),
                (Side::BLACK, _) => (-TERMINAL_CP, 0.01),
                // Banqi 3rd colour — see chess-web's mirror; can't be
                // expressed cleanly on a Red-vs-Black axis.
                (_, _) => (0, 0.5),
            },
        };
        Some(Self { ply, side_to_move_at_pos: side_to_move, cp_stm_pov, red_win_pct })
    }
}
