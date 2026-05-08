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

use crate::eval::material_pst_v2::MaterialPstV2;
use crate::eval::material_v1::MaterialV1;
use crate::eval::Evaluator;
use crate::search::{score_root_moves, ScoredMove};
use crate::{AiMoveResult, AiOptions, Difficulty};

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

/// Shared body. `engine_label` is only used for the `AiMoveResult` —
/// the search itself is fully parameterised on the [`Evaluator`].
fn run<E: Evaluator>(
    state: &GameState,
    opts: &AiOptions,
    eval: &E,
    engine_label: &'static str,
) -> Option<AiMoveResult> {
    if !matches!(state.status, chess_core::state::GameStatus::Ongoing) {
        return None;
    }

    let depth = opts.max_depth.unwrap_or_else(|| opts.difficulty.default_depth()).max(1);
    let seed = opts.seed.unwrap_or(0xC0FFEE_u64);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);

    let (scored, nodes) = score_root_moves(state, depth, eval);
    if scored.is_empty() {
        return None;
    }

    let best_score = scored.iter().map(|sm| sm.score).max().unwrap();
    let chosen = match opts.difficulty {
        Difficulty::Easy => {
            // Pick uniformly from top-3 by score.
            let mut sorted: Vec<&ScoredMove> = scored.iter().collect();
            sorted.sort_by_key(|sm| std::cmp::Reverse(sm.score));
            let take = sorted.len().min(3);
            let pick = sorted[..take].choose(&mut rng).copied().unwrap();
            (pick.mv.clone(), pick.score)
        }
        Difficulty::Normal => {
            // Best-or-near-best: any move within 10cp of best, picked at random.
            let near: Vec<&ScoredMove> =
                scored.iter().filter(|sm| best_score - sm.score <= 10).collect();
            let pick = near.choose(&mut rng).copied().unwrap();
            (pick.mv.clone(), pick.score)
        }
        Difficulty::Hard => {
            // Strict best (first occurrence on ties — deterministic).
            let pick = scored.iter().find(|sm| sm.score == best_score).unwrap();
            (pick.mv.clone(), pick.score)
        }
    };

    // Tiny noise on Easy/Normal via the same rng so identical positions
    // with different seeds vary even when scores tie cleanly. (Hard stays
    // stable — deterministic by construction.)
    let _: u8 = rng.gen();

    let _ = engine_label; // hook for future logging — keeps the name() in scope.
    Some(AiMoveResult { mv: chosen.0, score: chosen.1, depth, nodes })
}
