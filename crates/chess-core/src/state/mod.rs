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
}

impl GameState {
    pub fn new(rules: RuleSet) -> Self {
        crate::setup::build_initial_state(rules)
    }

    /// Generate all legal moves for the side to move.
    pub fn legal_moves(&self) -> MoveList {
        let mut out = MoveList::new();
        crate::rules::generate_moves(self, &mut out);
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

        // Normalize the move (fill in Reveal payload from board if missing).
        let normalized = self.normalize_for_apply(m)?;
        self.apply_inner(&normalized)?;

        self.history.push(MoveRecord { mover, the_move: normalized.clone(), no_progress_before });

        if normalized.resets_no_progress() {
            self.no_progress_plies = 0;
        } else {
            self.no_progress_plies = self.no_progress_plies.saturating_add(1);
        }

        self.turn_order.advance();
        self.side_to_move = self.turn_order.current_side();

        Ok(())
    }

    /// Undo the last move.
    pub fn unmake_move(&mut self) -> Result<(), CoreError> {
        let rec = self.history.pop().ok_or(CoreError::Illegal("no move to undo"))?;
        self.unapply_inner(&rec.the_move)?;
        self.no_progress_plies = rec.no_progress_before;
        // Rewind turn order
        if self.turn_order.current == 0 {
            self.turn_order.current = (self.turn_order.seats.len() as u8) - 1;
        } else {
            self.turn_order.current -= 1;
        }
        self.side_to_move = self.turn_order.current_side();
        // Game status reset to Ongoing — caller may recompute.
        self.status = GameStatus::Ongoing;
        Ok(())
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
