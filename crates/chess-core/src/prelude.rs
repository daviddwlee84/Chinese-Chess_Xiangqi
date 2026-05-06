//! Crate prelude. `use chess_core::prelude::*;` for the common API.

pub use crate::coord::{Direction, File, Rank, Square};
pub use crate::error::CoreError;
pub use crate::piece::{Piece, PieceKind, PieceOnSquare, Side};
