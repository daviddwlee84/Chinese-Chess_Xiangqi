//! Move enum.
//!
//! Flat enum (not a trait) so serde, equality, and pattern matching work
//! without ceremony. The `Reveal` variant carries `Option<Piece>` because
//! the network ABI distinguishes pre-flip (`None` from client) from
//! post-flip (`Some` from authoritative server). See ADR-0004.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::coord::Square;
use crate::piece::Piece;

/// One logical move from a player's perspective.
///
/// `ChainCapture` is a single move even though it encodes a sequence —
/// undo pops once, history records once, the network ships one message.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum Move {
    /// Banqi: flip the face-down piece at `at`. The identity is `None`
    /// when the client requests the flip and `Some(_)` once the
    /// authoritative engine has resolved it.
    Reveal { at: Square, revealed: Option<Piece> },

    /// Plain non-capturing slide.
    Step { from: Square, to: Square },

    /// Capture (single hop). `captured` is recorded so undo can restore.
    Capture { from: Square, to: Square, captured: Piece },

    /// Chain capture (banqi house rule 連吃 / 暗連). `path` is the sequence
    /// of hops, each of which is a capture. Last hop's `to` is the final
    /// landing square. Length ≥ 1.
    ChainCapture { from: Square, path: SmallVec<[ChainHop; 4]> },

    /// Cannon capture-by-jump. `screen` is the piece jumped over.
    CannonJump { from: Square, to: Square, screen: Square, captured: Piece },
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct ChainHop {
    pub to: Square,
    pub captured: Piece,
}

/// Move-list type alias — sized to avoid heap allocation in typical positions.
pub type MoveList = SmallVec<[Move; 32]>;

impl Move {
    /// Origin square of the move (where the moving piece started).
    /// `Reveal` returns the flip square.
    #[inline]
    pub fn origin_square(&self) -> Square {
        match self {
            Move::Reveal { at, .. } => *at,
            Move::Step { from, .. }
            | Move::Capture { from, .. }
            | Move::ChainCapture { from, .. }
            | Move::CannonJump { from, .. } => *from,
        }
    }

    /// Destination square (where the moving piece ended). `Reveal` has none.
    #[inline]
    pub fn to_square(&self) -> Option<Square> {
        match self {
            Move::Reveal { .. } => None,
            Move::Step { to, .. } | Move::Capture { to, .. } | Move::CannonJump { to, .. } => {
                Some(*to)
            }
            Move::ChainCapture { path, .. } => path.last().map(|h| h.to),
        }
    }

    /// Whether this move resets the no-progress counter (capture or reveal).
    #[inline]
    pub fn resets_no_progress(&self) -> bool {
        matches!(
            self,
            Move::Reveal { .. }
                | Move::Capture { .. }
                | Move::ChainCapture { .. }
                | Move::CannonJump { .. }
        )
    }
}
