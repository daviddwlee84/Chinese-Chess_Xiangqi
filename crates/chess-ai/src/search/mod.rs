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

use chess_core::moves::Move;
use chess_core::state::GameState;

use crate::eval::Evaluator;

/// Mate score — large enough that no material configuration can match it.
pub const MATE: i32 = 1_000_000;

/// Cap nodes-per-search to keep web UI snappy. Hit at depth 4 in busy
/// midgames; the search returns the best-found-so-far when exceeded.
/// Bumped from 250k (v1 MVP) — v2's PST adds no extra node cost; later
/// versions with iterative deepening will set this dynamically.
pub const NODE_BUDGET: u32 = 250_000;

/// Outcome of a root search: the chosen move plus diagnostic stats. The
/// caller (engine impl) is free to apply Easy/Normal randomisation on
/// top of the returned scores; this layer is deterministic.
#[derive(Clone, Debug)]
pub struct ScoredMove {
    pub mv: Move,
    pub score: i32,
}

/// Score every root move once at `depth` plies, in capture-first order.
/// Returns one [`ScoredMove`] per move tried; aborts early when the
/// node budget is exhausted (the remaining moves stay unscored, which
/// is fine — caller picks among what we managed to score).
pub fn score_root_moves<E: Evaluator>(
    state: &GameState,
    depth: u8,
    eval: &E,
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
    let mut alpha = -MATE - 1;
    let beta = MATE + 1;

    for mv in ordered {
        if work.make_move(&mv).is_err() {
            continue;
        }
        let score = -negamax(&mut work, depth.saturating_sub(1), -beta, -alpha, &mut nodes, eval);
        let _ = work.unmake_move();
        scored.push(ScoredMove { mv, score });
        if score > alpha {
            alpha = score;
        }
        if nodes >= NODE_BUDGET {
            break;
        }
    }
    (scored, nodes)
}

/// Negamax with α-β pruning, capture-first move ordering, and node
/// budgeting. Side-relative scores: positive = good for the side to
/// move at this node.
pub fn negamax<E: Evaluator>(
    state: &mut GameState,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    nodes: &mut u32,
    eval: &E,
) -> i32 {
    *nodes = nodes.saturating_add(1);
    let moves = state.legal_moves();
    if moves.is_empty() {
        // No legal moves under Asian rules → loss for the side to move
        // (checkmate or stalemate). Depth-relative mate makes the
        // search prefer faster mates and slower losses.
        return -MATE + depth as i32;
    }
    if depth == 0 || *nodes >= NODE_BUDGET {
        return eval.evaluate(state);
    }

    let mut ordered: Vec<Move> = moves.into_iter().collect();
    ordered.sort_by_key(|m| if is_capture(m) { 0 } else { 1 });

    let mut best = -MATE - 1;
    for mv in ordered {
        if state.make_move(&mv).is_err() {
            continue;
        }
        let score = -negamax(state, depth - 1, -beta, -alpha, nodes, eval);
        let _ = state.unmake_move();
        if score > best {
            best = score;
        }
        if score > alpha {
            alpha = score;
        }
        if alpha >= beta {
            break;
        }
    }
    best
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
pub fn score_root_moves_qmvv<E: Evaluator>(
    state: &GameState,
    depth: u8,
    eval: &E,
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
    let mut alpha = -MATE - 1;
    let beta = MATE + 1;

    for (_score, mv) in ordered {
        if work.make_move(&mv).is_err() {
            continue;
        }
        let v = -negamax_qmvv(&mut work, depth.saturating_sub(1), -beta, -alpha, &mut nodes, eval);
        let _ = work.unmake_move();
        scored.push(ScoredMove { mv, score: v });
        if v > alpha {
            alpha = v;
        }
        if nodes >= NODE_BUDGET {
            break;
        }
    }
    (scored, nodes)
}

/// Negamax + α-β with MVV-LVA move ordering and quiescence search at
/// the horizon. Drop-in replacement for [`negamax`] in v4.
pub fn negamax_qmvv<E: Evaluator>(
    state: &mut GameState,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    nodes: &mut u32,
    eval: &E,
) -> i32 {
    *nodes = nodes.saturating_add(1);
    let moves = state.legal_moves();
    if moves.is_empty() {
        return -MATE + depth as i32;
    }
    if *nodes >= NODE_BUDGET {
        return eval.evaluate(state);
    }
    if depth == 0 {
        // Hand off to quiescence instead of returning the static eval.
        return quiescence::quiescence(state, alpha, beta, nodes, eval, quiescence::Q_MAX_PLIES);
    }

    // MVV-LVA ordering instead of flat capture-first.
    let mut ordered: Vec<(i32, Move)> =
        moves.into_iter().map(|m| (ordering::mvv_lva_score(state, &m), m)).collect();
    ordered.sort_by_key(|(s, _)| std::cmp::Reverse(*s));

    let mut best = -MATE - 1;
    for (_score, mv) in ordered {
        if state.make_move(&mv).is_err() {
            continue;
        }
        let v = -negamax_qmvv(state, depth - 1, -beta, -alpha, nodes, eval);
        let _ = state.unmake_move();
        if v > best {
            best = v;
        }
        if v > alpha {
            alpha = v;
        }
        if alpha >= beta {
            break;
        }
    }
    best
}
