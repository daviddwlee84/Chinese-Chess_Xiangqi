//! Turn order: round-robin over 2 or 3 seats.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::piece::Side;

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct TurnOrder {
    pub seats: SmallVec<[Side; 3]>,
    /// Index into `seats`.
    pub current: u8,
}

impl TurnOrder {
    pub fn two_player() -> Self {
        let mut seats = SmallVec::new();
        seats.push(Side::RED);
        seats.push(Side::BLACK);
        Self { seats, current: 0 }
    }

    pub fn three_player() -> Self {
        let mut seats = SmallVec::new();
        seats.push(Side(0));
        seats.push(Side(1));
        seats.push(Side(2));
        Self { seats, current: 0 }
    }

    #[inline]
    pub fn current_side(&self) -> Side {
        self.seats[self.current as usize]
    }

    pub fn advance(&mut self) {
        self.current = (self.current + 1) % self.seats.len() as u8;
    }

    /// Advance to the next non-eliminated seat. Used when a faction is wiped
    /// out in 三國暗棋. If all opponents are eliminated this is a no-op.
    pub fn advance_skipping(&mut self, eliminated: &[Side]) {
        for _ in 0..self.seats.len() {
            self.advance();
            if !eliminated.contains(&self.current_side()) {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_player_alternates() {
        let mut t = TurnOrder::two_player();
        assert_eq!(t.current_side(), Side::RED);
        t.advance();
        assert_eq!(t.current_side(), Side::BLACK);
        t.advance();
        assert_eq!(t.current_side(), Side::RED);
    }

    #[test]
    fn three_player_cycles() {
        let mut t = TurnOrder::three_player();
        let order: Vec<_> = (0..6)
            .map(|_| {
                let s = t.current_side();
                t.advance();
                s
            })
            .collect();
        assert_eq!(order, vec![Side(0), Side(1), Side(2), Side(0), Side(1), Side(2)]);
    }

    #[test]
    fn advance_skipping_skips_eliminated() {
        let mut t = TurnOrder::three_player();
        // Side 0 just moved; eliminated = [Side(1)] means skip to Side(2)
        t.advance_skipping(&[Side(1)]);
        assert_eq!(t.current_side(), Side(2));
        // Continue: skip Side(1) again
        t.advance_skipping(&[Side(1)]);
        assert_eq!(t.current_side(), Side(0));
    }
}
