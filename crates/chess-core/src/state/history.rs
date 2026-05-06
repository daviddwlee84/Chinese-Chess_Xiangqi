//! Move history for undo and repetition detection.

use serde::{Deserialize, Serialize};

use crate::moves::Move;
use crate::piece::Side;

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct MoveRecord {
    pub mover: Side,
    pub the_move: Move,
    /// Snapshot of `no_progress_plies` BEFORE this move; lets undo restore it.
    pub no_progress_before: u16,
}
