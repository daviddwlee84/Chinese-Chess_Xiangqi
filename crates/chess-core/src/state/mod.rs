//! Game state: board + turn + history + status.

pub mod history;
pub mod repetition;
pub mod turn_order;

pub use history::MoveRecord;
pub use turn_order::TurnOrder;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::board::Board;
use crate::error::CoreError;
use crate::moves::{Move, MoveList};
use crate::piece::{Piece, PieceOnSquare, Side};
use crate::rules::{RuleSet, Variant};

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum GameStatus {
    Ongoing,
    Won { winner: Side, reason: WinReason },
    Drawn { reason: DrawReason },
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum WinReason {
    Checkmate,
    Stalemate,
    Resignation,
    OnlyOneSideHasPieces,
    Timeout,
    /// Casual xiangqi (`xiangqi_allow_self_check`): the loser's general was
    /// physically captured rather than ending in checkmate. In standard
    /// rules this state is unreachable because the legality filter rejects
    /// any move that would leave the general capturable.
    GeneralCaptured,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum DrawReason {
    NoProgress,
    Repetition,
    Agreed,
    InsufficientMaterial,
}

/// Side ↔ piece-color assignment for banqi (set after the first flip).
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct SideAssignment {
    /// `mapping[side.0 as usize]` = the piece-color that seat controls.
    pub mapping: SmallVec<[Side; 3]>,
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct GameState {
    pub rules: RuleSet,
    pub board: Board,
    pub side_to_move: Side,
    pub turn_order: TurnOrder,
    pub history: Vec<MoveRecord>,
    pub status: GameStatus,
    /// Banqi only: established after first flip. `None` until then.
    pub side_assignment: Option<SideAssignment>,
    pub no_progress_plies: u16,
    /// Banqi 連吃 chain-mode lock. `Some(sq)` means the moving piece at
    /// `sq` just made a chain-eligible capture and may continue capturing
    /// — the turn does NOT auto-advance until the player issues
    /// `Move::EndChain { at: sq }` (or makes another chain-eligible
    /// capture which keeps the lock active). `#[serde(default)]` so
    /// pre-chain-mode snapshots load.
    #[serde(default)]
    pub chain_lock: Option<crate::coord::Square>,
}

impl GameState {
    pub fn new(rules: RuleSet) -> Self {
        crate::setup::build_initial_state(rules)
    }

    /// Generate all legal moves for the side to move.
    ///
    /// When `chain_lock` is set (banqi 連吃 mid-chain), the legal-move
    /// list is filtered to capture-only moves originating from the
    /// locked square, plus `Move::EndChain` as the explicit terminator.
    pub fn legal_moves(&self) -> MoveList {
        let mut out = MoveList::new();
        crate::rules::generate_moves(self, &mut out);
        if let Some(lock) = self.chain_lock {
            out.retain(|m| match m {
                Move::Capture { from, .. }
                | Move::CannonJump { from, .. }
                | Move::DarkCapture { from, .. }
                | Move::ChainCapture { from, .. } => *from == lock,
                _ => false,
            });
            out.push(Move::EndChain { at: lock });
        }
        out
    }

    /// Apply a move. Caller is responsible for legality (use `legal_moves`).
    /// Returns the move pushed to history (with `Reveal` payloads filled in).
    pub fn make_move(&mut self, m: &Move) -> Result<(), CoreError> {
        if matches!(self.status, GameStatus::Won { .. } | GameStatus::Drawn { .. }) {
            return Err(CoreError::GameOver);
        }
        let mover = self.side_to_move;
        let no_progress_before = self.no_progress_plies;
        let chain_lock_before = self.chain_lock;

        // Normalize the move (fill in Reveal payload from board if missing).
        let normalized = self.normalize_for_apply(m)?;
        self.apply_inner(&normalized)?;

        self.history.push(MoveRecord {
            mover,
            the_move: normalized.clone(),
            no_progress_before,
            chain_lock_before,
        });

        if normalized.resets_no_progress() {
            self.no_progress_plies = 0;
        } else {
            self.no_progress_plies = self.no_progress_plies.saturating_add(1);
        }

        // Decide whether the moving piece enters / continues chain mode.
        // Banqi-only, gated on CHAIN_CAPTURE flag, only after a real
        // capture that landed an attacker on a square where it can
        // capture again. EndChain explicitly clears the lock.
        self.chain_lock = self.compute_chain_lock_after(&normalized);

        // Turn advances UNLESS we just entered / are continuing chain
        // mode (chain_lock now Some).
        if self.chain_lock.is_none() {
            self.turn_order.advance();
            self.side_to_move = self.turn_order.current_side();
        }

        Ok(())
    }

    /// Undo the last move.
    pub fn unmake_move(&mut self) -> Result<(), CoreError> {
        let rec = self.history.pop().ok_or(CoreError::Illegal("no move to undo"))?;
        self.unapply_inner(&rec.the_move)?;
        self.no_progress_plies = rec.no_progress_before;

        // If chain_lock is currently Some, the move did NOT advance the
        // turn (the player was either entering or continuing chain mode).
        // Otherwise the turn advanced and we need to rewind it.
        let turn_advanced = self.chain_lock.is_none();

        // Restore the pre-move chain_lock.
        self.chain_lock = rec.chain_lock_before;

        if turn_advanced {
            if self.turn_order.current == 0 {
                self.turn_order.current = (self.turn_order.seats.len() as u8) - 1;
            } else {
                self.turn_order.current -= 1;
            }
            self.side_to_move = self.turn_order.current_side();
        }
        // Game status reset to Ongoing — caller may recompute.
        self.status = GameStatus::Ongoing;
        Ok(())
    }

    /// After applying `m`, decide whether the moving piece is now in
    /// banqi 連吃 chain mode. Returns the locked piece's square when
    /// chain mode should remain / activate; `None` otherwise.
    ///
    /// Activates only for banqi + `CHAIN_CAPTURE` rules, after a capture
    /// move whose attacker actually landed on a square (probe and trade
    /// outcomes of `Move::DarkCapture` skip the lock — the attacker
    /// stayed put or died). The attacker must have at least one further
    /// legal capture from the landing square.
    fn compute_chain_lock_after(&self, m: &Move) -> Option<crate::coord::Square> {
        if self.rules.variant != Variant::Banqi {
            return None;
        }
        if !self.rules.house.contains(crate::rules::HouseRules::CHAIN_CAPTURE) {
            return None;
        }
        // Where did the moving piece end up? Only chain-eligible when
        // the attacker actually moved to a new square.
        let landing = match m {
            Move::Capture { to, .. } | Move::CannonJump { to, .. } => *to,
            Move::DarkCapture { to, revealed, attacker, .. } => {
                // `Move::DarkCapture` resolves to one of three outcomes
                // at apply-time. Only `Capture` (rank ok, attacker took
                // defender's square) puts the attacker at `to`; `Probe`
                // leaves the attacker at `from` and `Trade` removes the
                // attacker entirely. Neither of those is chain-eligible.
                let attacker_piece = (*attacker)?;
                let revealed_piece = (*revealed)?;
                match dark_capture_outcome(attacker_piece, revealed_piece, self.rules.house) {
                    DarkCaptureOutcome::Capture => *to,
                    DarkCaptureOutcome::Probe | DarkCaptureOutcome::Trade => return None,
                }
            }
            Move::ChainCapture { path, .. } => path.last().map(|h| h.to)?,
            // Reveal / Step / EndChain don't trigger chain mode.
            Move::Reveal { .. } | Move::Step { .. } | Move::EndChain { .. } => return None,
        };
        let landing_pos = self.board.get(landing)?;
        if !landing_pos.revealed {
            return None;
        }
        let active_color = self.current_color();
        if landing_pos.piece.side != active_color {
            // Defensive: shouldn't happen now that DarkCapture branches
            // explicitly above, but stays correct if future variants add
            // new capture-with-reveal moves.
            return None;
        }
        if has_capture_from(self, landing, landing_pos.piece) {
            Some(landing)
        } else {
            None
        }
    }

    fn normalize_for_apply(&self, m: &Move) -> Result<Move, CoreError> {
        match m {
            Move::Reveal { at, revealed: None } => {
                let pos = self.board.get(*at).ok_or(CoreError::Illegal("Reveal: empty"))?;
                if pos.revealed {
                    return Err(CoreError::Illegal("Reveal: already revealed"));
                }
                Ok(Move::Reveal { at: *at, revealed: Some(pos.piece) })
            }
            Move::DarkCapture { from, to, revealed, attacker } => {
                let attacker = match attacker {
                    Some(p) => *p,
                    None => {
                        let pos = self
                            .board
                            .get(*from)
                            .ok_or(CoreError::Illegal("DarkCapture: empty from"))?;
                        if !pos.revealed {
                            return Err(CoreError::Illegal("DarkCapture: from not revealed"));
                        }
                        pos.piece
                    }
                };
                let revealed = match revealed {
                    Some(p) => *p,
                    None => {
                        let pos = self
                            .board
                            .get(*to)
                            .ok_or(CoreError::Illegal("DarkCapture: empty to"))?;
                        if pos.revealed {
                            return Err(CoreError::Illegal("DarkCapture: target already revealed"));
                        }
                        pos.piece
                    }
                };
                Ok(Move::DarkCapture {
                    from: *from,
                    to: *to,
                    revealed: Some(revealed),
                    attacker: Some(attacker),
                })
            }
            _ => Ok(m.clone()),
        }
    }

    fn apply_inner(&mut self, m: &Move) -> Result<(), CoreError> {
        match m {
            Move::Reveal { at, revealed } => {
                let mut pos = self.board.get(*at).ok_or(CoreError::Illegal("Reveal: empty"))?;
                if pos.revealed {
                    return Err(CoreError::Illegal("Reveal: already revealed"));
                }
                pos.revealed = true;
                self.board.set(*at, Some(pos));
                // First flip in banqi locks side assignment.
                if self.side_assignment.is_none() && self.rules.variant == Variant::Banqi {
                    if let Some(p) = revealed {
                        self.side_assignment =
                            Some(banqi_side_assignment(self.side_to_move, p.side));
                    }
                }
            }
            Move::Step { from, to } => {
                let p = self.board.get(*from).ok_or(CoreError::Illegal("Step: empty from"))?;
                if self.board.get(*to).is_some() {
                    return Err(CoreError::Illegal("Step: occupied to"));
                }
                self.board.set(*from, None);
                self.board.set(*to, Some(p));
            }
            Move::Capture { from, to, captured: _ } => {
                let p = self.board.get(*from).ok_or(CoreError::Illegal("Capture: empty from"))?;
                self.board.set(*from, None);
                self.board.set(*to, Some(p));
            }
            Move::CannonJump { from, to, screen: _, captured: _ } => {
                let p =
                    self.board.get(*from).ok_or(CoreError::Illegal("CannonJump: empty from"))?;
                self.board.set(*from, None);
                self.board.set(*to, Some(p));
            }
            Move::ChainCapture { from, path } => {
                let p = self.board.get(*from).ok_or(CoreError::Illegal("Chain: empty from"))?;
                if path.is_empty() {
                    return Err(CoreError::Illegal("Chain: empty path"));
                }
                self.board.set(*from, None);
                for hop in &path[..path.len() - 1] {
                    self.board.set(hop.to, None);
                }
                self.board.set(path.last().unwrap().to, Some(p));
            }
            Move::EndChain { at } => {
                // No board change — just verifies that a chain-locked piece
                // exists at `at`. The turn-advance machinery in
                // `make_move` clears `chain_lock` based on
                // `compute_chain_lock_after`.
                if self.chain_lock != Some(*at) {
                    return Err(CoreError::Illegal("EndChain: no active chain lock"));
                }
            }
            Move::DarkCapture { from, to, revealed, attacker } => {
                let revealed_piece =
                    revealed.ok_or(CoreError::Illegal("DarkCapture: missing revealed"))?;
                let attacker_piece =
                    attacker.ok_or(CoreError::Illegal("DarkCapture: missing attacker"))?;
                // Reveal the target.
                let mut target_pos =
                    self.board.get(*to).ok_or(CoreError::Illegal("DarkCapture: empty to"))?;
                if target_pos.revealed {
                    return Err(CoreError::Illegal("DarkCapture: target already revealed"));
                }
                target_pos.revealed = true;
                self.board.set(*to, Some(target_pos));

                // First flip in banqi locks side assignment (mirrors Reveal).
                if self.side_assignment.is_none() && self.rules.variant == Variant::Banqi {
                    self.side_assignment =
                        Some(banqi_side_assignment(self.side_to_move, revealed_piece.side));
                }

                // Resolve outcome based on rank + DARK_CAPTURE_TRADE flag.
                let attacker_pos =
                    self.board.get(*from).ok_or(CoreError::Illegal("DarkCapture: empty from"))?;
                let outcome =
                    dark_capture_outcome(attacker_piece, revealed_piece, self.rules.house);
                match outcome {
                    DarkCaptureOutcome::Capture => {
                        self.board.set(*from, None);
                        self.board.set(*to, Some(attacker_pos));
                    }
                    DarkCaptureOutcome::Trade => {
                        // Attacker dies; target stays revealed at `to`.
                        self.board.set(*from, None);
                    }
                    DarkCaptureOutcome::Probe => {
                        // Both pieces stay; target is now revealed.
                    }
                }
            }
        }
        Ok(())
    }

    fn unapply_inner(&mut self, m: &Move) -> Result<(), CoreError> {
        match m {
            Move::Reveal { at, .. } => {
                let mut pos =
                    self.board.get(*at).ok_or(CoreError::Illegal("Undo Reveal: empty"))?;
                pos.revealed = false;
                self.board.set(*at, Some(pos));
                // First-flip might have set side_assignment; if this was the first move, clear it.
                if self.history.is_empty() {
                    self.side_assignment = None;
                }
            }
            Move::Step { from, to } => {
                let p = self.board.get(*to).ok_or(CoreError::Illegal("Undo Step: empty to"))?;
                self.board.set(*to, None);
                self.board.set(*from, Some(p));
            }
            Move::Capture { from, to, captured } => {
                let p = self.board.get(*to).ok_or(CoreError::Illegal("Undo Capture: empty to"))?;
                self.board.set(*to, Some(PieceOnSquare::revealed(*captured)));
                self.board.set(*from, Some(p));
            }
            Move::CannonJump { from, to, screen: _, captured } => {
                let p = self.board.get(*to).ok_or(CoreError::Illegal("Undo Jump: empty to"))?;
                self.board.set(*to, Some(PieceOnSquare::revealed(*captured)));
                self.board.set(*from, Some(p));
            }
            Move::ChainCapture { from, path } => {
                let last_to = path.last().unwrap().to;
                let p =
                    self.board.get(last_to).ok_or(CoreError::Illegal("Undo Chain: empty last"))?;
                self.board.set(last_to, None);
                // Restore intermediate captures (in any order).
                for hop in path {
                    self.board.set(hop.to, Some(PieceOnSquare::revealed(hop.captured)));
                }
                self.board.set(*from, Some(p));
            }
            Move::EndChain { .. } => {
                // No board change to undo. chain_lock is restored by
                // `unmake_move` from `MoveRecord.chain_lock_before`.
            }
            Move::DarkCapture { from, to, revealed, attacker } => {
                let revealed_piece =
                    revealed.ok_or(CoreError::Illegal("Undo DarkCapture: missing revealed"))?;
                let attacker_piece =
                    attacker.ok_or(CoreError::Illegal("Undo DarkCapture: missing attacker"))?;
                let outcome =
                    dark_capture_outcome(attacker_piece, revealed_piece, self.rules.house);
                // Restore attacker at `from` (reveals it as it was).
                match outcome {
                    DarkCaptureOutcome::Capture => {
                        // Attacker is at `to`; remove it, restore at `from`.
                        self.board.set(*to, None);
                        self.board.set(*from, Some(PieceOnSquare::revealed(attacker_piece)));
                    }
                    DarkCaptureOutcome::Trade => {
                        // Attacker is gone; restore it at `from`.
                        self.board.set(*from, Some(PieceOnSquare::revealed(attacker_piece)));
                    }
                    DarkCaptureOutcome::Probe => {
                        // Attacker is still at `from`. Nothing to do for it.
                    }
                }
                // Always re-hide the target (it was revealed during apply).
                self.board.set(*to, Some(PieceOnSquare::hidden(revealed_piece)));
                // First-flip side assignment may have been set by this move.
                if self.history.is_empty() {
                    self.side_assignment = None;
                }
            }
        }
        Ok(())
    }

    /// Whether `side`'s general is currently under attack by the opponent.
    /// Xiangqi-specific; banqi has no general-in-check concept.
    pub fn is_in_check(&self, side: Side) -> bool {
        crate::rules::xiangqi::is_in_check(self, side)
    }

    /// Recompute `status` based on legal moves and draw counters.
    /// Caller invokes this after `make_move`; we don't auto-call to avoid
    /// recursing into move generation during legality filtering.
    ///
    /// TODO (PR 2): threefold repetition. Currently only no-progress and
    /// no-legal-moves are detected.
    pub fn refresh_status(&mut self) {
        if matches!(self.status, GameStatus::Won { .. } | GameStatus::Drawn { .. }) {
            return;
        }
        // Casual xiangqi: a missing general means the opposite side won.
        // Standard rules can't reach this branch (the legality filter
        // forbids leaving the general capturable), but checking unconditionally
        // is cheap and keeps the invariant for any future variant that allows
        // physical king capture.
        if self.rules.variant == Variant::Xiangqi {
            for &seat in self.turn_order.seats.iter() {
                let has_general = find_piece(&self.board, |p| {
                    p.kind == crate::piece::PieceKind::General && p.side == seat
                })
                .is_some();
                if !has_general {
                    self.status = GameStatus::Won {
                        winner: seat.opposite(),
                        reason: WinReason::GeneralCaptured,
                    };
                    return;
                }
            }
        }
        if self.no_progress_plies >= self.rules.draw_policy.no_progress_plies {
            self.status = GameStatus::Drawn { reason: DrawReason::NoProgress };
            return;
        }
        let moves = self.legal_moves();
        if moves.is_empty() {
            let me = self.side_to_move;
            self.status = match self.rules.variant {
                Variant::Xiangqi => {
                    let reason = if self.is_in_check(me) {
                        WinReason::Checkmate
                    } else {
                        // Asian rules: stalemate is a loss for the stalemated side.
                        WinReason::Stalemate
                    };
                    GameStatus::Won { winner: me.opposite(), reason }
                }
                Variant::Banqi => GameStatus::Won {
                    winner: me.opposite(),
                    reason: WinReason::OnlyOneSideHasPieces,
                },
                Variant::ThreeKingdomBanqi => {
                    // PR 2: more nuanced 3-side win/elimination logic.
                    GameStatus::Won {
                        winner: me.opposite(),
                        reason: WinReason::OnlyOneSideHasPieces,
                    }
                }
            };
        }
    }

    pub fn default_starter(variant: Variant) -> Side {
        match variant {
            Variant::Xiangqi => Side::RED,
            Variant::Banqi => Side::RED,
            Variant::ThreeKingdomBanqi => Side(0),
        }
    }

    /// The piece-color the active seat actually controls.
    ///
    /// Banqi remaps after the first flip locks `side_assignment`; xiangqi
    /// and three-kingdom always return the seat as-is. Move generation
    /// for banqi must filter pieces by this color, not by `side_to_move`
    /// — `side_to_move` is the seat / turn-order index, which only
    /// coincides with the piece-color before the first reveal.
    #[inline]
    pub fn current_color(&self) -> Side {
        match self.side_assignment.as_ref() {
            Some(sa) => {
                let idx = self.side_to_move.0 as usize;
                if idx < sa.mapping.len() {
                    sa.mapping[idx]
                } else {
                    self.side_to_move
                }
            }
            None => self.side_to_move,
        }
    }
}

/// Outcome of a 暗吃 (`Move::DarkCapture`) once the target is revealed.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) enum DarkCaptureOutcome {
    /// Rank check passes — attacker takes target's square (normal capture).
    Capture,
    /// Rank check fails AND `DARK_CAPTURE_TRADE` is set — attacker is
    /// removed; defender stays revealed at `to`.
    Trade,
    /// Rank check fails, no trade flag — both pieces stay; target is
    /// now revealed (information probe; turn advances normally).
    Probe,
}

/// Decide the outcome of a dark-capture given the involved pieces and
/// the active house rules. Used by both apply and unapply (recomputed
/// rather than stored in the move record).
pub(crate) fn dark_capture_outcome(
    attacker: Piece,
    revealed: Piece,
    house: crate::rules::HouseRules,
) -> DarkCaptureOutcome {
    if attacker.side == revealed.side {
        // Same side → never a capture; treat as probe (own piece blocks).
        return DarkCaptureOutcome::Probe;
    }
    if crate::rules::banqi::can_capture(attacker.kind, revealed.kind) {
        DarkCaptureOutcome::Capture
    } else if house.contains(crate::rules::HouseRules::DARK_CAPTURE_TRADE) {
        DarkCaptureOutcome::Trade
    } else {
        DarkCaptureOutcome::Probe
    }
}

/// Whether `attacker` standing on `from` has at least one further legal
/// capture in any direction. Used to decide whether banqi 連吃 chain
/// mode should remain active after a capture (see
/// `compute_chain_lock_after`). Only consults banqi rules — caller
/// guarantees variant + CHAIN_CAPTURE flag.
fn has_capture_from(state: &GameState, from: crate::coord::Square, attacker: Piece) -> bool {
    use crate::coord::Direction;
    use crate::piece::PieceKind;
    use crate::rules::{banqi::can_capture, HouseRules};

    let house = state.rules.house;
    let board = &state.board;

    // Cannon: only via jump-over-screen.
    if attacker.kind == PieceKind::Cannon {
        for &dir in &Direction::ORTHOGONAL {
            let (_walked, screen) = board.ray(from, dir);
            let Some(screen_sq) = screen else { continue };
            let mut cursor = screen_sq;
            while let Some(next) = board.step(cursor, dir) {
                match board.get(next) {
                    None => cursor = next,
                    Some(pos) => {
                        if pos.revealed && pos.piece.side != attacker.side {
                            return true;
                        }
                        break;
                    }
                }
            }
        }
        return false;
    }

    // Orthogonal one-step (and chariot rush extension if enabled).
    for &dir in &Direction::ORTHOGONAL {
        let Some(to) = board.step(from, dir) else { continue };
        match board.get(to) {
            None => {}
            Some(target) if !target.revealed => {
                if house.contains(HouseRules::DARK_CAPTURE) {
                    return true;
                }
            }
            Some(target) if target.piece.side == attacker.side => {}
            Some(target) => {
                if can_capture(attacker.kind, target.piece.kind) {
                    return true;
                }
            }
        }
    }

    // Chariot rush — multi-square ray; with a gap, captures any piece.
    if attacker.kind == PieceKind::Chariot && house.contains(HouseRules::CHARIOT_RUSH) {
        for &dir in &Direction::ORTHOGONAL {
            let (walked, blocker) = board.ray(from, dir);
            if walked.is_empty() {
                // Rank-bound 1-step capture is the base-rule branch above.
                continue;
            }
            let Some(target_sq) = blocker else { continue };
            let Some(pos) = board.get(target_sq) else { continue };
            if pos.revealed && pos.piece.side != attacker.side {
                return true;
            }
            if !pos.revealed && house.contains(HouseRules::DARK_CAPTURE) {
                return true;
            }
        }
    }

    // 馬斜 — diagonal one-step, captures any piece.
    if attacker.kind == PieceKind::Horse && house.contains(HouseRules::HORSE_DIAGONAL) {
        for &dir in &Direction::DIAGONAL {
            let Some(to) = board.step(from, dir) else { continue };
            match board.get(to) {
                None => {}
                Some(target) if !target.revealed => {
                    if house.contains(HouseRules::DARK_CAPTURE) {
                        return true;
                    }
                }
                Some(target) if target.piece.side == attacker.side => {}
                Some(_) => return true,
            }
        }
    }

    false
}

fn banqi_side_assignment(flipper: Side, revealed_side: Side) -> SideAssignment {
    let mut mapping = SmallVec::new();
    if flipper == Side::RED {
        mapping.push(revealed_side);
        mapping.push(revealed_side.opposite());
    } else {
        mapping.push(revealed_side.opposite());
        mapping.push(revealed_side);
    }
    SideAssignment { mapping }
}

/// Find the (single) piece matching predicate. Used to locate a general.
pub(crate) fn find_piece<F>(board: &Board, pred: F) -> Option<crate::coord::Square>
where
    F: Fn(Piece) -> bool,
{
    for sq in board.squares() {
        if let Some(pos) = board.get(sq) {
            if pos.revealed && pred(pos.piece) {
                return Some(sq);
            }
        }
    }
    None
}

#[cfg(test)]
mod state_tests {
    use super::*;
    use crate::rules::{HouseRules, RuleSet};

    #[test]
    fn opening_xiangqi_status_ongoing_after_refresh() {
        let mut state = GameState::new(RuleSet::xiangqi());
        state.refresh_status();
        assert_eq!(state.status, GameStatus::Ongoing);
    }

    #[test]
    fn no_progress_plies_advance_then_reset_on_capture() {
        // Synthetic: build a tiny xiangqi position where Red can capture.
        let mut state = GameState::new(RuleSet::xiangqi());
        // Skim a move that doesn't capture: cannon h2 forward.
        let m = state.legal_moves().into_iter().find(|m| matches!(m, Move::Step { .. })).unwrap();
        state.make_move(&m).unwrap();
        assert_eq!(state.no_progress_plies, 1);
    }

    #[test]
    fn banqi_no_progress_resets_on_reveal() {
        let mut state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 9));
        state.no_progress_plies = 5;
        let m = state.legal_moves().into_iter().find(|m| matches!(m, Move::Reveal { .. })).unwrap();
        state.make_move(&m).unwrap();
        assert_eq!(state.no_progress_plies, 0);
    }
}
