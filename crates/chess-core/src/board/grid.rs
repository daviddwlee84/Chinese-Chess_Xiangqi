//! Iteration primitives over a `BoardShape`.
//!
//! Storage lives on `Board` in `mod.rs`; this module is the place for
//! helpers that depend only on shape (not on cell contents).

use super::shape::BoardShape;
use crate::coord::Square;

/// Iterate every playable square on a shape, in linear-index order.
pub fn squares_of(shape: BoardShape) -> impl Iterator<Item = Square> {
    let count = shape.cell_count();
    (0..count).filter_map(move |i| if shape.is_playable(i) { Some(Square(i as u16)) } else { None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xiangqi_yields_90_squares() {
        assert_eq!(squares_of(BoardShape::Xiangqi9x10).count(), 90);
    }

    #[test]
    fn banqi_yields_32_squares() {
        assert_eq!(squares_of(BoardShape::Banqi4x8).count(), 32);
    }
}
