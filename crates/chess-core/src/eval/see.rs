//! Static Exchange Evaluation + piece value table.
//!
//! See module-level docs in `crate::eval`.

use crate::board::{Board, RegionKind};
use crate::coord::Square;
use crate::piece::{Piece, PieceKind, PieceOnSquare, Side};
use crate::rules::Variant;
use crate::state::GameState;

/// Sentinel "kingly" value used so that any exchange involving the
/// general dominates piece-value comparisons. Anything close to this
/// magnitude in a SEE result means the general is on the table —
/// callers should treat that as a checkmate / general-capture
/// situation, not a piece trade.
pub const SEE_GENERAL_VALUE: i16 = 10_000;

/// Material value of `piece` standing on `sq` in `state`'s position.
///
/// Xiangqi only: a soldier that has crossed the river is worth +1
/// because it gains the lateral step. Banqi treats all soldiers the
/// same (the past-river concept doesn't apply on a 4×8 board).
pub fn piece_value(piece: Piece, sq: Square, state: &GameState) -> i16 {
    match piece.kind {
        PieceKind::General => SEE_GENERAL_VALUE,
        PieceKind::Chariot => 9,
        PieceKind::Cannon => 5,
        PieceKind::Horse => 4,
        PieceKind::Advisor => 2,
        PieceKind::Elephant => 2,
        PieceKind::Soldier => soldier_value(piece, sq, state),
    }
}

fn soldier_value(piece: Piece, sq: Square, state: &GameState) -> i16 {
    if state.rules.variant != Variant::Xiangqi {
        return 1;
    }
    if state.board.in_region(sq, RegionKind::HomeHalf(piece.side)) {
        1
    } else {
        2
    }
}

/// Static Exchange Evaluation on `target_sq`.
///
/// Returns the **net material gain** for `attacker_side` if it
/// initiates a chain of captures on `target_sq`, with both sides
/// optimally choosing whether to recapture. Positive = attacker
/// profits; zero = equal trade; negative = attacker should not
/// initiate.
///
/// Returns `0` if there is nothing to capture on `target_sq` or if
/// `attacker_side` has no piece that can take it.
///
/// **Approximation**: this is a single-square exchange — discovered
/// attacks (e.g. a cannon revealed by removing a screen) along
/// *other* squares aren't modelled. Good enough for the threat
/// highlight UI; AI search uses its own quiescence pass.
pub fn see(state: &GameState, target_sq: Square, attacker_side: Side) -> i16 {
    let initial = match state.board.get(target_sq) {
        Some(pos) => pos,
        None => return 0,
    };
    // Build the gain stack via repeated least-valuable-attacker captures.
    let target_value = piece_value(initial.piece, target_sq, state);
    let mut working = state.board.clone();
    let variant = state.rules.variant;

    let Some((first_from, first_piece)) =
        pick_least_valuable_attacker(&working, target_sq, attacker_side, variant, state)
    else {
        return 0; // attacker has no way to take the target
    };

    let mut gains: Vec<i16> = Vec::with_capacity(8);
    gains.push(target_value);
    apply_capture(&mut working, first_from, target_sq, first_piece);

    let mut last_attacker_value = piece_value(first_piece, target_sq, state);
    let mut side = attacker_side.opposite();

    loop {
        let Some((from, piece)) =
            pick_least_valuable_attacker(&working, target_sq, side, variant, state)
        else {
            break;
        };
        // gains[d] = (last attacker's value just placed on target) - gains[d-1]
        // — this represents the trade outcome if the recapture happens.
        let prev = *gains.last().expect("gains non-empty");
        gains.push(last_attacker_value - prev);
        apply_capture(&mut working, from, target_sq, piece);
        last_attacker_value = piece_value(piece, target_sq, state);
        side = side.opposite();
    }

    negamax_retract(&mut gains);
    gains[0]
}

/// Standard SEE retraction: walking back from the deepest gain entry,
/// each side may decline to recapture if doing so leaves them worse
/// off (`gain[d-1] = -max(-gain[d-1], gain[d])`).
fn negamax_retract(gains: &mut Vec<i16>) {
    while gains.len() > 1 {
        let last = gains.pop().unwrap();
        let prev = gains.last_mut().unwrap();
        *prev = -((-*prev).max(last));
    }
}

/// Place `piece` (formerly at `from`) onto `target_sq`, removing
/// whatever was there. Used by `see` to roll the simulated exchange
/// forward one ply.
fn apply_capture(board: &mut Board, from: Square, target_sq: Square, piece: Piece) {
    board.set(from, None);
    board.set(target_sq, Some(PieceOnSquare::revealed(piece)));
}

