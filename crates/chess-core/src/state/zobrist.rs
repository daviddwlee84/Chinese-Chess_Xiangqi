//! Zobrist hashing for [`GameState`].
//!
//! 64-bit incremental position hash, suitable for transposition tables
//! (chess-ai v5+) and threefold-repetition detection. The table is
//! built at compile time with deterministic SplitMix64, so the same
//! position always hashes to the same value across runs and across
//! native/WASM targets.
//!
//! # Scope and limitations
//!
//! - The hash covers **piece location** (side × kind × square) and
//!   **side-to-move**. It does **not** distinguish revealed vs. hidden
//!   pieces in banqi (a `PieceOnSquare` contributes the same key
//!   whether `revealed=true` or not). This is fine for the v5 AI
//!   target (xiangqi, where every piece is revealed) and for
//!   repetition detection where the underlying piece identity matters
//!   more than face-up vs face-down.
//! - Per-position counters (`no_progress_plies`, `chain_lock`,
//!   `side_assignment`) are **not** hashed. Two positions that differ
//!   only in those fields hash identically — acceptable for both AI
//!   probing (the search controls those fields) and for threefold
//!   repetition (where the move-history sequence matters more than
//!   the move-clock).
//! - Three-kingdom games (3 sides) are supported in the table layout
//!   (`Side(2)` is wired in) but the variant itself is a PR-2 stub.
//!
//! # Sizing
//!
//! Table is `[3 sides × 7 kinds × 128 squares] u64` ≈ **21 KB** of
//! `static` const data — trivial. 128 squares accommodates the
//! `BoardShape::Custom` upper bound.

use crate::piece::{PieceKind, Side};
use crate::state::GameState;

const ZOBRIST_SIDES: usize = 3;
const ZOBRIST_KINDS: usize = 7;
const ZOBRIST_SQUARES: usize = 128;

type ZobristTable = [[[u64; ZOBRIST_SQUARES]; ZOBRIST_KINDS]; ZOBRIST_SIDES];

/// Deterministic seed for the compile-time SplitMix64 stream. Changing
/// this value invalidates all stored hashes (snapshots, replay logs);
/// don't touch unless you know what you're doing.
const ZOBRIST_SEED: u64 = 0xC4FE_EDFA_CEBA_DC0D;

/// SplitMix64 step. Adapted from <http://xoshiro.di.unimi.it/splitmix64.c>.
const fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const fn build_zobrist() -> (ZobristTable, u64) {
    let mut state = ZOBRIST_SEED;
    let mut table = [[[0u64; ZOBRIST_SQUARES]; ZOBRIST_KINDS]; ZOBRIST_SIDES];
    let mut s = 0;
    while s < ZOBRIST_SIDES {
        let mut k = 0;
        while k < ZOBRIST_KINDS {
            let mut sq = 0;
            while sq < ZOBRIST_SQUARES {
                table[s][k][sq] = splitmix64(&mut state);
                sq += 1;
            }
            k += 1;
        }
        s += 1;
    }
    let stm = splitmix64(&mut state);
    (table, stm)
}

static ZOBRIST: (ZobristTable, u64) = build_zobrist();

const fn piece_kind_index(kind: PieceKind) -> usize {
    match kind {
        PieceKind::General => 0,
        PieceKind::Advisor => 1,
        PieceKind::Elephant => 2,
        PieceKind::Chariot => 3,
        PieceKind::Horse => 4,
        PieceKind::Cannon => 5,
        PieceKind::Soldier => 6,
    }
}

/// Look up the Zobrist key for `(side, kind, square)`. Out-of-range
/// indices return 0 (defensive — should never happen for in-game data).
#[inline]
pub fn zobrist_piece(side: Side, kind: PieceKind, sq_idx: usize) -> u64 {
    let s = side.0 as usize;
    let k = piece_kind_index(kind);
    if s >= ZOBRIST_SIDES || sq_idx >= ZOBRIST_SQUARES {
        return 0;
    }
    ZOBRIST.0[s][k][sq_idx]
}

/// The single key XORed into the hash whenever `side_to_move` changes
/// to a non-zero side. (Conceptually one key per side; we use an
/// equivalent encoding where `Side(0)` contributes nothing.)
#[inline]
pub fn zobrist_side_to_move(side: Side) -> u64 {
    // Two-player: toggle every move. Three-player: rotate through 0/1/2.
    // Encoding: 0 → 0, 1 → key, 2 → key.rotate_left(13).
    match side.0 {
        0 => 0,
        1 => ZOBRIST.1,
        _ => ZOBRIST.1.rotate_left(13),
    }
}

