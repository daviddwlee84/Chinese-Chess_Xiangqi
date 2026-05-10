//! Search infrastructure shared across engine versions.
//!
//! Today: capture-first ordered negamax with α-β pruning + a node
//! budget that bails to "best-so-far" when search would block the UI.
//! Future versions (v3 = iterative deepening + transposition table,
//! v4 = quiescence + MVV-LVA) live in sibling modules and reuse this
//! file's primitives.
//!
//! Generic over [`Evaluator`](crate::eval::Evaluator) so v1 and v2 (and
//! later v3+) share the same search code with zero duplication. The
//! evaluator is borrowed (`&E`) — search never mutates it.

pub mod ordering;
pub mod quiescence;
pub mod tt;
pub mod v5;

use chess_core::moves::Move;
use chess_core::state::GameState;

use crate::eval::Evaluator;

/// Mate score — large enough that no material configuration can match it.
pub const MATE: i32 = 1_000_000;

/// Default per-search node budget for v1-v4 and the **baseline** for v5
/// at low requested depths. Picked to keep web UI responsive: hit at
/// depth 4 in busy midgames; the search returns the best-found-so-far
/// when exceeded.
///
/// **For v5** (iterative deepening), the actual budget is computed by
/// [`node_budget_for_depth`] from the user's requested target depth —
/// a request of `Some(10)` gets a much larger budget than the baseline
/// so iterative deepening can complete more iterations. v1-v4 callers
/// keep using this flat constant since they don't iterate.
pub const NODE_BUDGET: u32 = 250_000;

/// Hard ceiling on the auto-computed v5 budget. Picked so the worst
/// case (`target_depth >= 10`) stays under ~10 s on a typical 2024
/// laptop running WASM. Native is ~3-5× faster, so deep searches there
/// complete well within this ceiling.
pub const MAX_AUTO_NODE_BUDGET: u32 = 16_000_000;

/// Baseline depth at which [`node_budget_for_depth`] returns
/// [`NODE_BUDGET`] verbatim. Above this, the budget doubles per extra
/// requested ply (matching the empirically-observed effective branching
/// factor of v5 with TT-driven move ordering, ~2 in the opening / ~3
/// in tactical positions). Equal to `Difficulty::Hard.default_depth()`
/// so the **default Hard behaviour does not change** with this scaling
/// (preserves the perf characteristics documented in `docs/ai/perf.md`).
const NODE_BUDGET_BASELINE_DEPTH: u8 = 4;

/// Compute the effective per-search node budget for a given requested
/// target depth.
///
/// **Policy** (v5 only — v1-v4 keep using the flat [`NODE_BUDGET`]):
///
/// | `target_depth` | budget       |
/// |----------------|--------------|
/// | 1..=4          | `NODE_BUDGET` (250k) — unchanged baseline |
/// | 5              | 500k          |
/// | 6              | 1.0M          |
/// | 7              | 2.0M          |
/// | 8              | 4.0M          |
/// | 9              | 8.0M          |
/// | 10+            | 16.0M (cap)   |
///
/// Why the doubling: empirically v5's iterative deepening + TT achieves
/// effective branching factor ~2-3 in midgame Xiangqi positions
/// (better than raw v4 negamax thanks to TT-driven move ordering
/// hits). Doubling per extra ply means a depth-N+1 iteration uses
/// roughly 2× the depth-N nodes, so the new budget admits exactly one
/// more iteration before tripping. Exact alignment isn't possible
/// (positions vary wildly) but the policy keeps `reached_depth`
/// growing monotonically with `target_depth` instead of plateauing at
/// 4 forever — see `pitfalls/ai-search-depth-setting-shows-depth-4.md`.
///
/// The cap at [`MAX_AUTO_NODE_BUDGET`] keeps the worst-case wall-clock
/// bounded; users who want to override (either way) will get an
/// `AiOptions.node_budget: Option<u32>` knob in a follow-up.
pub fn node_budget_for_depth(target_depth: u8) -> u32 {
    if target_depth <= NODE_BUDGET_BASELINE_DEPTH {
        return NODE_BUDGET;
    }
    let extra = target_depth - NODE_BUDGET_BASELINE_DEPTH;
    // `1u32 << 32` overflows; the cap at MAX_AUTO_NODE_BUDGET is
    // reached at extra=6 (250k * 64 = 16M), so any extra ≥ 6 already
    // saturates. Use `checked_shl` and fall through to the cap on
    // overflow rather than relying on a hypothetical `saturating_shl`
    // (no such method on u32 as of stable 2026-05).
    let multiplier = 1u32.checked_shl(extra as u32).unwrap_or(u32::MAX);
    let scaled = NODE_BUDGET.saturating_mul(multiplier);
    scaled.min(MAX_AUTO_NODE_BUDGET)
}

