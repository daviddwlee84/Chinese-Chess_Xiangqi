//! Banqi move generation.
//!
//! Base rules (always emitted):
//! - `Reveal` for every face-down piece.
//! - 1-step orthogonal `Step` / `Capture` for face-up own pieces.
//! - Cannon `CannonJump` over exactly one screen (face-up or face-down).
//!
//! House-rule extensions:
//! - `CHAIN_CAPTURE`: emit `ChainCapture` for face-up consecutive captures
//!   along the same direction (length ≥ 2).
//! - `CHARIOT_RUSH`: chariot uses xiangqi-style multi-square ray for both
//!   slides and captures (rank-ignoring on captures with a gap).
//! - `DARK_CAPTURE` (`暗吃`): atomic reveal+capture against face-down
//!   targets. `DARK_CAPTURE_TRADE` (implies DARK_CAPTURE) makes
//!   rank-fail kill the attacker instead of probing in place.
//! - `HORSE_DIAGONAL` (`馬斜`): horse adds 4 diagonal one-step moves;
//!   diagonal captures ignore rank (any piece).
//!
//! Deferred (toggles ship as accepted-but-no-effect):
//! - `CANNON_FAST_MOVE`: cannon non-capturing slide.

use smallvec::SmallVec;

use crate::board::Board;
use crate::coord::{Direction, Square};
use crate::moves::{ChainHop, Move, MoveList};
use crate::piece::{Piece, PieceKind, Side};
use crate::rules::HouseRules;
use crate::state::GameState;

pub fn generate(state: &GameState, out: &mut MoveList) {
    let house = state.rules.house;
    gen_reveals(state, out);

    // After the first flip, the active seat may control the OPPOSITE
    // piece-color from its seat name. `current_color()` consults the
    // banqi side_assignment mapping; before the first reveal it falls
    // back to side_to_move (no constraint, since only Reveal moves are
    // generated above).
    let me = state.current_color();
    let board = &state.board;

    for sq in board.squares() {
        let Some(pos) = board.get(sq) else { continue };
        if !pos.revealed || pos.piece.side != me {
            continue;
        }
        gen_for_face_up_piece(state, sq, pos.piece, house, out);
    }
}

fn gen_reveals(state: &GameState, out: &mut MoveList) {
    // Banqi rule: ANY face-down piece can be flipped on a player's turn,
    // regardless of which side controls the seat. The first flip locks
    // the side assignment (handled in state::make_move).
    for sq in state.board.squares() {
        if let Some(pos) = state.board.get(sq) {
            if !pos.revealed {
                out.push(Move::Reveal { at: sq, revealed: None });
            }
        }
    }
}

fn gen_for_face_up_piece(
    state: &GameState,
    from: Square,
    piece: Piece,
    house: HouseRules,
    out: &mut MoveList,
) {
    let board = &state.board;

    // 1-step orthogonal (and the 連吃 chains it seeds).
    for &dir in &Direction::ORTHOGONAL {
        let Some(to) = board.step(from, dir) else { continue };
        match board.get(to) {
            None => {
                // Empty: 1-step slide.
                out.push(Move::Step { from, to });
            }
            Some(target) if !target.revealed => {
                // Face-down piece — blocks unless DARK_CAPTURE is on.
                // Cannon adjacent capture is illegal in standard banqi
                // (cannons capture only via jump-over-screen), so we
                // never emit a 1-step DarkCapture for a Cannon attacker
                // — its only DarkCapture path is the jump emitted in
                // `gen_cannon_jumps`.
                if house.contains(HouseRules::DARK_CAPTURE) && piece.kind != PieceKind::Cannon {
                    out.push(Move::DarkCapture { from, to, revealed: None, attacker: None });
                }
            }
            Some(target) if target.piece.side == piece.side => {
                // Own piece: blocked.
            }
            Some(target) => {
                if can_capture(piece.kind, target.piece.kind) {
                    out.push(Move::Capture { from, to, captured: target.piece });
                    if house.contains(HouseRules::CHAIN_CAPTURE) {
                        gen_chain_extensions(board, piece, from, to, dir, target.piece, out);
                    }
                }
            }
        }
    }

    // 馬斜: horse adds 4 diagonal one-step moves; diagonal captures ignore rank.
    if piece.kind == PieceKind::Horse && house.contains(HouseRules::HORSE_DIAGONAL) {
        gen_horse_diagonal(board, from, piece, house, out);
    }

    // Chariot rush replaces the chariot's 1-step capture with a ray.
    // We've already emitted the 1-step Step/Capture above; the ray emits
    // additional sliding moves. The 1-step capture is still in the list
    // (a strict subset of CHARIOT_RUSH's possibilities).
    if piece.kind == PieceKind::Chariot && house.contains(HouseRules::CHARIOT_RUSH) {
        gen_chariot_rush(board, from, piece.side, house, out);
    }

    // Cannon: jump-over-screen captures (always, this is base banqi).
    // Hidden targets past the screen become 炮暗吃 DarkCapture moves
    // when DARK_CAPTURE is on — the outcome resolver bypasses rank.
    if piece.kind == PieceKind::Cannon {
        gen_cannon_jumps(board, from, piece.side, house, out);
    }
}

