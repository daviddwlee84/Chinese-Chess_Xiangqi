//! Transposition table for the v5 search.
//!
//! Hash-keyed cache of search results. Each entry stores:
//!
//! - the **full** Zobrist key (`u64`) — verified on probe, so the
//!   small chance of a wrong-position hit is eliminated at the cost
//!   of one extra `u64` of memory per slot;
//! - the search **depth** at which the score was computed;
//! - the **score** (mate-distance-corrected, see `score_to_tt`);
//! - the [`Bound`] flag (Exact / Lower / Upper);
//! - the **best move** at this position, used by the parent search
//!   for move-ordering on its next visit.
//!
//! ## Replacement policy
//!
//! Always-replace. Simpler than depth-preferred or two-bucket schemes;
//! good enough for the depths v5 reaches (≤ 6 ply on the 250k-node
//! budget). Future v5.1 may switch to depth-preferred + age, see
//! `backlog/v5.1-search-refinements.md`.
//!
//! ## Sizing
//!
//! Default 2^17 = 131_072 slots. Each slot is `Option<TtEntry>`
//! (~40 B with `Move` ~24 B + 8 B key + 4 B score + 1 B depth + 1 B
//! bound + niche/padding) → ~5 MB. Safe on WASM heap (32 MB typical
//! mobile limit). Constant for all targets per design decision.

use chess_core::moves::Move;

/// Score-bound flag stored in each TT entry.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(u8)]
pub enum Bound {
    /// Exact score within the search window — `score` is precise.
    Exact,
    /// Fail-low (alpha) — actual score is at most `score`.
    /// Useful as an upper bound on parent's beta.
    Upper,
    /// Fail-high (beta) — actual score is at least `score`.
    /// Useful as a lower bound on parent's alpha.
    Lower,
}

#[derive(Clone, Debug)]
pub struct TtEntry {
    pub key: u64,
    pub depth: u8,
    pub score: i32,
    pub bound: Bound,
    pub best_move: Option<Move>,
}

/// Power-of-two-sized hash table. Address with `key & mask`.
pub struct TranspositionTable {
    entries: Vec<Option<TtEntry>>,
    mask: usize,
    pub probes: u32,
    pub hits: u32,
    pub stores: u32,
}

/// 2^17 = 131_072 slots — see module-level note for the sizing rationale.
pub const DEFAULT_TT_BITS: u8 = 17;

impl TranspositionTable {
    /// Allocate a table with `1 << bits` slots. `bits` is clamped to
    /// `[8, 24]` defensively (256 slots .. 16 M slots ≈ 600 MB).
    pub fn with_pow2_bits(bits: u8) -> Self {
        let bits = bits.clamp(8, 24);
        let len = 1usize << bits;
        Self { entries: vec![None; len], mask: len - 1, probes: 0, hits: 0, stores: 0 }
    }

    /// Convenience: default-sized table.
    pub fn new() -> Self {
        Self::with_pow2_bits(DEFAULT_TT_BITS)
    }

    #[inline]
    fn slot(&self, key: u64) -> usize {
        (key as usize) & self.mask
    }

    /// Look up an entry by full key. Returns `None` when the slot is
    /// empty or holds a different key (Type-1 collision).
    pub fn probe(&mut self, key: u64) -> Option<&TtEntry> {
        self.probes = self.probes.saturating_add(1);
        let idx = self.slot(key);
        match &self.entries[idx] {
            Some(e) if e.key == key => {
                self.hits = self.hits.saturating_add(1);
                Some(e)
            }
            _ => None,
        }
    }

    /// Store an entry, always replacing any previous occupant of the
    /// slot. Caller is responsible for mate-score adjustment via
    /// [`score_to_tt`].
    pub fn store(&mut self, entry: TtEntry) {
        self.stores = self.stores.saturating_add(1);
        let idx = self.slot(entry.key);
        self.entries[idx] = Some(entry);
    }

    /// Total number of slots (powers of two; `1 << bits`).
    pub fn capacity(&self) -> usize {
        self.entries.len()
    }
}