/// Outcome of a root search: the chosen move plus diagnostic stats. The
/// caller (engine impl) is free to apply Easy/Normal randomisation on
/// top of the returned scores; this layer is deterministic.
#[derive(Clone, Debug)]
pub struct ScoredMove {
    pub mv: Move,
    pub score: i32,
    /// Principal variation continuation — the sequence of moves the
    /// search expects to follow `mv` if both sides play optimally.
    /// Element 0 is the opponent's best reply, element 1 is the AI's
    /// best follow-up, etc. Empty when the search bailed at the leaf
    /// (mate, stalemate, node-budget hit, or `depth == 0`).
    /// Length ≤ `depth - 1`. Same `Vec<Move>` semantics as
    /// `chess_core::state::history`.
    pub pv: Vec<Move>,
}

/// Score every root move once at `depth` plies, in capture-first order.
/// Returns one [`ScoredMove`] per move tried; aborts early when the
/// node budget is exhausted (the remaining moves stay unscored, which
/// is fine — caller picks among what we managed to score).
///
/// **Important**: each root move is searched with a **full window**
/// (`-MATE-1 .. MATE+1`) — we do NOT narrow alpha based on previously-
/// scored root moves. This costs more nodes but ensures the returned
/// `score` for each move is the move's *true* value, not a bound
/// clamped to whatever the running best was.
///
/// Why: the [`crate::Randomness`] layer downstream picks a move from
/// "top-K within ±cp_window of best". If alpha-beta at the root
/// reports a suicide as "score = -132" (the running alpha, not the
/// true -50_000 cp value), the randomness layer can't distinguish it
/// from a genuinely-good move and may pick the suicide. Class-of-bug
/// fix; see `pitfalls/alpha-beta-root-score-pollution.md` for the
/// full write-up.
pub fn score_root_moves<E: Evaluator>(
    state: &GameState,
    depth: u8,
    eval: &E,
) -> (Vec<ScoredMove>, u32) {
    score_root_moves_with_budget(state, depth, eval, NODE_BUDGET)
}

/// Variant of [`score_root_moves`] that takes an explicit
/// `node_budget`. Engine entry points wanting to honor
/// `AiOptions::node_budget` call this directly.
pub fn score_root_moves_with_budget<E: Evaluator>(
    state: &GameState,
    depth: u8,
    eval: &E,
    node_budget: u32,
) -> (Vec<ScoredMove>, u32) {
    let moves = state.legal_moves();
    if moves.is_empty() {
        return (Vec::new(), 0);
    }
    let mut ordered: Vec<Move> = moves.into_iter().collect();
    ordered.sort_by_key(|m| if is_capture(m) { 0 } else { 1 });

    let mut work = state.clone();
    let mut nodes: u32 = 0;
    let mut scored: Vec<ScoredMove> = Vec::with_capacity(ordered.len());

    for mv in ordered {
        if work.make_move(&mv).is_err() {
            continue;
        }
        // Full-window search — see fn doc.
        let (child_score, child_pv) = negamax(
            &mut work,
            depth.saturating_sub(1),
            -(MATE + 1),
            MATE + 1,
            &mut nodes,
            eval,
            node_budget,
        );
        let score = -child_score;
        let _ = work.unmake_move();
        scored.push(ScoredMove { mv, score, pv: child_pv });
        if nodes >= node_budget {
            break;
        }
    }
    (scored, nodes)
}

