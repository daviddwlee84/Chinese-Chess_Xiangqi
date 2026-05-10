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
        let (child_score, child_pv) =
            negamax(&mut work, depth.saturating_sub(1), -(MATE + 1), MATE + 1, &mut nodes, eval);
        let score = -child_score;
        let _ = work.unmake_move();
        scored.push(ScoredMove { mv, score, pv: child_pv });
        if nodes >= NODE_BUDGET {
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
pub fn negamax<E: Evaluator>(
    state: &mut GameState,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    nodes: &mut u32,
    eval: &E,
) -> (i32, Vec<Move>) {
    *nodes = nodes.saturating_add(1);
    let moves = state.legal_moves();
    if moves.is_empty() {
        // No legal moves under Asian rules → loss for the side to move
        // (checkmate or stalemate). Depth-relative mate makes the
        // search prefer faster mates and slower losses.
        return (-MATE + depth as i32, Vec::new());
    }
    if depth == 0 || *nodes >= NODE_BUDGET {
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
        let (child_score, child_pv) = negamax(state, depth - 1, -beta, -alpha, nodes, eval);
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
        );
        let v = -child_score;
        let _ = work.unmake_move();
        scored.push(ScoredMove { mv, score: v, pv: child_pv });
        if nodes >= NODE_BUDGET {
            break;
        }
    }
    (scored, nodes)
}

/// Negamax + α-β with MVV-LVA move ordering and quiescence search at
/// the horizon. Drop-in replacement for [`negamax`] in v4. Returns
/// `(score, pv)` like [`negamax`].
pub fn negamax_qmvv<E: Evaluator>(
    state: &mut GameState,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    nodes: &mut u32,
    eval: &E,
) -> (i32, Vec<Move>) {
    *nodes = nodes.saturating_add(1);
    let moves = state.legal_moves();
    if moves.is_empty() {
        return (-MATE + depth as i32, Vec::new());
    }
    if *nodes >= NODE_BUDGET {
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
        let (child_score, child_pv) = negamax_qmvv(state, depth - 1, -beta, -alpha, nodes, eval);
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
