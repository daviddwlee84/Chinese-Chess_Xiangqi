//! Crate-wide error type.

use thiserror::Error;

use crate::coord::Square;
use crate::piece::Side;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    #[error("illegal move: {0}")]
    Illegal(&'static str),

    #[error("game already ended")]
    GameOver,

    #[error("not your turn (expected side {expected:?}, got {got:?})")]
    WrongSide { expected: Side, got: Side },

    #[error("square out of bounds: {0:?}")]
    OutOfBounds(Square),

    #[error("malformed notation: {0}")]
    BadNotation(String),

    #[error("setup error: {0}")]
    Setup(&'static str),
}
