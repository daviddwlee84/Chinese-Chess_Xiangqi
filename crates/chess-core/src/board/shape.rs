//! Board shapes.
//!
//! `BoardShape` is the discriminator: pick one and the `Board` knows its
//! dimensions and (for irregular boards) which cells are off-limits.

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum BoardShape {
    /// Standard xiangqi: 9 files × 10 ranks. All cells valid.
    Xiangqi9x10,

    /// Banqi: 4 files × 8 ranks (half a xiangqi board, oriented sideways).
    Banqi4x8,

    /// 三國暗棋. Implementation lands in PR 2; the variant exists in PR 1
    /// so the type system is already wired for it.
    ThreeKingdom,

    /// Arbitrary custom-shape variants (escape hatch for future use).
    /// `mask` bit `i` set means cell `i` is playable. Up to 128 cells;
    /// widen if a future variant exceeds that.
    Custom { width: u8, height: u8, mask: u128 },
}

impl BoardShape {
    #[inline]
    pub const fn dimensions(self) -> (u8, u8) {
        match self {
            BoardShape::Xiangqi9x10 => (9, 10),
            BoardShape::Banqi4x8 => (4, 8),
            // PR-2 placeholder; pick a plausible bounding box for now.
            BoardShape::ThreeKingdom => (4, 8),
            BoardShape::Custom { width, height, .. } => (width, height),
        }
    }

    #[inline]
    pub const fn cell_count(self) -> usize {
        let (w, h) = self.dimensions();
        (w as usize) * (h as usize)
    }

    /// Whether the cell at the given linear index is a playable square.
    /// Rectangular shapes accept any in-range index; irregular shapes
    /// consult their mask.
    #[inline]
    pub const fn is_playable(self, idx: usize) -> bool {
        match self {
            BoardShape::Xiangqi9x10 | BoardShape::Banqi4x8 => idx < self.cell_count(),
            BoardShape::ThreeKingdom => false, // PR-2: replace with proper mask
            BoardShape::Custom { mask, .. } => idx < 128 && (mask >> idx) & 1 == 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xiangqi_dimensions() {
        assert_eq!(BoardShape::Xiangqi9x10.dimensions(), (9, 10));
        assert_eq!(BoardShape::Xiangqi9x10.cell_count(), 90);
    }

    #[test]
    fn banqi_dimensions() {
        assert_eq!(BoardShape::Banqi4x8.dimensions(), (4, 8));
        assert_eq!(BoardShape::Banqi4x8.cell_count(), 32);
    }

    #[test]
    fn rectangular_all_cells_playable() {
        for i in 0..90 {
            assert!(BoardShape::Xiangqi9x10.is_playable(i));
        }
        assert!(!BoardShape::Xiangqi9x10.is_playable(90));
    }

    #[test]
    fn custom_mask_respected() {
        let shape = BoardShape::Custom { width: 3, height: 3, mask: 0b101_010_101 };
        assert!(shape.is_playable(0));
        assert!(!shape.is_playable(1));
        assert!(shape.is_playable(2));
        assert!(!shape.is_playable(3));
        assert!(shape.is_playable(4));
    }
}
