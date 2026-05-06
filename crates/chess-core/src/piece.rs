//! Pieces and sides.

use serde::{Deserialize, Serialize};

/// Player side. Two-player games use 0 and 1; 三國暗棋 uses 0, 1, 2.
///
/// Wraps a small int rather than a fixed enum so 3-player isn't a special case.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[repr(transparent)]
pub struct Side(pub u8);

impl Side {
    pub const RED: Side = Side(0);
    pub const BLACK: Side = Side(1);
    /// Third faction in 三國暗棋. Maps to 蜀/吳/魏 externally as the variant chooses.
    pub const GREEN: Side = Side(2);

    #[inline]
    pub const fn raw(self) -> u8 {
        self.0
    }

    /// 2-player only: the opposing side.
    #[inline]
    pub const fn opposite(self) -> Side {
        Side(self.0 ^ 1)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub enum PieceKind {
    /// 將 / 帥
    General,
    /// 士 / 仕
    Advisor,
    /// 象 / 相
    Elephant,
    /// 車 / 俥
    Chariot,
    /// 馬 / 傌
    Horse,
    /// 炮 / 砲
    Cannon,
    /// 卒 / 兵
    Soldier,
}

impl PieceKind {
    /// Banqi capture rank. Higher beats lower (with the cannon and
    /// soldier-vs-general exceptions handled in the move generator,
    /// not by this number).
    #[inline]
    pub const fn banqi_rank(self) -> u8 {
        match self {
            PieceKind::General => 6,
            PieceKind::Advisor => 5,
            PieceKind::Elephant => 4,
            PieceKind::Chariot => 3,
            PieceKind::Horse => 2,
            PieceKind::Cannon => 1,
            PieceKind::Soldier => 0,
        }
    }

    /// Iteration helper.
    pub const ALL: [PieceKind; 7] = [
        PieceKind::General,
        PieceKind::Advisor,
        PieceKind::Elephant,
        PieceKind::Chariot,
        PieceKind::Horse,
        PieceKind::Cannon,
        PieceKind::Soldier,
    ];
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct Piece {
    pub side: Side,
    pub kind: PieceKind,
}

impl Piece {
    #[inline]
    pub const fn new(side: Side, kind: PieceKind) -> Self {
        Self { side, kind }
    }
}

/// What sits on a square. Banqi pieces start `revealed: false`;
/// xiangqi pieces are always `revealed: true`.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
pub struct PieceOnSquare {
    pub piece: Piece,
    pub revealed: bool,
}

impl PieceOnSquare {
    #[inline]
    pub const fn revealed(piece: Piece) -> Self {
        Self { piece, revealed: true }
    }

    #[inline]
    pub const fn hidden(piece: Piece) -> Self {
        Self { piece, revealed: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_opposite_is_involution() {
        assert_eq!(Side::RED.opposite(), Side::BLACK);
        assert_eq!(Side::BLACK.opposite(), Side::RED);
        assert_eq!(Side::RED.opposite().opposite(), Side::RED);
    }

    #[test]
    fn banqi_rank_strictly_decreasing_general_to_soldier() {
        // General > Advisor > Elephant > Chariot > Horse > Cannon > Soldier
        let order = [
            PieceKind::General,
            PieceKind::Advisor,
            PieceKind::Elephant,
            PieceKind::Chariot,
            PieceKind::Horse,
            PieceKind::Cannon,
            PieceKind::Soldier,
        ];
        for w in order.windows(2) {
            assert!(w[0].banqi_rank() > w[1].banqi_rank(), "{:?} should outrank {:?}", w[0], w[1],);
        }
    }

    #[test]
    fn piece_kind_all_covers_seven_distinct() {
        let mut sorted: Vec<_> = PieceKind::ALL.iter().collect();
        sorted.sort_by_key(|p| format!("{:?}", p));
        sorted.dedup();
        assert_eq!(sorted.len(), 7);
    }
}
