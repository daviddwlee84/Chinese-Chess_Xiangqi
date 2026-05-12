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
use chess_core::state::GameStatus;
#[cfg(target_arch = "wasm32")]
use leptos::*;

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

    /// Definitive end-of-game sample. Bypasses the cp→% logistic
    /// (which clamps to `[0.01, 0.99]` and would never reach a clean
    /// 100/0) so the eval bar / badge / chart all jump to the actual
    /// outcome the moment the game ends.
    ///
    /// Why this exists: the per-move sample pumps (AI move pump + hint
    /// pump in `pages/local.rs`) bail when `state.status != Ongoing`,
    /// so the last sample they record is from the position **before**
    /// the game-ending move. For a Red-wins-by-general-capture finish
    /// the last AI-pump sample is "Red 10 %" (Black just played a
    /// move it thought was great, before its general got captured),
    /// which leaves the badge confusingly stuck at ~10 % even after
    /// Red's victory banner mounts. See user feedback 2026-05-12.
    ///
    /// `cp_stm_pov` is set to ±MATE-ish to keep the field consistent
    /// with the cp→% relationship the chart Y-axis assumes (so an
    /// `EvalSample` consumer that re-derives `red_win_pct` from
    /// `cp_stm_pov` doesn't disagree with our explicit override).
    /// `side_to_move_at_pos` is whoever was on move when the position
    /// became terminal (i.e. the loser in a `Won` outcome — the side
    /// that has no legal reply).
    pub fn final_outcome(ply: usize, side_to_move: Side, status: &GameStatus) -> Option<Self> {
        // Sentinel cp magnitude — large enough to land in the
        // `cp_to_win_pct` early-out (≥ MATE - 1000) so re-derivation
        // matches our explicit `red_win_pct` to two decimals.
        const TERMINAL_CP: i32 = 30_000;
        let (cp_stm_pov, red_win_pct) = match status {
            GameStatus::Ongoing => return None,
            GameStatus::Drawn { .. } => (0, 0.5),
            GameStatus::Won { winner, .. } => match (*winner, side_to_move) {
                (Side::RED, Side::RED) => (TERMINAL_CP, 0.99),
                (Side::RED, _) => (-TERMINAL_CP, 0.99),
                (Side::BLACK, Side::BLACK) => (TERMINAL_CP, 0.01),
                (Side::BLACK, _) => (-TERMINAL_CP, 0.01),
                // Banqi 3rd colour — treat as "not Red, not Black"
                // and call it 50/50 since the Red-vs-Black axis can't
                // express a Green win cleanly. The end-game banner
                // will still announce the correct winner.
                (_, _) => (0, 0.5),
            },
        };
        Some(Self { ply, side_to_move_at_pos: side_to_move, cp_stm_pov, red_win_pct })
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

/// Push a fresh sample into the per-ply samples vector, replacing any
/// existing entry for the same ply (the freshest analysis wins).
///
/// Both the AI move pump and the hint pump can produce a sample for
/// the same position (vs-AI mode: AI moves, then hint pump fires for
/// the next position; both write samples for adjacent plies, but the
/// hint pump's analysis post-dates the AI pump's). The dedup-by-ply
/// behaviour keeps the vector indexed by ply without duplicates while
/// allowing later analyses to refine the cp value (especially helpful
/// when the AI pump used a quick eval and the hint pump used Hard
/// search at the same depth).
///
/// Wasm-only because the leptos `RwSignal` type isn't available on
/// the native workspace check (chess-web compiles its UI modules
/// `wasm32`-only). The helper itself is the only chess-web code that
/// touches `RwSignal` from a non-wasm-gated module.
#[cfg(target_arch = "wasm32")]
pub fn push_or_replace_sample(samples: RwSignal<Vec<EvalSample>>, sample: EvalSample) {
    samples.update(|v| {
        if let Some(idx) = v.iter().position(|s| s.ply == sample.ply) {
            v[idx] = sample;
        } else {
            v.push(sample);
        }
    });
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

    /// `final_outcome` returns `None` while the game is still going —
    /// the per-move pump is the only producer in the Ongoing state and
    /// we don't want to leak a bogus 50/50 sample if a caller forgets
    /// the status check.
    #[test]
    fn final_outcome_none_when_ongoing() {
        let s = EvalSample::final_outcome(7, Side::RED, &GameStatus::Ongoing);
        assert!(s.is_none());
    }

    /// Red-wins terminal position pins Red to ~99 % regardless of who
    /// is on move at the terminal node. Pins the user-reported bug
    /// (2026-05-12): a Red-by-general-capture finish was leaving the
    /// badge at "Red 10 %" because the last per-move sample was the
    /// pre-loss AI-evaluation of Black thinking it was winning.
    #[test]
    fn final_outcome_red_wins_pins_red_high() {
        use chess_core::state::WinReason;
        for stm in [Side::RED, Side::BLACK] {
            let s = EvalSample::final_outcome(
                39,
                stm,
                &GameStatus::Won { winner: Side::RED, reason: WinReason::GeneralCaptured },
            )
            .expect("terminal sample");
            assert!(
                s.red_win_pct > 0.95,
                "Red wins should pin Red high regardless of stm={:?}, got {}",
                stm,
                s.red_win_pct,
            );
            // Re-deriving from cp must agree with the explicit override
            // (consumers like the chart Y-axis trust this invariant).
            let derived = stm_cp_to_red_win_pct(s.cp_stm_pov, s.side_to_move_at_pos);
            assert!(
                (derived - s.red_win_pct).abs() < 0.05,
                "cp-derived {} should match red_win_pct {}",
                derived,
                s.red_win_pct,
            );
        }
    }

    /// Draw → 50/50.
    #[test]
    fn final_outcome_draw_is_50_50() {
        use chess_core::state::DrawReason;
        let s = EvalSample::final_outcome(
            120,
            Side::RED,
            &GameStatus::Drawn { reason: DrawReason::NoProgress },
        )
        .expect("terminal sample");
        assert!((s.red_win_pct - 0.5).abs() < 1e-6);
        assert_eq!(s.cp_stm_pov, 0);
    }
}
