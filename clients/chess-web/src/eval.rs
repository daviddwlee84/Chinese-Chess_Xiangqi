//! Win-rate sample collection and conversion helpers.
//!
//! Driven by `pages/local.rs` (the only producer today): when an AI
//! pump or hint pump completes a `chess_ai::analyze` call after a new
//! ply has been played, the page pushes one [`EvalSample`] per ply
//! into a `RwSignal<Vec<EvalSample>>`. The eval-bar / sidebar-badge
//! components read the most-recent sample for live display; the
//! end-game chart reads the entire vector to draw the trend line.
//!
//! All samples are normalised to **red-side POV** (Y-axis convention)
//! so the chart and bar render consistently regardless of whose turn
//! it was when the sample was taken. Conversion happens at the
//! producer side via [`stm_cp_to_red_win_pct`].
//!
//! Lives in `chess-web` (not `chess-ai`) because the data model is
//! UI-shaped (`Vec<sample>` indexed by ply, not e.g. a hashmap by
//! position hash). The cp→% conversion itself lives in `chess-ai`
//! ([`chess_ai::cp_to_win_pct`]) so chess-tui can share the math.

use chess_core::piece::Side;

/// One AI evaluation snapshot taken after a specific ply was played.
///
/// `ply == 0` represents the **initial position** before any move
/// (recorded once on game start when the eval-bar flag is enabled);
/// `ply == n` represents the position **after** the n-th half-move.
/// `side_to_move_at_pos` is whoever is on move *at this position* —
/// i.e., the side the AI was scoring when this sample was produced.
///
/// Cheap (16 bytes) — kept by-value in `Vec<EvalSample>`. Cloning a
/// 200-ply game's worth costs ~3 KB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EvalSample {
    pub ply: usize,
    pub side_to_move_at_pos: Side,
    /// Raw centipawn score from `chess_ai::analyze` (side-relative —
    /// positive favours `side_to_move_at_pos`).
    pub cp_stm_pov: i32,
    /// Pre-computed Red-side win probability in `[0.01, 0.99]`. The
    /// chart Y-axis and the eval-bar marker both read this directly so
    /// the conversion math runs once at sample time, not per-render.
    pub red_win_pct: f32,
}

impl EvalSample {
    /// Build a sample by converting a side-relative cp score to the
    /// Red-POV win probability the UI consumes.
    pub fn new(ply: usize, side_to_move: Side, cp_stm_pov: i32) -> Self {
        Self {
            ply,
            side_to_move_at_pos: side_to_move,
            cp_stm_pov,
            red_win_pct: stm_cp_to_red_win_pct(cp_stm_pov, side_to_move),
        }
    }

    /// Black-side win probability — derived (always `1 - red_win_pct`).
    /// The bar / badge consumers prefer this directly over inverting
    /// `red_win_pct` at render time.
    pub fn black_win_pct(&self) -> f32 {
        1.0 - self.red_win_pct
    }
}

/// Convert a side-to-move-relative centipawn score to Red's win
/// probability. Wraps [`chess_ai::cp_to_win_pct`] with the side flip
/// so callers don't have to remember which way to negate.
pub fn stm_cp_to_red_win_pct(cp_stm_pov: i32, side_to_move: Side) -> f32 {
    let stm_pct = chess_ai::cp_to_win_pct(cp_stm_pov);
    match side_to_move {
        Side::RED => stm_pct,
        _ => 1.0 - stm_pct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Red-POV invariant: positive cp + Red-to-move ↔ negative cp +
    /// Black-to-move ↔ same Red win %. Pin so a future refactor that
    /// swaps the negation breaks loud.
    #[test]
    fn red_win_pct_symmetric_across_side_negation() {
        let red_pct = stm_cp_to_red_win_pct(300, Side::RED);
        let black_pct = stm_cp_to_red_win_pct(-300, Side::BLACK);
        assert!(
            (red_pct - black_pct).abs() < 1e-6,
            "+300 cp Red-to-move ({}) should equal -300 cp Black-to-move ({})",
            red_pct,
            black_pct,
        );
        // And both should be > 50 % (Red is winning either way).
        assert!(red_pct > 0.5, "Red should be winning: {}", red_pct);
    }

    /// Even position (cp = 0) should be ~50 % regardless of side.
    #[test]
    fn red_win_pct_50_50_when_even() {
        let red = stm_cp_to_red_win_pct(0, Side::RED);
        let black = stm_cp_to_red_win_pct(0, Side::BLACK);
        assert!((red - 0.5).abs() < 1e-6);
        assert!((black - 0.5).abs() < 1e-6);
    }

    /// Sample's `red_win_pct` matches its `cp_stm_pov` + `side_to_move`
    /// inputs (no drift between the constructor and direct conversion).
    #[test]
    fn sample_red_win_pct_matches_helper() {
        let s = EvalSample::new(7, Side::BLACK, 250);
        assert_eq!(s.red_win_pct, stm_cp_to_red_win_pct(250, Side::BLACK));
        assert!((s.red_win_pct + s.black_win_pct() - 1.0).abs() < 1e-6);
    }

    /// Edge case — mate-in-N for Black on Black's turn = Red's losing
    /// = clamped to 0.01 Red win %.
    #[test]
    fn red_win_pct_mate_for_black_clamps_red_to_1pct() {
        let s = EvalSample::new(42, Side::BLACK, chess_ai::WIN_PCT_K as i32 * 100);
        assert!(s.red_win_pct < 0.05, "Black mating should drop Red %, got {}", s.red_win_pct);
    }
}
