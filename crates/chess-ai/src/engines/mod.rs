//! Engine implementations.
//!
//! An [`Engine`] picks a move from a [`GameState`] given [`AiOptions`].
//! Plumbing is intentionally thin: each engine wires a chosen
//! [`Evaluator`](crate::eval::Evaluator) into [`crate::search`] and
//! applies the difficulty randomisation policy.
//!
//! Why one module per version (rather than a single configurable
//! engine): "switchable, non-overwriting" — when v3 adds iterative
//! deepening or v4 adds quiescence, the older engines stay byte-for-byte
//! reachable for regression / repro / comparative play. See
//! `docs/ai/README.md` for the version index.

use chess_core::state::GameState;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::eval::material_king_safety_pst_v3::MaterialKingSafetyPstV3;
use crate::eval::material_pst_v2::MaterialPstV2;
use crate::eval::material_v1::MaterialV1;
use crate::eval::Evaluator;
use crate::search::v5::score_root_moves_v5;
use crate::search::{score_root_moves, score_root_moves_qmvv, ScoredMove};
use crate::{AiAnalysis, AiMoveResult, AiOptions, Randomness, Strategy};

/// A search engine: picks a move for the side to move in `state`.
///
/// `Some(_)` only when there's at least one legal move and the game is
/// not already terminated. Engines must never mutate `state` (they may
/// clone).
pub trait Engine {
    fn choose_move(&self, state: &GameState, opts: &AiOptions) -> Option<AiMoveResult>;
    /// Stable short tag used by the picker UI and logs.
    fn name(&self) -> &'static str;
}

/// v1 (2026-05-08 MVP): negamax + α-β + capture-first + material-only eval.
#[derive(Default, Clone, Copy, Debug)]
pub struct NegamaxV1;

impl Engine for NegamaxV1 {
    fn choose_move(&self, state: &GameState, opts: &AiOptions) -> Option<AiMoveResult> {
        run(state, opts, &MaterialV1, "negamax-v1")
    }
    fn name(&self) -> &'static str {
        "negamax-v1"
    }
}

/// v2: same search as v1, but evaluator now layers piece-square tables
/// on top of material. Default engine since 2026-05-08.
#[derive(Default, Clone, Copy, Debug)]
pub struct NegamaxV2;

impl Engine for NegamaxV2 {
    fn choose_move(&self, state: &GameState, opts: &AiOptions) -> Option<AiMoveResult> {
        run(state, opts, &MaterialPstV2, "negamax-v2")
    }
    fn name(&self) -> &'static str {
        "negamax-v2"
    }
}

/// v3 (2026-05-09): same search + PSTs as v2, but the evaluator gives
/// the General a 50_000 cp value (instead of 0) so casual-rules games
/// no longer let the AI walk into 1-ply mates by ignoring the King.
/// Was the default 2026-05-09 → 2026-05-09; superseded by v4 because
/// horizon-effect blunders on captures still slipped through.
#[derive(Default, Clone, Copy, Debug)]
pub struct NegamaxV3;

impl Engine for NegamaxV3 {
    fn choose_move(&self, state: &GameState, opts: &AiOptions) -> Option<AiMoveResult> {
        run(state, opts, &MaterialKingSafetyPstV3, "negamax-v3")
    }
    fn name(&self) -> &'static str {
        "negamax-v3"
    }
}

/// v4 (2026-05-09): same v3 evaluator (material + PSTs + king safety),
/// but the search now uses MVV-LVA capture ordering and a quiescence
/// search at the horizon. Stops the "AI wins a chariot then loses it
/// back next move" class of horizon-effect blunder.
/// Default since 2026-05-09. See `docs/ai/v4-quiescence-mvv-lva.md`.
#[derive(Default, Clone, Copy, Debug)]
pub struct NegamaxQuiescenceMvvLvaV4;

impl Engine for NegamaxQuiescenceMvvLvaV4 {
    fn choose_move(&self, state: &GameState, opts: &AiOptions) -> Option<AiMoveResult> {
        run_qmvv(state, opts, &MaterialKingSafetyPstV3, "negamax-quiescence-mvv-lva-v4")
    }
    fn name(&self) -> &'static str {
        "negamax-quiescence-mvv-lva-v4"
    }
}

/// v5 (2026-05-10): same v3 evaluator and v4 quiescence/MVV-LVA, but the
/// search now uses **iterative deepening** with a **Zobrist
/// transposition table** — depth-by-depth search reuses TT-stored
/// scores and best-move hints from shallower iterations to drastically
/// reduce node count for the same effective depth. Default since
/// 2026-05-10. See `docs/ai/v5-id-tt.md`.
#[derive(Default, Clone, Copy, Debug)]
pub struct NegamaxIterativeTtV5;

impl Engine for NegamaxIterativeTtV5 {
    fn choose_move(&self, state: &GameState, opts: &AiOptions) -> Option<AiMoveResult> {
        analyze_v5(state, opts).map(|a| a.chosen)
    }
    fn name(&self) -> &'static str {
        "negamax-iterative-tt-v5"
    }
}