impl Default for TranspositionTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Adjust a mate score for storage. Mate scores are stored as
/// "distance from this node", so a mate-in-3 from ply 5 is stored as
/// `MATE - 3` (not `MATE - 8`). The converse [`score_from_tt`] re-adds
/// the current ply when retrieving, so a deeper TT hit produces the
/// correct mate distance.
///
/// `mate_threshold` is `MATE - max_search_ply`; everything above that
/// is treated as a mate score. Using a generous threshold (e.g.
/// `MATE - 1000`) makes the bookkeeping robust to deep search.
#[inline]
pub fn score_to_tt(score: i32, ply: i32, mate_threshold: i32) -> i32 {
    if score >= mate_threshold {
        score + ply
    } else if score <= -mate_threshold {
        score - ply
    } else {
        score
    }
}

#[inline]
pub fn score_from_tt(score: i32, ply: i32, mate_threshold: i32) -> i32 {
    if score >= mate_threshold {
        score - ply
    } else if score <= -mate_threshold {
        score + ply
    } else {
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess_core::coord::Square;

    fn dummy_move() -> Move {
        Move::Step { from: Square(0), to: Square(1) }
    }

    #[test]
    fn empty_probe_returns_none() {
        let mut tt = TranspositionTable::new();
        assert!(tt.probe(0xDEAD_BEEF).is_none());
    }

    #[test]
    fn store_then_probe_round_trips() {
        let mut tt = TranspositionTable::new();
        tt.store(TtEntry {
            key: 0xABCD,
            depth: 4,
            score: 123,
            bound: Bound::Exact,
            best_move: Some(dummy_move()),
        });
        let e = tt.probe(0xABCD).expect("should hit");
        assert_eq!(e.depth, 4);
        assert_eq!(e.score, 123);
        assert_eq!(e.bound, Bound::Exact);
        assert!(e.best_move.is_some());
    }

    #[test]
    fn always_replace_overwrites_slot() {
        let mut tt = TranspositionTable::with_pow2_bits(8);
        // Two keys colliding into the same slot (low 8 bits identical).
        let k1: u64 = 0x1234_5678_9ABC_DE12;
        let k2: u64 = 0xFFFF_FFFF_FFFF_FF12;
        tt.store(TtEntry { key: k1, depth: 1, score: 1, bound: Bound::Exact, best_move: None });
        assert!(tt.probe(k1).is_some());
        tt.store(TtEntry { key: k2, depth: 1, score: 2, bound: Bound::Exact, best_move: None });
        // k1 evicted; k2 hits.
        assert!(tt.probe(k1).is_none());
        assert!(tt.probe(k2).is_some());
    }

    #[test]
    fn collision_returns_none() {
        // Wrong key in the same slot returns None (no false positive).
        let mut tt = TranspositionTable::with_pow2_bits(8);
        tt.store(TtEntry { key: 0x100, depth: 1, score: 1, bound: Bound::Exact, best_move: None });
        // Same low byte (0x00) as 0x100; same slot.
        assert!(tt.probe(0x200).is_none());
    }

    #[test]
    fn mate_score_adjustment_round_trips() {
        const MATE: i32 = 1_000_000;
        const MATE_THRESHOLD: i32 = MATE - 1000;
        // Win-in-3 from ply 5: stored as +999_997 + 5 = +1_000_002,
        // retrieved at ply 5 as +1_000_002 - 5 = +999_997. ✅
        let stored = score_to_tt(MATE - 3, 5, MATE_THRESHOLD);
        let restored = score_from_tt(stored, 5, MATE_THRESHOLD);
        assert_eq!(restored, MATE - 3);
        // Loss-in-2 from ply 7.
        let stored = score_to_tt(-(MATE - 2), 7, MATE_THRESHOLD);
        let restored = score_from_tt(stored, 7, MATE_THRESHOLD);
        assert_eq!(restored, -(MATE - 2));
        // Non-mate scores unchanged.
        let stored = score_to_tt(42, 5, MATE_THRESHOLD);
        let restored = score_from_tt(stored, 5, MATE_THRESHOLD);
        assert_eq!(restored, 42);
        assert_eq!(stored, 42);
    }

    #[test]
    fn capacity_is_power_of_two() {
        assert_eq!(TranspositionTable::with_pow2_bits(10).capacity(), 1024);
        assert_eq!(TranspositionTable::with_pow2_bits(17).capacity(), 131_072);
    }

    #[test]
    fn bits_clamped_to_safe_range() {
        // Below floor → bumped to 256.
        assert_eq!(TranspositionTable::with_pow2_bits(0).capacity(), 256);
        // Above ceiling clamped to 24 bits → 16M slots.
        assert_eq!(TranspositionTable::with_pow2_bits(40).capacity(), 1 << 24);
    }
}
