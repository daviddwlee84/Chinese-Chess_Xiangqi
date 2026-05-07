//! Move history for undo and repetition detection.

use serde::{Deserialize, Serialize};

use crate::coord::Square;
use crate::moves::Move;
use crate::piece::Side;

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct MoveRecord {
    pub mover: Side,
    pub the_move: Move,
    /// Snapshot of `no_progress_plies` BEFORE this move; lets undo restore it.
    pub no_progress_before: u16,
    /// Snapshot of `chain_lock` BEFORE this move; lets undo restore it.
    /// Added in protocol v5; `#[serde(default)]` so older histories load
    /// (chain mode wasn't possible pre-v5, so the default `None` is correct).
    #[serde(default)]
    pub chain_lock_before: Option<Square>,
}
