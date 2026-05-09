//! Move ordering helpers used by negamax + α-β.
//!
//! Better ordering = earlier α-β cutoffs = fewer nodes. The killer
//! ordering for chess-family games is **MVV-LVA**: try captures of
//! valuable pieces by cheap pieces *first*. After all captures, quiet
//! moves come last.
//!
//! v1-v3 used a flat "captures before non-captures" sort. v4 swaps in
//! MVV-LVA at the same call site. Same pruning algorithm, fewer
//! nodes per search.

use chess_core::moves::Move;
use chess_core::piece::PieceKind;
use chess_core::state::GameState;

/// MVV-LVA-style score for a single move. Higher = try first.
///
/// - Captures: `victim_value * 10 - attacker_value`. The `*10` ensures
///   any capture beats any quiet move; the subtraction breaks ties so
///   "soldier takes chariot" beats "chariot takes chariot".
/// - Reveals (banqi): high constant — they always change material/info,
///   never bypassed for a quiet move. (Banqi engines are out of scope
///   for v4 but the score is well-defined.)
/// - Quiet `Step`: 0.
/// - `EndChain`: 0 (administrative move).
///
/// Returned as `i32` so callers can use `sort_by_key(Reverse(score))`
/// without overflow concerns.
pub fn mvv_lva_score(state: &GameState, m: &Move) -> i32 {
    match m {
        Move::Capture { captured, from, .. } => {
            let attacker_v = piece_value_for_ordering(state, *from);
            let victim_v = kind_value_for_ordering(captured.kind);
            victim_v * 10 - attacker_v
        }
        Move::CannonJump { captured, from, .. } => {
            let attacker_v = piece_value_for_ordering(state, *from);
            let victim_v = kind_value_for_ordering(captured.kind);
            // Cannon jumps are a discovery for the attacker; bias them
            // up slightly over plain captures of the same victim.
            victim_v * 10 - attacker_v + 5
        }
        Move::ChainCapture { from, path } => {
            let attacker_v = piece_value_for_ordering(state, *from);
            let chain_v: i32 = path.iter().map(|h| kind_value_for_ordering(h.captured.kind)).sum();
            chain_v * 10 - attacker_v
        }
        Move::DarkCapture { revealed, attacker, .. } => {
            // Both fields are Option<_> on the wire; engines fill them
            // in via `make_move`. For pre-apply ordering (root + recursion)
            // we may not have them yet — fall back to a moderate constant.
            match (revealed, attacker) {
                (Some(r), Some(a)) => {
                    kind_value_for_ordering(r.kind) * 10 - kind_value_for_ordering(a.kind)
                }
                _ => 100,
            }
        }
        Move::Reveal { .. } => 50,
        Move::Step { .. } => 0,
        Move::EndChain { .. } => 0,
    }
}

/// Compact piece-value table used **only** for move ordering (NOT for
/// evaluation — the real eval has its own piece values). Slight tuning
/// vs the eval table to bias chariot trades.
#[inline]
fn kind_value_for_ordering(k: PieceKind) -> i32 {
    match k {
        // General excluded — its value is "you instantly lost", which
        // dwarfs everything else, but in MVV-LVA terms we want the
        // ordering to reflect "this move is critically attractive".
        // Use a very large constant.
        PieceKind::General => 10_000,
        PieceKind::Chariot => 900,
        PieceKind::Cannon => 450,
        PieceKind::Horse => 400,
        PieceKind::Advisor => 200,
        PieceKind::Elephant => 200,
        PieceKind::Soldier => 100,
    }
}

/// Read the attacker's piece value from the board. Returns 0 if the
/// `from` square is empty (defensive — shouldn't happen for a legal
/// move but the function must be total).
fn piece_value_for_ordering(state: &GameState, from: chess_core::coord::Square) -> i32 {
    state.board.get(from).map(|pos| kind_value_for_ordering(pos.piece.kind)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_core::board::Board;
    use chess_core::coord::{File, Rank, Square};
    use chess_core::piece::{Piece, PieceKind, PieceOnSquare, Side};
    use chess_core::rules::RuleSet;

    fn empty_state() -> GameState {
        let mut state = GameState::new(RuleSet::xiangqi_casual());
        let board: Board = state.board.clone();
        let squares: Vec<Square> = board.squares().collect();
        for sq in squares {
            state.board.set(sq, None);
        }
        state
    }

    fn place(state: &mut GameState, sq: Square, side: Side, kind: PieceKind) {
        state.board.set(sq, Some(PieceOnSquare::revealed(Piece::new(side, kind))));
    }

    #[test]
    fn mvv_lva_prefers_high_value_victim() {
        let mut state = empty_state();
        let from = state.board.sq(File(0), Rank(0));
        let to1 = state.board.sq(File(1), Rank(0));
        let to2 = state.board.sq(File(2), Rank(0));
        place(&mut state, from, Side::RED, PieceKind::Soldier);
        place(&mut state, to1, Side::BLACK, PieceKind::Soldier); // weak victim
        place(&mut state, to2, Side::BLACK, PieceKind::Chariot); // strong victim

        let weak =
            Move::Capture { from, to: to1, captured: Piece::new(Side::BLACK, PieceKind::Soldier) };
        let strong =
            Move::Capture { from, to: to2, captured: Piece::new(Side::BLACK, PieceKind::Chariot) };
        assert!(
            mvv_lva_score(&state, &strong) > mvv_lva_score(&state, &weak),
            "Capturing chariot should outrank capturing soldier"
        );
    }

    #[test]
    fn mvv_lva_prefers_cheap_attacker_for_same_victim() {
        let mut state = empty_state();
        let chariot_from = state.board.sq(File(0), Rank(0));
        let soldier_from = state.board.sq(File(2), Rank(0));
        let victim = state.board.sq(File(1), Rank(0));
        place(&mut state, chariot_from, Side::RED, PieceKind::Chariot);
        place(&mut state, soldier_from, Side::RED, PieceKind::Soldier);
        place(&mut state, victim, Side::BLACK, PieceKind::Horse);

        let by_chariot = Move::Capture {
            from: chariot_from,
            to: victim,
            captured: Piece::new(Side::BLACK, PieceKind::Horse),
        };
        let by_soldier = Move::Capture {
            from: soldier_from,
            to: victim,
            captured: Piece::new(Side::BLACK, PieceKind::Horse),
        };
        assert!(
            mvv_lva_score(&state, &by_soldier) > mvv_lva_score(&state, &by_chariot),
            "Soldier-takes-horse should outrank chariot-takes-horse (cheap attacker preferred)"
        );
    }

    #[test]
    fn captures_outrank_any_quiet_move() {
        let mut state = empty_state();
        let from = state.board.sq(File(0), Rank(0));
        let to = state.board.sq(File(1), Rank(0));
        place(&mut state, from, Side::RED, PieceKind::Chariot);
        place(&mut state, to, Side::BLACK, PieceKind::Soldier);

        let cap = Move::Capture { from, to, captured: Piece::new(Side::BLACK, PieceKind::Soldier) };
        let quiet = Move::Step { from, to: state.board.sq(File(0), Rank(1)) };
        assert!(
            mvv_lva_score(&state, &cap) > mvv_lva_score(&state, &quiet),
            "any capture must outrank any quiet move"
        );
    }
}