/// 馬斜 — horse may *capture* diagonally one step (any piece, rank
/// ignored). Diagonal **non-capturing** moves are NOT allowed: a horse
/// without an enemy diagonal neighbour stays on the orthogonal grid.
/// Hidden diagonal targets become DarkCapture when the dark-capture
/// flag is on; that DarkCapture also ignores rank at apply-time
/// because the diagonal-attack precedent is "any piece" (the same
/// reason it's a "house rule" rather than standard).
fn gen_horse_diagonal(
    board: &Board,
    from: Square,
    piece: Piece,
    house: HouseRules,
    out: &mut MoveList,
) {
    for &dir in &Direction::DIAGONAL {
        let Some(to) = board.step(from, dir) else { continue };
        match board.get(to) {
            // Empty diagonal: blocked. Diagonal moves require a capture.
            None => {}
            Some(target) if !target.revealed => {
                if house.contains(HouseRules::DARK_CAPTURE) {
                    out.push(Move::DarkCapture { from, to, revealed: None, attacker: None });
                }
            }
            Some(target) if target.piece.side == piece.side => {
                // Own piece: blocked.
            }
            Some(target) => {
                // Any-piece diagonal capture (rank ignored).
                out.push(Move::Capture { from, to, captured: target.piece });
            }
        }
    }
}

fn gen_chariot_rush(
    board: &Board,
    from: Square,
    side: Side,
    house: HouseRules,
    out: &mut MoveList,
) {
    for &dir in &Direction::ORTHOGONAL {
        let (walked, blocker) = board.ray(from, dir);
        // Steps 1..N are non-capturing slides. Step 1 was already emitted
        // by the base rule, but it's harmless to push duplicates — `make_move`
        // is idempotent w.r.t. matching moves and the legal-moves consumer
        // dedups via SmallVec ordering. Actually let's avoid duplicates:
        for sq in walked.iter().skip(1) {
            out.push(Move::Step { from, to: *sq });
        }
        if let Some(target_sq) = blocker {
            if let Some(pos) = board.get(target_sq) {
                if pos.revealed && pos.piece.side != side {
                    // Multi-square capture (1-step capture already emitted by base rule).
                    // Rank IGNORED: with a gap, the chariot may capture any piece.
                    if !walked.is_empty() {
                        out.push(Move::Capture { from, to: target_sq, captured: pos.piece });
                    }
                } else if !pos.revealed
                    && !walked.is_empty()
                    && house.contains(HouseRules::DARK_CAPTURE)
                {
                    // 車衝暗吃: chariot rush onto a face-down blocker via gap.
                    out.push(Move::DarkCapture {
                        from,
                        to: target_sq,
                        revealed: None,
                        attacker: None,
                    });
                }
            }
        }
    }
}

