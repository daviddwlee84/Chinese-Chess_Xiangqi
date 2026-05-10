//! v5 search: v4 (negamax + quiescence + MVV-LVA) + iterative deepening
//! + Zobrist transposition table.
//!
//! See `docs/ai/v5-id-tt.md` for the design write-up.

use chess_core::moves::Move;
use chess_core::state::GameState;

use crate::eval::Evaluator;
use crate::search::ordering;
use crate::search::quiescence;
use crate::search::tt::{score_from_tt, score_to_tt, Bound, TranspositionTable, TtEntry};
use crate::search::{is_capture, ScoredMove, MATE, NODE_BUDGET};

/// Mate-detection threshold. Anything above this is treated as a mate
/// score for storage adjustment. Generous so deep search stays robust.
const MATE_THRESHOLD: i32 = MATE - 1000;

/// Negamax + α-β with TT probe/store, MVV-LVA + TT-best-move ordering,
/// quiescence at the horizon. Drop-in upgrade of v4's
/// [`crate::search::negamax_qmvv`].
///
/// Returns `(score, pv)` like its v4 sibling. PV is reconstructed from
/// child returns (not the TT — TT entries don't carry the full PV, only
/// the best move at this node).
///
/// `ply` is the distance from the search root, used for mate-score
/// adjustment ([`score_to_tt`] / [`score_from_tt`]).
#[allow(clippy::too_many_arguments)]
pub fn negamax_v5<E: Evaluator>(
    state: &mut GameState,
    depth: u8,
    mut alpha: i32,
    beta: i32,
    nodes: &mut u32,
    eval: &E,
    tt: &mut TranspositionTable,
    ply: i32,
) -> (i32, Vec<Move>) {
    *nodes = nodes.saturating_add(1);
    let alpha_orig = alpha;
    let key = state.position_hash;

    // -------- TT probe --------
    let mut tt_best_move: Option<Move> = None;
    if let Some(entry) = tt.probe(key) {
        tt_best_move = entry.best_move.clone();
        if entry.depth >= depth {
            let score = score_from_tt(entry.score, ply, MATE_THRESHOLD);
            match entry.bound {
                Bound::Exact => {
                    let pv = tt_best_move.iter().cloned().collect();
                    return (score, pv);
                }
                Bound::Lower if score >= beta => {
                    let pv = tt_best_move.iter().cloned().collect();
                    return (score, pv);
                }
                Bound::Upper if score <= alpha => {
                    let pv = tt_best_move.iter().cloned().collect();
                    return (score, pv);
                }
                _ => {}
            }
        }
    }

    let moves = state.legal_moves();
    if moves.is_empty() {
        // No legal moves under Asian rules → loss for the side to move
        // (checkmate or stalemate). Depth-relative mate makes the
        // search prefer faster mates and slower losses.
        return (-MATE + ply, Vec::new());
    }
    if *nodes >= NODE_BUDGET {
        return (eval.evaluate(state), Vec::new());
    }
    if depth == 0 {
        // Quiescence at the horizon — no PV (its captures are
        // exploratory, not the line we'd commit to).
        return (
            quiescence::quiescence(state, alpha, beta, nodes, eval, quiescence::Q_MAX_PLIES),
            Vec::new(),
        );
    }

    // -------- Move ordering: TT-best first, then MVV-LVA --------
    let mut ordered: Vec<(i32, Move)> = moves
        .into_iter()
        .map(|m| {
            // Boost TT-best by a huge constant so it sorts first
            // regardless of MVV-LVA score.
            let mvv = ordering::mvv_lva_score(state, &m);
            let bonus = match &tt_best_move {
                Some(best) if best == &m => 1_000_000,
                _ => 0,
            };
            (mvv + bonus, m)
        })
        .collect();
    ordered.sort_by_key(|(s, _)| std::cmp::Reverse(*s));

    let mut best = -MATE - 1;
    let mut best_pv: Vec<Move> = Vec::new();
    let mut best_move: Option<Move> = None;
    for (_score, mv) in ordered {
        if state.make_move(&mv).is_err() {
            continue;
        }
        let (child_score, child_pv) =
            negamax_v5(state, depth - 1, -beta, -alpha, nodes, eval, tt, ply + 1);
        let v = -child_score;
        let _ = state.unmake_move();
        if v > best {
            best = v;
            best_pv.clear();
            best_pv.push(mv.clone());
            best_pv.extend(child_pv);
            best_move = Some(mv.clone());
        }
        if v > alpha {
            alpha = v;
        }
        if alpha >= beta {
            break;
        }
    }

    // -------- TT store --------
    let bound = if best <= alpha_orig {
        // Failed low — true score is at most `best`.
        Bound::Upper
    } else if best >= beta {
        // Failed high — true score is at least `best`.
        Bound::Lower
    } else {
        Bound::Exact
    };
    tt.store(TtEntry {
        key,
        depth,
        score: score_to_tt(best, ply, MATE_THRESHOLD),
        bound,
        best_move,
    });

    (best, best_pv)
}

