//! Initial position builders.
//!
//! Returns a fully-formed `GameState` for each variant. Banqi shuffling is
//! seeded via `ChaCha8Rng` so games are reproducible from `RuleSet::banqi_seed`.

use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use crate::board::{Board, BoardShape};
use crate::piece::{Piece, PieceKind, PieceOnSquare, Side};
use crate::rules::{RuleSet, Variant};
use crate::state::{GameState, GameStatus, TurnOrder};

pub fn build_initial_state(rules: RuleSet) -> GameState {
    match rules.variant {
        Variant::Xiangqi => build_xiangqi(rules),
        Variant::Banqi => build_banqi(rules),
        Variant::ThreeKingdomBanqi => build_three_kingdom_stub(rules),
    }
}

fn build_xiangqi(rules: RuleSet) -> GameState {
    let mut board = Board::new(BoardShape::Xiangqi9x10);
    place_xiangqi_initial(&mut board);
    let mut state = GameState {
        rules,
        board,
        side_to_move: Side::RED,
        turn_order: TurnOrder::two_player(),
        history: Vec::new(),
        status: GameStatus::Ongoing,
        side_assignment: None,
        no_progress_plies: 0,
        chain_lock: None,
        position_hash: 0,
    };
    state.recompute_position_hash();
    state
}

/// Place the 32 standard xiangqi pieces. Files indexed 0..9 left-to-right
/// from Red's view; ranks 0..9 bottom-to-top with Red on the bottom.
fn place_xiangqi_initial(board: &mut Board) {
    use crate::coord::{File, Rank};

    let mut back = |kind: PieceKind, side: Side, files: &[u8], rank: u8| {
        for &f in files {
            board.set(
                board.sq(File(f), Rank(rank)),
                Some(PieceOnSquare::revealed(Piece::new(side, kind))),
            );
        }
    };

    // Red back rank (rank 0)
    back(PieceKind::Chariot, Side::RED, &[0, 8], 0);
    back(PieceKind::Horse, Side::RED, &[1, 7], 0);
    back(PieceKind::Elephant, Side::RED, &[2, 6], 0);
    back(PieceKind::Advisor, Side::RED, &[3, 5], 0);
    back(PieceKind::General, Side::RED, &[4], 0);

    // Red cannons (rank 2)
    back(PieceKind::Cannon, Side::RED, &[1, 7], 2);

    // Red soldiers (rank 3)
    back(PieceKind::Soldier, Side::RED, &[0, 2, 4, 6, 8], 3);

    // Black mirrors at rank 9, 7, 6
    back(PieceKind::Chariot, Side::BLACK, &[0, 8], 9);
    back(PieceKind::Horse, Side::BLACK, &[1, 7], 9);
    back(PieceKind::Elephant, Side::BLACK, &[2, 6], 9);
    back(PieceKind::Advisor, Side::BLACK, &[3, 5], 9);
    back(PieceKind::General, Side::BLACK, &[4], 9);
    back(PieceKind::Cannon, Side::BLACK, &[1, 7], 7);
    back(PieceKind::Soldier, Side::BLACK, &[0, 2, 4, 6, 8], 6);
}

fn build_banqi(rules: RuleSet) -> GameState {
    let mut board = Board::new(BoardShape::Banqi4x8);
    let seed = rules.banqi_seed.unwrap_or_else(|| {
        // Nondeterministic in the absence of a seed.
        use rand::RngCore;
        rand::thread_rng().next_u64()
    });
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut pieces = banqi_piece_set();
    pieces.shuffle(&mut rng);
    for (i, p) in pieces.into_iter().enumerate() {
        board.set(crate::coord::Square(i as u16), Some(PieceOnSquare::hidden(p)));
    }
    let mut state = GameState {
        rules,
        board,
        side_to_move: Side::RED,
        turn_order: TurnOrder::two_player(),
        history: Vec::new(),
        status: GameStatus::Ongoing,
        side_assignment: None,
        no_progress_plies: 0,
        chain_lock: None,
        position_hash: 0,
    };
    state.recompute_position_hash();
    state
}

