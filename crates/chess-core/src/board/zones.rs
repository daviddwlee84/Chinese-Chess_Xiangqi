//! Region predicates: palace, river, half-board, etc.
//!
//! Variant-specific. Functions here read from a `Board` and return booleans;
//! callers don't need to know how each shape encodes its zones.

use super::shape::BoardShape;
use crate::coord::Square;
use crate::piece::Side;

/// Named regions across all variants. Not every region applies to every shape;
/// `in_region` returns false for non-applicable combinations.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum RegionKind {
    /// 九宮 — palace for the given side.
    Palace(Side),
    /// 河界 — the river (xiangqi only).
    River,
    /// The half of the board nearest the given side's home rank.
    HomeHalf(Side),
}

/// Whether `sq` is in the named region on a board of `shape`.
pub fn in_region(shape: BoardShape, sq: Square, region: RegionKind) -> bool {
    let (w, h) = shape.dimensions();
    if !shape.is_playable(sq.raw() as usize) {
        return false;
    }
    let (file, rank) = (sq.raw() as u8 % w, sq.raw() as u8 / w);

    match (shape, region) {
        // ---- Xiangqi: palaces are 3 files (3..6) × 3 ranks per side ----
        (BoardShape::Xiangqi9x10, RegionKind::Palace(side)) => {
            let in_files = (3..=5).contains(&file);
            let in_ranks = match side {
                s if s == Side::RED => (0..=2).contains(&rank),
                s if s == Side::BLACK => (7..=9).contains(&rank),
                _ => false,
            };
            in_files && in_ranks
        }
        (BoardShape::Xiangqi9x10, RegionKind::River) => rank == 4 || rank == 5,
        (BoardShape::Xiangqi9x10, RegionKind::HomeHalf(side)) => match side {
            s if s == Side::RED => rank <= 4,
            s if s == Side::BLACK => rank >= 5,
            _ => false,
        },

        // ---- Banqi: no palace, no river. HomeHalf splits the long axis. ----
        (BoardShape::Banqi4x8, RegionKind::HomeHalf(side)) => match side {
            s if s == Side::RED => rank < h / 2,
            s if s == Side::BLACK => rank >= h / 2,
            _ => false,
        },

        // ThreeKingdom and Custom: caller provides their own zone logic.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sq(shape: BoardShape, file: u8, rank: u8) -> Square {
        let (w, _) = shape.dimensions();
        Square(rank as u16 * w as u16 + file as u16)
    }

    #[test]
    fn xiangqi_red_palace_corners() {
        let shape = BoardShape::Xiangqi9x10;
        // Red palace: files 3-5, ranks 0-2
        assert!(in_region(shape, sq(shape, 3, 0), RegionKind::Palace(Side::RED)));
        assert!(in_region(shape, sq(shape, 5, 2), RegionKind::Palace(Side::RED)));
        // Just outside
        assert!(!in_region(shape, sq(shape, 2, 0), RegionKind::Palace(Side::RED)));
        assert!(!in_region(shape, sq(shape, 3, 3), RegionKind::Palace(Side::RED)));
        // Black palace shouldn't claim red squares
        assert!(!in_region(shape, sq(shape, 4, 1), RegionKind::Palace(Side::BLACK)));
    }

    #[test]
    fn xiangqi_river_is_two_middle_ranks() {
        let shape = BoardShape::Xiangqi9x10;
        for f in 0..9u8 {
            assert!(in_region(shape, sq(shape, f, 4), RegionKind::River));
            assert!(in_region(shape, sq(shape, f, 5), RegionKind::River));
            assert!(!in_region(shape, sq(shape, f, 3), RegionKind::River));
            assert!(!in_region(shape, sq(shape, f, 6), RegionKind::River));
        }
    }

    #[test]
    fn banqi_home_halves_partition_the_board() {
        let shape = BoardShape::Banqi4x8;
        for r in 0..8u8 {
            for f in 0..4u8 {
                let s = sq(shape, f, r);
                let red = in_region(shape, s, RegionKind::HomeHalf(Side::RED));
                let black = in_region(shape, s, RegionKind::HomeHalf(Side::BLACK));
                assert!(red ^ black, "rank {r} file {f} should be in exactly one half");
            }
        }
    }
}