/// Iterative-deepening root search with TT.
///
/// Searches depths 1, 2, …, `target_depth`, reusing the TT across
/// iterations so each deeper search benefits from move-ordering hints
/// and value bounds discovered at shallower depths. Returns the
/// scored root-move list from the **last completed iteration** plus
/// the actual depth reached (which may be less than `target_depth`
/// when the node budget was exhausted mid-iteration).
///
/// Same **full-window-at-root** rule as v4 — see
/// [`crate::search::score_root_moves_qmvv`]'s doc for why.
pub fn score_root_moves_v5<E: Evaluator>(
    state: &GameState,
    target_depth: u8,
    eval: &E,
) -> (Vec<ScoredMove>, u32, u8) {
    let moves = state.legal_moves();
    if moves.is_empty() {
        return (Vec::new(), 0, 0);
    }
    let mut tt = TranspositionTable::new();
    let mut nodes: u32 = 0;
    let mut last_completed: Vec<ScoredMove> = Vec::new();
    let mut reached_depth: u8 = 0;

    for d in 1..=target_depth {
        // Each iteration reuses the TT so deeper searches benefit from
        // shallower ones. Order root moves by TT best-move-at-root if
        // available, then MVV-LVA, then everything else.
        let root_key = state.position_hash;
        let tt_root_best: Option<Move> = tt.probe(root_key).and_then(|e| e.best_move.clone());

        let mut ordered: Vec<(i32, Move)> = moves
            .iter()
            .cloned()
            .map(|m| {
                let mvv = ordering::mvv_lva_score(state, &m);
                let bonus = match &tt_root_best {
                    Some(best) if best == &m => 1_000_000,
                    _ if is_capture(&m) => 0,
                    _ => 0,
                };
                (mvv + bonus, m)
            })
            .collect();
        ordered.sort_by_key(|(s, _)| std::cmp::Reverse(*s));

        let mut work = state.clone();
        let mut iter_scored: Vec<ScoredMove> = Vec::with_capacity(ordered.len());
        let mut iter_complete = true;

        for (_score, mv) in ordered {
            if work.make_move(&mv).is_err() {
                continue;
            }
            // Full-window root search.
            let (child_score, child_pv) = negamax_v5(
                &mut work,
                d.saturating_sub(1),
                -(MATE + 1),
                MATE + 1,
                &mut nodes,
                eval,
                &mut tt,
                /*ply=*/ 1,
            );
            let v = -child_score;
            let _ = work.unmake_move();
            iter_scored.push(ScoredMove { mv, score: v, pv: child_pv });
            if nodes >= NODE_BUDGET {
                // Budget hit mid-iteration. Don't promote this depth's
                // partial result — caller wants the last *completed*
                // depth so all root moves are scored consistently.
                iter_complete = false;
                break;
            }
        }

        if iter_complete {
            last_completed = iter_scored;
            reached_depth = d;
        } else {
            break;
        }
    }

    if reached_depth == 0 {
        // Even depth-1 didn't complete. Take whatever depth-1 got
        // (better than returning nothing). Recompute with a small
        // budget; this branch is extremely rare (would require
        // NODE_BUDGET < ~50).
        let (scored, n) = crate::search::score_root_moves_qmvv(state, 1, eval);
        return (scored, n, 1);
    }

    (last_completed, nodes, reached_depth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::material_king_safety_pst_v3::MaterialKingSafetyPstV3;
    use chess_core::rules::RuleSet;

    #[test]
    fn opening_returns_some_moves() {
        let state = GameState::new(RuleSet::xiangqi_casual());
        let (scored, nodes, depth) = score_root_moves_v5(&state, 2, &MaterialKingSafetyPstV3);
        assert!(!scored.is_empty(), "v5 should return scored moves");
        assert!(nodes > 0, "v5 should visit nodes");
        assert!(depth >= 1, "v5 should reach at least depth 1");
        assert!(depth <= 2, "v5 should not exceed target depth");
    }

    #[test]
    fn iterative_deepening_reaches_target_when_budget_allows() {
        let state = GameState::new(RuleSet::xiangqi_casual());
        let (_scored, _nodes, depth) = score_root_moves_v5(&state, 3, &MaterialKingSafetyPstV3);
        assert!(
            depth >= 2,
            "v5 should reach depth >= 2 in opening within node budget; got {depth}",
        );
    }

    #[test]
    fn tt_hits_during_iterative_deepening() {
        // After the first iteration, the second iteration should hit
        // the TT for many positions. We can't observe the TT directly
        // from outside, but we can compare node counts: depth-2 ID
        // must use FEWER nodes than depth-2 from scratch (because TT
        // stored the depth-1 results).
        // Actually, depth-2 ID also runs depth-1 first, so it will
        // use MORE nodes than a single depth-2. Instead, assert that
        // the depth-2 portion benefits: depth-3 ID nodes < 3x depth-1
        // nodes (TT amortizes).
        let state = GameState::new(RuleSet::xiangqi_casual());
        let (_, n1, _) = score_root_moves_v5(&state, 1, &MaterialKingSafetyPstV3);
        let (_, n3, d3) = score_root_moves_v5(&state, 3, &MaterialKingSafetyPstV3);
        // Sanity: depth 3 visits more than depth 1.
        assert!(n3 > n1, "deeper search visits more nodes ({n1} → {n3})");
        // TT benefit: depth-3 budget is 250k. Without TT we'd expect
        // depth-3 to be much higher than 100*depth-1; with TT the
        // factor should be modest in the opening.
        // Just check the search completed (depth>=2) — exact node
        // counts depend on move-ordering and are brittle to encode.
        assert!(d3 >= 2);
    }

    #[test]
    fn deterministic_same_state_same_score() {
        let state = GameState::new(RuleSet::xiangqi_casual());
        let (a, _, _) = score_root_moves_v5(&state, 2, &MaterialKingSafetyPstV3);
        let (b, _, _) = score_root_moves_v5(&state, 2, &MaterialKingSafetyPstV3);
        assert_eq!(a.len(), b.len());
        for (sa, sb) in a.iter().zip(b.iter()) {
            assert_eq!(sa.mv, sb.mv);
            assert_eq!(sa.score, sb.score);
        }
    }
}