/// Negamax with α-β pruning, capture-first move ordering, and node
/// budgeting. Side-relative scores: positive = good for the side to
/// move at this node.
///
/// Returns `(score, pv)` — the principal variation `pv` is the
/// sequence of best-line moves starting from THIS node (i.e. element
/// 0 is the move the side-to-move makes, element 1 is the opponent's
/// reply, etc.). Empty when the search reached a leaf (no legal
/// moves, depth == 0, or budget bail).
///
/// `node_budget` is the per-search hard cap; passed in so callers
/// can honor `AiOptions::node_budget` overrides without changing the
/// public surface of this function further.
pub fn negamax<E: Evaluator>(
    state: &mut GameState,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    nodes: &mut u32,
    eval: &E,
    node_budget: u32,
) -> (i32, Vec<Move>) {
    *nodes = nodes.saturating_add(1);
    let moves = state.legal_moves();
    if moves.is_empty() {
        // No legal moves under Asian rules → loss for the side to move
        // (checkmate or stalemate). Depth-relative mate makes the
        // search prefer faster mates and slower losses.
        return (-MATE + depth as i32, Vec::new());
    }
    if depth == 0 || *nodes >= node_budget {
        return (eval.evaluate(state), Vec::new());
    }

    let mut ordered: Vec<Move> = moves.into_iter().collect();
    ordered.sort_by_key(|m| if is_capture(m) { 0 } else { 1 });

    let mut best = -MATE - 1;
    let mut best_pv: Vec<Move> = Vec::new();
    for mv in ordered {
        if state.make_move(&mv).is_err() {
            continue;
        }
        let (child_score, child_pv) =
            negamax(state, depth - 1, -beta, -alpha, nodes, eval, node_budget);
        let score = -child_score;
        let _ = state.unmake_move();
        if score > best {
            best = score;
            best_pv.clear();
            best_pv.push(mv.clone());
            best_pv.extend(child_pv);
        }
        if score > alpha {
            alpha = score;
        }
        if alpha >= beta {
            break;
        }
    }
    (best, best_pv)
}

/// True iff the move removes an opponent piece. v3+ may upgrade this to
/// MVV-LVA (most-valuable-victim, least-valuable-attacker) ordering.
#[inline]
pub fn is_capture(m: &Move) -> bool {
    matches!(
        m,
        Move::Capture { .. }
            | Move::CannonJump { .. }
            | Move::ChainCapture { .. }
            | Move::DarkCapture { .. }
    )
}

// ---------------------------------------------------------------------
// v4: quiescence + MVV-LVA variant of the same negamax loop. Lives next
// to the v1-v3 search so they can share the [`Evaluator`] generic and
// the [`NODE_BUDGET`] / [`MATE`] constants. v5+ versions will likely
// add yet another `score_root_moves_*` variant rather than thread more
// flags into this one.
// ---------------------------------------------------------------------

/// MVV-LVA ordered, quiescence-aware root scoring. Same shape as
/// [`score_root_moves`] but the recursion uses [`negamax_qmvv`] +
/// [`quiescence::quiescence`] at the horizon.
///
/// Same **full-window-at-root** rule as [`score_root_moves`] — see that
/// function's doc for why.
pub fn score_root_moves_qmvv<E: Evaluator>(
    state: &GameState,
    depth: u8,
    eval: &E,
) -> (Vec<ScoredMove>, u32) {
    score_root_moves_qmvv_with_budget(state, depth, eval, NODE_BUDGET)
}

/// Variant of [`score_root_moves_qmvv`] that takes an explicit
/// `node_budget`. Engine entry points wanting to honor
/// `AiOptions::node_budget` call this directly.
pub fn score_root_moves_qmvv_with_budget<E: Evaluator>(
    state: &GameState,
    depth: u8,
    eval: &E,
    node_budget: u32,
) -> (Vec<ScoredMove>, u32) {
    let moves = state.legal_moves();
    if moves.is_empty() {
        return (Vec::new(), 0);
    }
    let mut ordered: Vec<(i32, Move)> =
        moves.into_iter().map(|m| (ordering::mvv_lva_score(state, &m), m)).collect();
    ordered.sort_by_key(|(s, _)| std::cmp::Reverse(*s));

    let mut work = state.clone();
    let mut nodes: u32 = 0;
    let mut scored: Vec<ScoredMove> = Vec::with_capacity(ordered.len());

    for (_score, mv) in ordered {
        if work.make_move(&mv).is_err() {
            continue;
        }
        // Full-window search — see `score_root_moves` doc.
        let (child_score, child_pv) = negamax_qmvv(
            &mut work,
            depth.saturating_sub(1),
            -(MATE + 1),
            MATE + 1,
            &mut nodes,
            eval,
            node_budget,
        );
        let v = -child_score;
        let _ = work.unmake_move();
        scored.push(ScoredMove { mv, score: v, pv: child_pv });
        if nodes >= node_budget {
            break;
        }
    }
    (scored, nodes)
}