fn gen_cannon_jumps(
    board: &Board,
    from: Square,
    side: Side,
    house: HouseRules,
    out: &mut MoveList,
) {
    for &dir in &Direction::ORTHOGONAL {
        // Find the screen (first occupied square in this direction).
        let (_walked, screen) = board.ray(from, dir);
        let Some(screen_sq) = screen else { continue };

        // Continue past the screen looking for a target.
        let mut cursor = screen_sq;
        while let Some(next) = board.step(cursor, dir) {
            match board.get(next) {
                None => {
                    cursor = next;
                }
                Some(pos) => {
                    if pos.revealed {
                        // Cannon captures any face-up enemy regardless of rank.
                        if pos.piece.side != side {
                            out.push(Move::CannonJump {
                                from,
                                to: next,
                                screen: screen_sq,
                                captured: pos.piece,
                            });
                        }
                    } else if house.contains(HouseRules::DARK_CAPTURE) {
                        // 炮暗吃 (cannon jump-over screen onto a face-down
                        // tile). DarkCapture's outcome resolver bypasses
                        // rank for cannons, so this always succeeds at
                        // apply-time regardless of what the target
                        // turns out to be — matching the standard banqi
                        // rule that cannon jumps capture any piece.
                        out.push(Move::DarkCapture {
                            from,
                            to: next,
                            revealed: None,
                            attacker: None,
                        });
                    }
                    break;
                }
            }
        }
    }
}

/// Extend a 1-step capture into chains of length 2, 3, … along the same
/// direction. Each emitted ChainCapture has `from = seed_from` and a path
/// starting with the seed hop, plus one or more additional captures.
fn gen_chain_extensions(
    board: &Board,
    moving: Piece,
    seed_from: Square,
    seed_to: Square,
    dir: Direction,
    seed_captured: Piece,
    out: &mut MoveList,
) {
    let mut path: SmallVec<[ChainHop; 4]> = SmallVec::new();
    path.push(ChainHop { to: seed_to, captured: seed_captured });

    extend_recursive(board, moving, seed_from, dir, seed_to, &mut path, out);
}

fn extend_recursive(
    board: &Board,
    moving: Piece,
    origin: Square,
    dir: Direction,
    cursor: Square,
    path: &mut SmallVec<[ChainHop; 4]>,
    out: &mut MoveList,
) {
    let Some(next) = board.step(cursor, dir) else { return };
    let Some(target) = board.get(next) else { return };
    if !target.revealed {
        // Chains stop at face-down tiles in this round. Chains-with-dark-hops
        // (true 暗連) is deferred — see plan's Phase 2 backlog.
        return;
    }
    if target.piece.side == moving.side {
        return; // own piece blocks
    }
    if !can_capture(moving.kind, target.piece.kind) {
        return; // outranked
    }
    path.push(ChainHop { to: next, captured: target.piece });
    out.push(Move::ChainCapture { from: origin, path: path.clone() });
    extend_recursive(board, moving, origin, dir, next, path, out);
    path.pop();
}

// ---- Capture rank logic ------------------------------------------------------