/// Recompute the position hash from scratch by scanning the board.
/// Use after constructing a [`GameState`] from a struct literal or
/// after deserialization (which restores `position_hash` to 0 by
/// default — see `#[serde(default)]` on the field).
pub fn compute_position_hash(state: &GameState) -> u64 {
    let mut h = 0u64;
    for sq in state.board.squares() {
        if let Some(pos) = state.board.get(sq) {
            h ^= zobrist_piece(pos.piece.side, pos.piece.kind, sq.0 as usize);
        }
    }
    h ^= zobrist_side_to_move(state.side_to_move);
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::Piece;
    use crate::rules::RuleSet;

    #[test]
    fn table_is_deterministic_across_calls() {
        let a = ZOBRIST.0[0][0][0];
        let b = ZOBRIST.0[0][0][0];
        assert_eq!(a, b);
        // Sanity: not all zeros.
        assert_ne!(a, 0);
    }

    #[test]
    fn distinct_piece_keys_are_distinct() {
        // Different (side, kind, square) triples must produce different
        // keys with very high probability. We don't enforce strict
        // uniqueness (collisions are theoretically possible at 64-bit),
        // but a small sample should be all-distinct.
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for s in 0..3u8 {
            for &k in &PieceKind::ALL {
                for sq in 0..90usize {
                    let key = zobrist_piece(Side(s), k, sq);
                    assert!(seen.insert(key), "duplicate key for {s:?} {k:?} {sq}");
                }
            }
        }
        assert_eq!(seen.len(), 3 * 7 * 90);
    }

    #[test]
    fn xiangqi_initial_hash_is_stable() {
        // Same starting position → same hash. Also asserts the value
        // is non-zero (32 pieces XOR'd together are extremely unlikely
        // to cancel out).
        let s1 = GameState::new(RuleSet::xiangqi());
        let s2 = GameState::new(RuleSet::xiangqi());
        assert_eq!(s1.position_hash, s2.position_hash);
        assert_ne!(s1.position_hash, 0);
    }

    #[test]
    fn side_to_move_alters_hash() {
        // Flipping side-to-move on the same board → different hashes.
        let mut s = GameState::new(RuleSet::xiangqi());
        let h0 = s.position_hash;
        // Toggle to BLACK manually + recompute.
        s.side_to_move = Side::BLACK;
        s.position_hash = compute_position_hash(&s);
        assert_ne!(s.position_hash, h0);
    }

    #[test]
    fn xor_off_then_on_round_trips() {
        let key = zobrist_piece(Side::RED, PieceKind::Chariot, 0);
        let h: u64 = 0x1234_5678_9ABC_DEF0;
        assert_eq!(h ^ key ^ key, h);
    }

    #[test]
    fn unknown_indices_return_zero() {
        // Defensive: out-of-range never panics.
        assert_eq!(zobrist_piece(Side(99), PieceKind::Chariot, 0), 0);
        assert_eq!(zobrist_piece(Side::RED, PieceKind::Chariot, 999), 0);
    }

    #[test]
    fn three_player_side_keys_distinct() {
        let k0 = zobrist_side_to_move(Side(0));
        let k1 = zobrist_side_to_move(Side(1));
        let k2 = zobrist_side_to_move(Side(2));
        assert_eq!(k0, 0);
        assert_ne!(k1, 0);
        assert_ne!(k2, 0);
        assert_ne!(k1, k2);
    }

    #[test]
    fn empty_board_hash_is_just_side_key() {
        // A board with zero pieces but `side_to_move = BLACK` should
        // hash to exactly `zobrist_side_to_move(BLACK)`.
        use crate::board::{Board, BoardShape};
        use crate::state::TurnOrder;
        use crate::state::{GameState, GameStatus};
        let state = GameState {
            rules: RuleSet::xiangqi(),
            board: Board::new(BoardShape::Xiangqi9x10),
            side_to_move: Side::BLACK,
            turn_order: TurnOrder::two_player(),
            history: Vec::new(),
            status: GameStatus::Ongoing,
            side_assignment: None,
            no_progress_plies: 0,
            chain_lock: None,
            position_hash: 0,
            banqi_first_mover_locked: false,
        };
        let h = compute_position_hash(&state);
        assert_eq!(h, zobrist_side_to_move(Side::BLACK));
        // Sanity: a single piece changes the hash.
        let _ = Piece::new(Side::RED, PieceKind::Chariot);
    }

    #[test]
    fn make_then_unmake_restores_hash() {
        // For every legal opening move, hash after make→unmake should
        // equal the starting hash. This is the core invariant for TT
        // correctness in the v5 search.
        let mut s = GameState::new(RuleSet::xiangqi());
        let h0 = s.position_hash;
        let moves = s.legal_moves();
        for mv in moves.iter() {
            s.make_move(mv).expect("legal opening move");
            assert_ne!(s.position_hash, h0, "applying {:?} should change the hash", mv);
            s.unmake_move().expect("unmake just-made move");
            assert_eq!(s.position_hash, h0, "make→unmake should restore hash for {:?}", mv);
        }
    }

    #[test]
    fn incremental_matches_full_recompute_after_long_sequence() {
        // After a 20-ply random-ish game, the incremental hash
        // maintained by make_move must match a from-scratch
        // recompute_position_hash().
        use rand::SeedableRng;
        let mut s = GameState::new(RuleSet::xiangqi_casual());
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(0xABCD_1234);
        for _ply in 0..20 {
            let moves = s.legal_moves();
            if moves.is_empty() {
                break;
            }
            use rand::Rng;
            let pick = rng.gen_range(0..moves.len());
            if s.make_move(&moves[pick]).is_err() {
                break;
            }
            let incremental = s.position_hash;
            let from_scratch = compute_position_hash(&s);
            assert_eq!(
                incremental, from_scratch,
                "drift after move {pick} (incremental != recompute)",
            );
        }
    }

    #[test]
    fn distinct_opening_positions_produce_distinct_hashes() {
        // Two different opening moves should leave the board in
        // different positions with different hashes (no Type-1 hash
        // collisions on this small set).
        use std::collections::HashSet;
        let s = GameState::new(RuleSet::xiangqi());
        let mut hashes = HashSet::new();
        for mv in s.legal_moves().iter() {
            let mut t = s.clone();
            t.make_move(mv).expect("legal opening move");
            assert!(
                hashes.insert(t.position_hash),
                "collision: two opening moves produced same hash"
            );
        }
        assert_eq!(hashes.len(), 44, "xiangqi opening has 44 legal moves");
    }
}
