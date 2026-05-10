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