/// Negamax + α-β with MVV-LVA move ordering and quiescence search at
/// the horizon. Drop-in replacement for [`negamax`] in v4. Returns
/// `(score, pv)` like [`negamax`].
///
/// `node_budget` is the per-search hard cap; passed in for the same
/// reason as [`negamax`].
pub fn negamax_qmvv<E: Evaluator>(
    state: &mut GameState,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    nodes: &mut u32,
    eval: &E,
    node_budget: u32,
) -> (i32, Vec<Move>) {
    *nodes = nodes.saturating_add(1);
    let moves = state.legal_moves();
    if moves.is_empty() {
        return (-MATE + depth as i32, Vec::new());
    }
    if *nodes >= node_budget {
        return (eval.evaluate(state), Vec::new());
    }
    if depth == 0 {
        // Hand off to quiescence instead of returning the static eval.
        // Quiescence doesn't track PV — its captures are exploratory,
        // not "the line we'll actually play". Returning empty is
        // correct (PV ends here from the main-search POV).
        return (
            quiescence::quiescence(state, alpha, beta, nodes, eval, quiescence::Q_MAX_PLIES),
            Vec::new(),
        );
    }

    // MVV-LVA ordering instead of flat capture-first.
    let mut ordered: Vec<(i32, Move)> =
        moves.into_iter().map(|m| (ordering::mvv_lva_score(state, &m), m)).collect();
    ordered.sort_by_key(|(s, _)| std::cmp::Reverse(*s));

    let mut best = -MATE - 1;
    let mut best_pv: Vec<Move> = Vec::new();
    for (_score, mv) in ordered {
        if state.make_move(&mv).is_err() {
            continue;
        }
        let (child_score, child_pv) =
            negamax_qmvv(state, depth - 1, -beta, -alpha, nodes, eval, node_budget);
        let v = -child_score;
        let _ = state.unmake_move();
        if v > best {
            best = v;
            best_pv.clear();
            best_pv.push(mv.clone());
            best_pv.extend(child_pv);
        }
        if v > alpha {
            alpha = v;
        }
        if alpha >= beta {
            break;
        }
    }
    (best, best_pv)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Defensive: the baseline value used by v1-v4 must not change
    /// without thinking — every test that pins v3/v4 perf depends on
    /// 250k. If we ever bump it, every doc/perf reference needs the
    /// same audit.
    #[test]
    fn node_budget_baseline_is_250k() {
        assert_eq!(NODE_BUDGET, 250_000);
    }

    /// Below the baseline depth (4 = `Difficulty::Hard.default_depth()`)
    /// the auto-budget is `NODE_BUDGET` verbatim. This is the
    /// "back-compat" guarantee — default Hard play has the same node
    /// budget as it did before the depth-scaling policy landed.
    #[test]
    fn node_budget_for_depth_at_or_below_baseline_returns_baseline() {
        for d in 1..=NODE_BUDGET_BASELINE_DEPTH {
            assert_eq!(
                node_budget_for_depth(d),
                NODE_BUDGET,
                "depth {d}: should equal baseline {NODE_BUDGET}",
            );
        }
    }

    /// Above the baseline the budget doubles per extra requested ply.
    /// This mirrors v5's empirical effective branching factor with TT.
    #[test]
    fn node_budget_for_depth_doubles_per_extra_ply_above_baseline() {
        // depth 5 → 2× baseline = 500k
        assert_eq!(node_budget_for_depth(5), 500_000);
        // depth 6 → 4× = 1M
        assert_eq!(node_budget_for_depth(6), 1_000_000);
        // depth 7 → 8× = 2M
        assert_eq!(node_budget_for_depth(7), 2_000_000);
        // depth 8 → 16× = 4M
        assert_eq!(node_budget_for_depth(8), 4_000_000);
        // depth 9 → 32× = 8M
        assert_eq!(node_budget_for_depth(9), 8_000_000);
    }

    /// At depth 10+ the budget hits `MAX_AUTO_NODE_BUDGET` and stays
    /// there; preserves a wall-clock ceiling so a typo of `depth=99`
    /// can't lock the browser tab.
    #[test]
    fn node_budget_for_depth_caps_at_max_auto_budget() {
        assert_eq!(node_budget_for_depth(10), MAX_AUTO_NODE_BUDGET);
        assert_eq!(node_budget_for_depth(15), MAX_AUTO_NODE_BUDGET);
        assert_eq!(node_budget_for_depth(255), MAX_AUTO_NODE_BUDGET);
    }

    /// `node_budget_for_depth` must be monotonically non-decreasing.
    /// A higher requested depth never shrinks the budget — required
    /// invariant for the "depth=N reaches deeper than depth=N-1"
    /// regression in `pitfalls/ai-search-depth-setting-shows-depth-4.md`.
    #[test]
    fn node_budget_for_depth_is_monotonic() {
        let mut prev = node_budget_for_depth(1);
        for d in 2u8..=20 {
            let cur = node_budget_for_depth(d);
            assert!(
                cur >= prev,
                "non-monotonic: depth {} → {} < depth {} → {}",
                d,
                cur,
                d - 1,
                prev,
            );
            prev = cur;
        }
    }
}