/// Shared body. `engine_label` is only used for the `AiMoveResult` —
/// the search itself is fully parameterised on the [`Evaluator`].
fn run<E: Evaluator>(
    state: &GameState,
    opts: &AiOptions,
    eval: &E,
    engine_label: &'static str,
) -> Option<AiMoveResult> {
    build_analysis(state, opts, eval, engine_label, Strategy::MaterialV1, /*qmvv=*/ false)
        .map(|a| a.chosen)
}

/// Same as [`run`] but uses [`score_root_moves_qmvv`] (MVV-LVA + quiescence)
/// for the search. Pulled out as a separate function rather than a flag
/// on `run` so the v1-v3 fast path stays untouched.
fn run_qmvv<E: Evaluator>(
    state: &GameState,
    opts: &AiOptions,
    eval: &E,
    engine_label: &'static str,
) -> Option<AiMoveResult> {
    build_analysis(
        state,
        opts,
        eval,
        engine_label,
        Strategy::QuiescenceMvvLvaV4,
        /*qmvv=*/ true,
    )
    .map(|a| a.chosen)
}

// ---------------------------------------------------------------------
// Public analyze_v* entry points — same work as the run/run_qmvv
// dispatchers, but return the full [`AiAnalysis`] instead of just the
// chosen move. `lib.rs::analyze` matches on `Strategy` and dispatches.
// ---------------------------------------------------------------------

pub(crate) fn analyze_v1(state: &GameState, opts: &AiOptions) -> Option<AiAnalysis> {
    build_analysis(state, opts, &MaterialV1, "negamax-v1", Strategy::MaterialV1, false)
}

pub(crate) fn analyze_v2(state: &GameState, opts: &AiOptions) -> Option<AiAnalysis> {
    build_analysis(state, opts, &MaterialPstV2, "negamax-v2", Strategy::MaterialPstV2, false)
}

pub(crate) fn analyze_v3(state: &GameState, opts: &AiOptions) -> Option<AiAnalysis> {
    build_analysis(
        state,
        opts,
        &MaterialKingSafetyPstV3,
        "negamax-v3",
        Strategy::MaterialKingSafetyPstV3,
        false,
    )
}

pub(crate) fn analyze_v4(state: &GameState, opts: &AiOptions) -> Option<AiAnalysis> {
    build_analysis(
        state,
        opts,
        &MaterialKingSafetyPstV3,
        "negamax-quiescence-mvv-lva-v4",
        Strategy::QuiescenceMvvLvaV4,
        true,
    )
}

/// v5 entry: iterative deepening + Zobrist transposition table. Has a
/// distinct shape from v1-v4 because [`score_root_moves_v5`] also
/// returns the actually-reached depth (which may be less than the
/// requested target when the node budget is exhausted mid-iteration).
pub(crate) fn analyze_v5(state: &GameState, opts: &AiOptions) -> Option<AiAnalysis> {
    if !matches!(state.status, chess_core::state::GameStatus::Ongoing) {
        return None;
    }

    let target_depth = opts.max_depth.unwrap_or_else(|| opts.difficulty.default_depth()).max(1);
    let seed = opts.seed.unwrap_or(0xC0FFEE_u64);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    let (mut scored, nodes, reached_depth) =
        score_root_moves_v5(state, target_depth, &MaterialKingSafetyPstV3);
    if scored.is_empty() {
        return None;
    }

    let randomness = opts.effective_randomness();
    let chosen = pick_with_randomness(&scored, randomness, &mut rng);
    let chosen_result =
        AiMoveResult { mv: chosen.mv.clone(), score: chosen.score, depth: reached_depth, nodes };
    let _: u8 = rng.gen();

    scored.sort_by_key(|sm| std::cmp::Reverse(sm.score));

    Some(AiAnalysis {
        chosen: chosen_result,
        scored,
        depth: reached_depth,
        nodes,
        strategy: Strategy::IterativeDeepeningTtV5,
        randomness,
    })
}

