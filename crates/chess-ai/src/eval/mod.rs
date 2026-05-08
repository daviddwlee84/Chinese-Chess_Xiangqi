//! Static evaluators.
//!
//! An [`Evaluator`] is a pure side-relative scorer: given a [`GameState`]
//! whose `side_to_move` is `me`, return positive for "me is winning",
//! negative for "me is losing". The search wraps this with negamax so the
//! evaluator never has to think about depth, alpha/beta, or the move that
//! got us here.
//!
//! Versions are additive — when v3 lands it adds a new module and a new
//! `eval::*` impl; older versions stay reachable via [`crate::Strategy`]
//! so `?engine=v1` still picks the original eval. See `docs/ai/README.md`.

use chess_core::state::GameState;

pub mod material_pst_v2;
pub mod material_v1;

/// A pure side-relative position scorer.
///
/// Implementations must be deterministic and side-relative: positive
/// favours `state.side_to_move`. Search code negates across plies, so
/// "absolute" evaluators (positive = Red) will produce broken alpha-beta.
pub trait Evaluator {
    /// Static score for the position. Centipawn-ish units: a single
    /// soldier ≈ 100, chariot ≈ 900, etc. Mate scores are NOT this
    /// evaluator's job — search returns `-MATE + depth` when there are
    /// no legal moves.
    fn evaluate(&self, state: &GameState) -> i32;

    /// Short tag for logs / sidebar / docs. Stable across releases.
    fn name(&self) -> &'static str;
}