/// Whether `attacker` may capture `defender` under standard banqi rank rules.
/// Cannons are not handled here — they capture via `gen_cannon_jumps`.
pub fn can_capture(attacker: PieceKind, defender: PieceKind) -> bool {
    use PieceKind::*;
    match (attacker, defender) {
        // Cannon's only legal capture is the jump; not via this path.
        (Cannon, _) => false,
        // Soldier-beats-General special case.
        (Soldier, General) => true,
        (General, Soldier) => false,
        // Otherwise: outrank the defender (≥).
        (a, d) => a.banqi_rank() >= d.banqi_rank(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::PieceOnSquare;
    use crate::rules::RuleSet;

    fn empty_banqi() -> GameState {
        // Build a banqi state and clear all pieces, for hand-crafted positions.
        let mut state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 0));
        for sq in state.board.squares().collect::<Vec<_>>() {
            state.board.set(sq, None);
        }
        state
    }

    fn place(state: &mut GameState, sq: Square, side: Side, kind: PieceKind, revealed: bool) {
        let p = Piece::new(side, kind);
        state.board.set(
            sq,
            Some(if revealed { PieceOnSquare::revealed(p) } else { PieceOnSquare::hidden(p) }),
        );
    }

    #[test]
    fn fresh_banqi_only_emits_reveals() {
        let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 1));
        let moves = state.legal_moves();
        assert_eq!(moves.len(), 32);
        assert!(moves.iter().all(|m| matches!(m, Move::Reveal { .. })));
    }

    #[test]
    fn rank_capture_table() {
        // High rank captures low.
        assert!(can_capture(PieceKind::General, PieceKind::Advisor));
        assert!(can_capture(PieceKind::Chariot, PieceKind::Horse));
        // Equal rank can also capture.
        assert!(can_capture(PieceKind::Horse, PieceKind::Horse));
        // Low cannot capture high.
        assert!(!can_capture(PieceKind::Soldier, PieceKind::Advisor));
        // Soldier-beats-General; reverse is false.
        assert!(can_capture(PieceKind::Soldier, PieceKind::General));
        assert!(!can_capture(PieceKind::General, PieceKind::Soldier));
        // Cannon goes through the jump path, not this one.
        assert!(!can_capture(PieceKind::Cannon, PieceKind::Soldier));
    }

    #[test]
    fn solo_chariot_one_step_orthogonal() {
        let mut state = empty_banqi();
        let me_sq = state.board.sq(crate::coord::File(1), crate::coord::Rank(1));
        place(&mut state, me_sq, Side::RED, PieceKind::Chariot, true);
        // Make sure side_assignment lets RED move.
        state.side_assignment = Some(crate::state::SideAssignment {
            mapping: smallvec::smallvec![Side::RED, Side::BLACK],
        });
        let moves = state.legal_moves();
        // 4 orthogonal moves, all to empty squares.
        let steps: Vec<_> = moves.iter().filter(|m| matches!(m, Move::Step { .. })).collect();
        assert_eq!(steps.len(), 4);
    }

    #[test]
    fn cannon_jumps_over_screen() {
        let mut state = empty_banqi();
        let cannon_sq = state.board.sq(crate::coord::File(0), crate::coord::Rank(0));
        let screen_sq = state.board.sq(crate::coord::File(0), crate::coord::Rank(2));
        let target_sq = state.board.sq(crate::coord::File(0), crate::coord::Rank(4));
        place(&mut state, cannon_sq, Side::RED, PieceKind::Cannon, true);
        place(&mut state, screen_sq, Side::BLACK, PieceKind::Soldier, true);
        place(&mut state, target_sq, Side::BLACK, PieceKind::General, true);
        state.side_assignment = Some(crate::state::SideAssignment {
            mapping: smallvec::smallvec![Side::RED, Side::BLACK],
        });
        let moves = state.legal_moves();
        let jumps: Vec<_> = moves.iter().filter(|m| matches!(m, Move::CannonJump { .. })).collect();
        assert!(!jumps.is_empty(), "cannon should produce at least one jump capture");
    }

    #[test]
    fn chain_capture_emits_when_enabled() {
        let mut state = empty_banqi();
        // Set HouseRules::CHAIN_CAPTURE on.
        state.rules = RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0);
        // Red horse at b1, two black soldiers at b2 and b3 — capturable since
        // horse(2) >= soldier(0).
        let h = state.board.sq(crate::coord::File(1), crate::coord::Rank(1));
        let s1 = state.board.sq(crate::coord::File(1), crate::coord::Rank(2));
        let s2 = state.board.sq(crate::coord::File(1), crate::coord::Rank(3));
        place(&mut state, h, Side::RED, PieceKind::Horse, true);
        place(&mut state, s1, Side::BLACK, PieceKind::Soldier, true);
        place(&mut state, s2, Side::BLACK, PieceKind::Soldier, true);
        state.side_assignment = Some(crate::state::SideAssignment {
            mapping: smallvec::smallvec![Side::RED, Side::BLACK],
        });
        let moves = state.legal_moves();
        let chains: Vec<_> =
            moves.iter().filter(|m| matches!(m, Move::ChainCapture { .. })).collect();
        assert!(!chains.is_empty(), "chain capture should emit");
        // The 2-hop chain is among them.
        let two_hop =
            chains.iter().any(|m| matches!(m, Move::ChainCapture { path, .. } if path.len() == 2));
        assert!(two_hop, "expected a 2-hop chain capture");
    }

    #[test]
    fn chain_capture_disabled_means_only_single_capture() {
        let mut state = empty_banqi();
        state.rules = RuleSet::banqi_with_seed(HouseRules::empty(), 0);
        let h = state.board.sq(crate::coord::File(1), crate::coord::Rank(1));
        let s1 = state.board.sq(crate::coord::File(1), crate::coord::Rank(2));
        let s2 = state.board.sq(crate::coord::File(1), crate::coord::Rank(3));
        place(&mut state, h, Side::RED, PieceKind::Horse, true);
        place(&mut state, s1, Side::BLACK, PieceKind::Soldier, true);
        place(&mut state, s2, Side::BLACK, PieceKind::Soldier, true);
        state.side_assignment = Some(crate::state::SideAssignment {
            mapping: smallvec::smallvec![Side::RED, Side::BLACK],
        });
        let moves = state.legal_moves();
        let chains: Vec<_> =
            moves.iter().filter(|m| matches!(m, Move::ChainCapture { .. })).collect();
        assert!(chains.is_empty(), "no chains without CHAIN_CAPTURE");
    }

    #[test]
    fn chain_capture_make_unmake_round_trip() {
        let mut state = empty_banqi();
        state.rules = RuleSet::banqi_with_seed(HouseRules::CHAIN_CAPTURE, 0);
        let h = state.board.sq(crate::coord::File(1), crate::coord::Rank(1));
        let s1 = state.board.sq(crate::coord::File(1), crate::coord::Rank(2));
        let s2 = state.board.sq(crate::coord::File(1), crate::coord::Rank(3));
        place(&mut state, h, Side::RED, PieceKind::Horse, true);
        place(&mut state, s1, Side::BLACK, PieceKind::Soldier, true);
        place(&mut state, s2, Side::BLACK, PieceKind::Soldier, true);
        state.side_assignment = Some(crate::state::SideAssignment {
            mapping: smallvec::smallvec![Side::RED, Side::BLACK],
        });

        let snapshot = state.clone();
        let chain = state
            .legal_moves()
            .into_iter()
            .find(|m| matches!(m, Move::ChainCapture { path, .. } if path.len() == 2))
            .expect("chain capture should exist");

        // Verify origin is correct.
        if let Move::ChainCapture { from, .. } = &chain {
            assert_eq!(*from, h, "chain origin must be the horse's starting square");
        }

        state.make_move(&chain).unwrap();
        // Horse should be at s2; s1 and h should be empty.
        assert!(state.board.get(h).is_none());
        assert!(state.board.get(s1).is_none());
        assert!(state.board.get(s2).is_some());

        state.unmake_move().unwrap();
        assert_eq!(state.board, snapshot.board, "board must restore after chain undo");
    }

    #[test]
    fn chariot_rush_emits_long_slides() {
        let mut state = empty_banqi();
        state.rules = RuleSet::banqi_with_seed(HouseRules::CHARIOT_RUSH, 0);
        let c = state.board.sq(crate::coord::File(0), crate::coord::Rank(0));
        place(&mut state, c, Side::RED, PieceKind::Chariot, true);
        state.side_assignment = Some(crate::state::SideAssignment {
            mapping: smallvec::smallvec![Side::RED, Side::BLACK],
        });
        let moves = state.legal_moves();
        // Without rush: chariot at corner has 2 moves (N, E). With rush:
        // up to 3+7 = 10 moves along the long axis.
        assert!(moves.len() >= 5, "chariot rush should produce more slides");
    }
}