/// Unified search-and-pick path for both `run`/`run_qmvv` and the
/// `analyze_v*` introspection entry points.
///
/// `qmvv = true` selects the MVV-LVA + quiescence root-scoring path
/// (`score_root_moves_qmvv`); `false` selects the legacy capture-first
/// path (`score_root_moves`). Both produce a `Vec<ScoredMove>` plus a
/// node count; this function applies the randomness policy to pick
/// `chosen` and assembles an [`AiAnalysis`].
fn build_analysis<E: Evaluator>(
    state: &GameState,
    opts: &AiOptions,
    eval: &E,
    engine_label: &'static str,
    strategy: Strategy,
    qmvv: bool,
) -> Option<AiAnalysis> {
    if !matches!(state.status, chess_core::state::GameStatus::Ongoing) {
        return None;
    }

    let depth = opts.max_depth.unwrap_or_else(|| opts.difficulty.default_depth()).max(1);
    let seed = opts.seed.unwrap_or(0xC0FFEE_u64);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    let (mut scored, nodes) = if qmvv {
        score_root_moves_qmvv(state, depth, eval)
    } else {
        score_root_moves(state, depth, eval)
    };
    if scored.is_empty() {
        return None;
    }

    let randomness = opts.effective_randomness();
    let chosen = pick_with_randomness(&scored, randomness, &mut rng);
    let chosen_result = AiMoveResult { mv: chosen.mv.clone(), score: chosen.score, depth, nodes };

    // Burn one byte of RNG so identical positions with different seeds
    // vary even when the chosen move ties cleanly. (Hard stays stable
    // — deterministic by construction when the user picks STRICT.)
    let _: u8 = rng.gen();

    // Sort scored list best-first for the debug UI. Ties broken by
    // original index (stable sort on `score`) — preserves "first
    // occurrence wins" semantics from the picker.
    scored.sort_by_key(|sm| std::cmp::Reverse(sm.score));

    let _ = engine_label; // hook for future logging.
    Some(AiAnalysis { chosen: chosen_result, scored, depth, nodes, strategy, randomness })
}

/// Apply the [`Randomness`] policy to a list of scored root moves and
/// return the survivor that the seeded RNG picked. Pulled out as a
/// standalone helper so a unit test can pin the policy independently
/// of the search.
///
/// Algorithm:
/// 1. Find best score.
/// 2. Filter to moves within `cp_window` cp of best.
/// 3. Sort filtered survivors descending by score.
/// 4. Take at most `top_k.max(1)` of them.
/// 5. RNG picks one uniformly.
fn pick_with_randomness<'a>(
    scored: &'a [ScoredMove],
    r: Randomness,
    rng: &mut ChaCha8Rng,
) -> &'a ScoredMove {
    debug_assert!(!scored.is_empty(), "caller must check for empty");
    let best = scored.iter().map(|sm| sm.score).max().expect("non-empty");
    let cp_window = r.cp_window.max(0);
    let top_k = r.top_k.max(1);

    let mut eligible: Vec<&ScoredMove> =
        scored.iter().filter(|sm| best - sm.score <= cp_window).collect();
    // Stable sort by score desc, breaking ties by original index so the
    // "first occurrence" determinism of strict mode is preserved.
    // (Use `sort_by_key(Reverse(...))` rather than the equivalent
    // closure form to satisfy clippy 1.95's `unnecessary_sort_by`.)
    eligible.sort_by_key(|sm| std::cmp::Reverse(sm.score));
    let take = eligible.len().min(top_k);
    let pool = &eligible[..take];
    pool.choose(rng).copied().expect("pool non-empty when scored non-empty")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_core::coord::Square;
    use chess_core::moves::Move;

    fn fake_step(score: i32) -> ScoredMove {
        // Construct a fake move; coords don't matter for the picker test.
        ScoredMove { mv: Move::Step { from: Square(0), to: Square(1) }, score, pv: Vec::new() }
    }

    fn pool() -> Vec<ScoredMove> {
        vec![fake_step(100), fake_step(95), fake_step(80), fake_step(50), fake_step(0)]
    }

    #[test]
    fn strict_picks_best_only() {
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        let p = pool();
        let pick = pick_with_randomness(&p, Randomness::STRICT, &mut rng);
        assert_eq!(pick.score, 100);
    }

    #[test]
    fn subtle_picks_within_window() {
        // Window 20 → eligible: 100, 95, 80; top-3 = all three. RNG picks one.
        let p = pool();
        for seed in 0..20u64 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let pick = pick_with_randomness(&p, Randomness::SUBTLE, &mut rng);
            assert!(
                [100, 95, 80].contains(&pick.score),
                "subtle picked outside top-3-within-20: {}",
                pick.score
            );
        }
    }

    #[test]
    fn varied_picks_within_60cp() {
        let p = pool();
        for seed in 0..20u64 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let pick = pick_with_randomness(&p, Randomness::VARIED, &mut rng);
            assert!(pick.score >= 100 - 60, "varied picked below window: {}", pick.score);
        }
    }

    #[test]
    fn top_k_zero_treated_as_one() {
        let mut rng = ChaCha8Rng::seed_from_u64(0);
        let p = pool();
        let pick = pick_with_randomness(&p, Randomness { top_k: 0, cp_window: 200 }, &mut rng);
        assert_eq!(pick.score, 100, "top_k=0 should clamp to 1 (best only)");
    }

    #[test]
    fn determinism_same_seed_same_pick() {
        let p = pool();
        let mut rng_a = ChaCha8Rng::seed_from_u64(42);
        let mut rng_b = ChaCha8Rng::seed_from_u64(42);
        let a = pick_with_randomness(&p, Randomness::CHAOTIC, &mut rng_a);
        let b = pick_with_randomness(&p, Randomness::CHAOTIC, &mut rng_b);
        assert_eq!(a.score, b.score);
    }
}