/// One xiangqi piece-set per side, all 32 pieces. Order doesn't matter
/// because the caller shuffles.
fn banqi_piece_set() -> Vec<Piece> {
    let mut v = Vec::with_capacity(32);
    for side in [Side::RED, Side::BLACK] {
        v.push(Piece::new(side, PieceKind::General));
        v.extend((0..2).map(|_| Piece::new(side, PieceKind::Advisor)));
        v.extend((0..2).map(|_| Piece::new(side, PieceKind::Elephant)));
        v.extend((0..2).map(|_| Piece::new(side, PieceKind::Chariot)));
        v.extend((0..2).map(|_| Piece::new(side, PieceKind::Horse)));
        v.extend((0..2).map(|_| Piece::new(side, PieceKind::Cannon)));
        v.extend((0..5).map(|_| Piece::new(side, PieceKind::Soldier)));
    }
    debug_assert_eq!(v.len(), 32);
    v
}

fn build_three_kingdom_stub(rules: RuleSet) -> GameState {
    // PR-2 placeholder. For now build an empty 4x8 board.
    let mut state = GameState {
        rules,
        board: Board::new(BoardShape::Banqi4x8),
        side_to_move: Side(0),
        turn_order: TurnOrder::three_player(),
        history: Vec::new(),
        status: GameStatus::Ongoing,
        side_assignment: None,
        no_progress_plies: 0,
        chain_lock: None,
        position_hash: 0,
    };
    state.recompute_position_hash();
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xiangqi_initial_has_32_pieces() {
        let s = GameState::new(RuleSet::xiangqi());
        let count = s.board.squares().filter(|sq| s.board.get(*sq).is_some()).count();
        assert_eq!(count, 32);
    }

    #[test]
    fn xiangqi_generals_in_palaces() {
        let s = GameState::new(RuleSet::xiangqi());
        use crate::coord::{File, Rank};
        let red_g = s.board.get(s.board.sq(File(4), Rank(0))).unwrap();
        let black_g = s.board.get(s.board.sq(File(4), Rank(9))).unwrap();
        assert_eq!(red_g.piece.kind, PieceKind::General);
        assert_eq!(red_g.piece.side, Side::RED);
        assert_eq!(black_g.piece.kind, PieceKind::General);
        assert_eq!(black_g.piece.side, Side::BLACK);
        assert!(red_g.revealed);
    }

    #[test]
    fn banqi_initial_all_hidden() {
        let s = GameState::new(crate::rules::RuleSet::banqi_with_seed(
            crate::rules::HouseRules::empty(),
            7,
        ));
        let mut count = 0;
        for sq in s.board.squares() {
            if let Some(p) = s.board.get(sq) {
                assert!(!p.revealed, "banqi piece should start face-down");
                count += 1;
            }
        }
        assert_eq!(count, 32);
    }

    #[test]
    fn banqi_seed_is_deterministic() {
        let r1 = crate::rules::RuleSet::banqi_with_seed(crate::rules::HouseRules::empty(), 42);
        let r2 = crate::rules::RuleSet::banqi_with_seed(crate::rules::HouseRules::empty(), 42);
        let s1 = GameState::new(r1);
        let s2 = GameState::new(r2);
        assert_eq!(s1.board, s2.board);
    }

    #[test]
    fn banqi_different_seeds_differ() {
        let r1 = crate::rules::RuleSet::banqi_with_seed(crate::rules::HouseRules::empty(), 1);
        let r2 = crate::rules::RuleSet::banqi_with_seed(crate::rules::HouseRules::empty(), 2);
        let s1 = GameState::new(r1);
        let s2 = GameState::new(r2);
        assert_ne!(s1.board, s2.board);
    }
}
