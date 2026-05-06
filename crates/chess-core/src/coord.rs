//! Coordinate types.
//!
//! `Square` is a linear index into a board's cell array. Boards know their own
//! shape and convert between `Square` and `(File, Rank)` as needed. See
//! `docs/adr/0002-square-as-u16.md` for why this isn't a `(file, rank)` tuple.

use serde::{Deserialize, Serialize};

/// Linear square index. Interpreted by a `Board` via its `BoardShape`.
///
/// `u16` accommodates up to 65535 cells — comfortably more than any
/// xiangqi-family board (largest planned: 19×19 = 361).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[repr(transparent)]
pub struct Square(pub u16);

impl Square {
    /// Sentinel for "no square" without an `Option<Square>`.
    pub const NONE: Square = Square(u16::MAX);

    #[inline]
    pub const fn new(idx: u16) -> Self {
        Self(idx)
    }

    #[inline]
    pub const fn raw(self) -> u16 {
        self.0
    }

    #[inline]
    pub const fn is_none(self) -> bool {
        self.0 == u16::MAX
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[repr(transparent)]
pub struct File(pub u8);

#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[repr(transparent)]
pub struct Rank(pub u8);

/// Eight compass directions.
///
/// `ORTHOGONAL` and `DIAGONAL` constants make iteration ergonomic.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum Direction {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl Direction {
    pub const ORTHOGONAL: [Direction; 4] = [Direction::N, Direction::S, Direction::E, Direction::W];

    pub const DIAGONAL: [Direction; 4] =
        [Direction::NE, Direction::NW, Direction::SE, Direction::SW];

    pub const ALL: [Direction; 8] = [
        Direction::N,
        Direction::S,
        Direction::E,
        Direction::W,
        Direction::NE,
        Direction::NW,
        Direction::SE,
        Direction::SW,
    ];

    /// `(dfile, drank)` deltas. North is +rank, East is +file.
    #[inline]
    pub const fn delta(self) -> (i8, i8) {
        match self {
            Direction::N => (0, 1),
            Direction::S => (0, -1),
            Direction::E => (1, 0),
            Direction::W => (-1, 0),
            Direction::NE => (1, 1),
            Direction::NW => (-1, 1),
            Direction::SE => (1, -1),
            Direction::SW => (-1, -1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_none_sentinel_distinct() {
        assert!(Square::NONE.is_none());
        assert!(!Square::new(0).is_none());
        assert!(!Square::new(89).is_none());
    }

    #[test]
    fn direction_constants_complete() {
        let mut all: Vec<_> =
            Direction::ORTHOGONAL.iter().chain(Direction::DIAGONAL.iter()).collect();
        all.sort_by_key(|d| format!("{:?}", d));
        let mut expected: Vec<_> = Direction::ALL.iter().collect();
        expected.sort_by_key(|d| format!("{:?}", d));
        assert_eq!(all, expected);
    }

    #[test]
    fn direction_deltas_consistent() {
        // North/South are inverses; East/West are inverses; etc.
        assert_eq!(Direction::N.delta(), (0, 1));
        assert_eq!(Direction::S.delta(), (0, -1));
        assert_eq!(Direction::E.delta(), (1, 0));
        assert_eq!(Direction::W.delta(), (-1, 0));
        assert_eq!(Direction::NE.delta(), (1, 1));
        assert_eq!(Direction::SW.delta(), (-1, -1));
    }
}
