//! Display orientation. Engine coords are physical (Red on rank 0); the
//! renderer rotates / transposes per shape so each observer sees their own
//! pieces at the bottom of the screen, and banqi displays in traditional
//! 8×4 layout.
//!
//! Verbatim copy of `clients/chess-tui/src/orient.rs` — `backlog/promote-client-shared.md`
//! tracks promoting both copies into a shared crate once a third client appears
//! or the copies first diverge. CI catches drift because the round-trip tests
//! are duplicated here too.

use chess_core::board::BoardShape;
use chess_core::coord::Square;
use chess_core::piece::Side;

/// Dimensions of the display grid in (rows, cols). Banqi transposes; xiangqi
/// keeps model dimensions as-is.
pub fn display_dims(shape: BoardShape) -> (u8, u8) {
    let (w, h) = shape.dimensions();
    match shape {
        BoardShape::Banqi4x8 => (w, h), // 4 rows × 8 cols
        _ => (h, w),                    // 10 rows × 9 cols for xiangqi
    }
}

/// Square (model coord) → (display_row, display_col).
#[allow(dead_code)]
pub fn project_cell(sq: Square, observer: Side, shape: BoardShape) -> (u8, u8) {
    let (w, h) = shape.dimensions();
    let f = (sq.0 as u8) % w;
    let r = (sq.0 as u8) / w;
    match shape {
        BoardShape::Xiangqi9x10 => match observer {
            Side::RED => (h - 1 - r, f),   // rank 9 at top, rank 0 at bottom
            Side::BLACK => (r, w - 1 - f), // rank 0 at top, files reversed
            _ => (h - 1 - r, f),           // 三國暗棋 fallback
        },
        BoardShape::Banqi4x8 => (f, r), // transpose: rank → col, file → row
        BoardShape::ThreeKingdom => (h - 1 - r, f),
        BoardShape::Custom { .. } => (h - 1 - r, f),
    }
}

/// Display (row, col) → Square. Returns `None` when out of bounds or the
/// shape masks that cell off (Custom / ThreeKingdom).
pub fn square_at_display(row: u8, col: u8, observer: Side, shape: BoardShape) -> Option<Square> {
    let (w, h) = shape.dimensions();
    let (dr, dc) = display_dims(shape);
    if row >= dr || col >= dc {
        return None;
    }
    let (f, r) = match shape {
        BoardShape::Xiangqi9x10 => match observer {
            Side::RED => (col, h - 1 - row),
            Side::BLACK => (w - 1 - col, row),
            _ => (col, h - 1 - row),
        },
        BoardShape::Banqi4x8 => (row, col),
        BoardShape::ThreeKingdom => (col, h - 1 - row),
        BoardShape::Custom { .. } => (col, h - 1 - row),
    };
    if f >= w || r >= h {
        return None;
    }
    let idx = (r as u16) * (w as u16) + (f as u16);
    if !shape.is_playable(idx as usize) {
        return None;
    }
    Some(Square(idx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_squares(shape: BoardShape) -> impl Iterator<Item = Square> {
        let n = shape.cell_count() as u16;
        (0..n).map(Square).filter(move |s| shape.is_playable(s.0 as usize))
    }

    #[test]
    fn xiangqi_red_round_trip() {
        let shape = BoardShape::Xiangqi9x10;
        for sq in all_squares(shape) {
            let (r, c) = project_cell(sq, Side::RED, shape);
            assert_eq!(square_at_display(r, c, Side::RED, shape), Some(sq));
        }
    }

    #[test]
    fn xiangqi_black_round_trip() {
        let shape = BoardShape::Xiangqi9x10;
        for sq in all_squares(shape) {
            let (r, c) = project_cell(sq, Side::BLACK, shape);
            assert_eq!(square_at_display(r, c, Side::BLACK, shape), Some(sq));
        }
    }

    #[test]
    fn xiangqi_red_and_black_are_180_rotations() {
        let shape = BoardShape::Xiangqi9x10;
        let (dr, dc) = display_dims(shape);
        for sq in all_squares(shape) {
            let (rr, rc) = project_cell(sq, Side::RED, shape);
            let (br, bc) = project_cell(sq, Side::BLACK, shape);
            assert_eq!(rr + br, dr - 1);
            assert_eq!(rc + bc, dc - 1);
        }
    }

    #[test]
    fn xiangqi_red_corner_layout() {
        let shape = BoardShape::Xiangqi9x10;
        let sq = Square(0);
        let (r, c) = project_cell(sq, Side::RED, shape);
        assert_eq!((r, c), (9, 0));

        let sq = Square(9 * 9);
        let (r, c) = project_cell(sq, Side::RED, shape);
        assert_eq!((r, c), (0, 0));
    }

    #[test]
    fn banqi_round_trip_and_dims() {
        let shape = BoardShape::Banqi4x8;
        assert_eq!(display_dims(shape), (4, 8));
        for sq in all_squares(shape) {
            let (r, c) = project_cell(sq, Side::RED, shape);
            assert!(r < 4 && c < 8, "banqi cell out of display: ({}, {})", r, c);
            assert_eq!(square_at_display(r, c, Side::RED, shape), Some(sq));
        }
    }

    #[test]
    fn out_of_bounds_returns_none() {
        let shape = BoardShape::Xiangqi9x10;
        assert!(square_at_display(10, 0, Side::RED, shape).is_none());
        assert!(square_at_display(0, 9, Side::RED, shape).is_none());
    }
}
