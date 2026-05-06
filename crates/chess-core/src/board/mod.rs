//! Board: dense storage + shape-aware coordinate operations.

pub mod grid;
pub mod shape;
pub mod zones;

pub use shape::BoardShape;
pub use zones::RegionKind;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::coord::{Direction, File, Rank, Square};
use crate::piece::PieceOnSquare;

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct Board {
    shape: BoardShape,
    width: u8,
    height: u8,
    cells: Vec<Option<PieceOnSquare>>,
}

impl Board {
    pub fn new(shape: BoardShape) -> Self {
        let (width, height) = shape.dimensions();
        Self { shape, width, height, cells: vec![None; shape.cell_count()] }
    }

    #[inline]
    pub fn shape(&self) -> BoardShape {
        self.shape
    }

    #[inline]
    pub fn width(&self) -> u8 {
        self.width
    }

    #[inline]
    pub fn height(&self) -> u8 {
        self.height
    }

    #[inline]
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Build a `Square` from `(file, rank)` on this board.
    #[inline]
    pub fn sq(&self, file: File, rank: Rank) -> Square {
        Square((rank.0 as u16) * (self.width as u16) + file.0 as u16)
    }

    /// Decompose a `Square` into `(file, rank)`.
    #[inline]
    pub fn file_rank(&self, sq: Square) -> (File, Rank) {
        let w = self.width as u16;
        (File((sq.0 % w) as u8), Rank((sq.0 / w) as u8))
    }

    pub fn get(&self, sq: Square) -> Option<PieceOnSquare> {
        self.cells.get(sq.0 as usize).copied().flatten()
    }

    pub fn set(&mut self, sq: Square, value: Option<PieceOnSquare>) {
        self.cells[sq.0 as usize] = value;
    }

    /// Iterate every playable square in linear order.
    pub fn squares(&self) -> impl Iterator<Item = Square> + '_ {
        grid::squares_of(self.shape)
    }

    /// One step in `dir` from `from`. Returns `None` if the destination
    /// would leave the board or land on a non-playable cell.
    pub fn step(&self, from: Square, dir: Direction) -> Option<Square> {
        let (df, dr) = dir.delta();
        let (file, rank) = self.file_rank(from);
        let nf = (file.0 as i16) + df as i16;
        let nr = (rank.0 as i16) + dr as i16;
        if nf < 0 || nr < 0 || nf >= self.width as i16 || nr >= self.height as i16 {
            return None;
        }
        let next = self.sq(File(nf as u8), Rank(nr as u8));
        if self.shape.is_playable(next.0 as usize) {
            Some(next)
        } else {
            None
        }
    }

    /// Cast a ray from `from` in `dir`. Returns the squares walked over
    /// (all empty) and, if the ray stopped on an occupied square, that
    /// square. Stops at the board edge with `None` blocker.
    pub fn ray(&self, from: Square, dir: Direction) -> (SmallVec<[Square; 16]>, Option<Square>) {
        let mut walked = SmallVec::new();
        let mut cursor = from;
        loop {
            match self.step(cursor, dir) {
                None => return (walked, None),
                Some(next) => {
                    if self.get(next).is_some() {
                        return (walked, Some(next));
                    }
                    walked.push(next);
                    cursor = next;
                }
            }
        }
    }

    /// Convenience predicate forwarding to [`zones::in_region`].
    #[inline]
    pub fn in_region(&self, sq: Square, region: RegionKind) -> bool {
        zones::in_region(self.shape, sq, region)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::{Piece, PieceKind, Side};

    #[test]
    fn xiangqi_round_trip_file_rank() {
        let b = Board::new(BoardShape::Xiangqi9x10);
        for f in 0..9 {
            for r in 0..10 {
                let s = b.sq(File(f), Rank(r));
                let (f2, r2) = b.file_rank(s);
                assert_eq!((f2.0, r2.0), (f, r));
            }
        }
    }

    #[test]
    fn step_off_board_is_none() {
        let b = Board::new(BoardShape::Xiangqi9x10);
        let corner = b.sq(File(0), Rank(0));
        assert!(b.step(corner, Direction::S).is_none());
        assert!(b.step(corner, Direction::W).is_none());
        assert!(b.step(corner, Direction::N).is_some());
    }

    #[test]
    fn ray_walks_until_blocker() {
        let mut b = Board::new(BoardShape::Xiangqi9x10);
        let from = b.sq(File(0), Rank(0));
        // Place blocker 3 ranks north
        let blocker = b.sq(File(0), Rank(3));
        b.set(blocker, Some(PieceOnSquare::revealed(Piece::new(Side::BLACK, PieceKind::Soldier))));
        let (walked, hit) = b.ray(from, Direction::N);
        assert_eq!(walked.len(), 2);
        assert_eq!(hit, Some(blocker));
    }

    #[test]
    fn ray_off_board_no_blocker() {
        let b = Board::new(BoardShape::Xiangqi9x10);
        let from = b.sq(File(0), Rank(7));
        let (walked, hit) = b.ray(from, Direction::N);
        assert_eq!(walked.len(), 2); // ranks 8, 9
        assert!(hit.is_none());
    }

    #[test]
    fn banqi_board_has_32_cells() {
        let b = Board::new(BoardShape::Banqi4x8);
        assert_eq!(b.cell_count(), 32);
        assert_eq!(b.squares().count(), 32);
    }
}