/// Find the lowest-valued piece of `side` that can capture `target_sq`
/// in a single ply, given the current `board`. `state` is used only
/// for the (variant + soldier-side-of-river) value lookup; piece
/// positions come from `board`.
fn pick_least_valuable_attacker(
    board: &Board,
    target_sq: Square,
    side: Side,
    variant: Variant,
    state: &GameState,
) -> Option<(Square, Piece)> {
    let attackers = match variant {
        Variant::Xiangqi => crate::rules::xiangqi::attackers_of(board, target_sq, side),
        Variant::Banqi | Variant::ThreeKingdomBanqi => {
            crate::rules::banqi::attackers_of(board, target_sq, side, state.rules.house)
        }
    };
    attackers.into_iter().min_by_key(|(sq, piece)| piece_value(*piece, *sq, state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleSet;

    /// Three-chariot mating-net fixture (also used in
    /// `view::tests::xiangqi_in_check_view_flags_observer`). Black
    /// general at e9 is attacked by Red rooks on d8/e8/f8; Red
    /// general at e0. Black's only piece is the general — useful for
    /// 'undefended target' SEE assertions.
    fn three_chariot_mate() -> GameState {
        let pos = "variant: xiangqi\nside_to_move: black\n\nboard:\n  . . . . k . . . .\n  . . . R R R . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . K . . . .\n";
        GameState::from_pos_text(pos).expect("parse pos")
    }

    /// 'No-attacker' baseline: SEE returns 0 when nobody threatens
    /// the target square (defender is safe — no exchange to model).
    /// Trivially true for the opening position from any opponent
    /// piece's vantage point — every piece is safely behind its line.
    #[test]
    fn see_returns_zero_when_no_attacker() {
        let state = GameState::new(RuleSet::xiangqi());
        // Red chariot at file 0 rank 0 — nothing attacks it in the
        // opening position (the cannon at h2 / b2 doesn't have a
        // screen yet).
        let chariot_sq = state.board.sq(crate::coord::File(0), crate::coord::Rank(0));
        assert_eq!(see(&state, chariot_sq, Side::BLACK), 0);
    }

    /// 'Free piece' baseline: an unprotected target returns its full
    /// value to the attacker. Three-chariot fixture's Black general
    /// is undefended (no Black pieces other than itself); SEE for
    /// Red against the general returns the SEE_GENERAL_VALUE
    /// sentinel.
    #[test]
    fn see_undefended_general_returns_general_value() {
        let state = three_chariot_mate();
        let king_sq = state.board.sq(crate::coord::File(4), crate::coord::Rank(9));
        let gain = see(&state, king_sq, Side::RED);
        assert_eq!(gain, SEE_GENERAL_VALUE);
    }

    /// Equal-trade baseline: chariot vs chariot with both sides
    /// having one defender → SEE = 0 (attacker takes, defender
    /// recaptures, net zero). Hand-built position avoids the noise
    /// of the full opening.
    #[test]
    fn see_equal_trade_returns_zero() {
        // Both files 4. Red chariot at e2 attacks Black chariot at
        // e7; Black soldier at e8 defends e7. Soldier is rank-3 vs
        // chariot value 9, so Red CAN profit if the defender is
        // mis-modelled — the SEE must correctly retract once
        // defender's recapture brings the trade back to zero.
        let pos = "variant: xiangqi\nside_to_move: red\n\nboard:\n  . . . . k . . . .\n  . . . . p . . . .\n  . . . . r . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . R . . . .\n  . . . . . . . . .\n  . . . . K . . . .\n";
        let state = GameState::from_pos_text(pos).expect("parse pos");
        let target = state.board.sq(crate::coord::File(4), crate::coord::Rank(7));
        // Red value: chariot (9) - chariot (9) = 0.
        assert_eq!(see(&state, target, Side::RED), 0);
    }

    /// Soldier-attacks-chariot regression: a single past-river soldier
    /// attacking an undefended chariot wins material (chariot 9 - 0
    /// recapture = +9 for the attacker side). Confirms the SEE
    /// considers the soldier as the cheapest attacker (it's also the
    /// only one in this construction) and doesn't get confused by
    /// the past-river value bump (which is for the soldier itself,
    /// not the captured piece).
    #[test]
    fn see_soldier_takes_undefended_chariot() {
        // Red soldier at e5 (past-river) attacks Black chariot at e6.
        // No Black defender adjacent.
        let pos = "variant: xiangqi\nside_to_move: red\n\nboard:\n  . . . . k . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . r . . . .\n  . . . . P . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . . . . . .\n  . . . . K . . . .\n";
        let state = GameState::from_pos_text(pos).expect("parse pos");
        let target = state.board.sq(crate::coord::File(4), crate::coord::Rank(6));
        assert_eq!(see(&state, target, Side::RED), 9);
    }

    /// Past-river soldier value bump: the value table promotes a
    /// soldier on the opponent's half of the board from 1 to 2.
    /// Round-trip verifies both halves of the if-branch.
    #[test]
    fn soldier_value_bumps_after_river() {
        let state = GameState::new(RuleSet::xiangqi());
        // Red soldier at file 0 rank 3 (still on red's home half).
        let home = state.board.sq(crate::coord::File(0), crate::coord::Rank(3));
        let red_soldier = Piece::new(Side::RED, crate::piece::PieceKind::Soldier);
        assert_eq!(piece_value(red_soldier, home, &state), 1);
        // Same piece pretend-placed on file 0 rank 6 (past river for Red).
        let past = state.board.sq(crate::coord::File(0), crate::coord::Rank(6));
        assert_eq!(piece_value(red_soldier, past, &state), 2);
    }
}
