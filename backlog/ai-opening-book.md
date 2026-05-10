# chess-ai opening book (P? — needs spike)

**Status**: P? / spike before commit
**Effort**: L (data collection is the bulk of the work)
**Related**: [`chess-ai-search.md`](chess-ai-search.md) (mentioned as
cross-cutting future work),
[`docs/ai/perf.md`](../docs/ai/perf.md) (current opening cost numbers
this would offset)

## Why this is a P?

A real opening book would meaningfully improve early-game play at
zero search cost — every difficulty from Easy upward would start with
"professional" first 5-10 plies. But several open questions block a
straight P2/P3 commit:

1. **Data source**. Is there an open-licensed xiangqi opening database
   we can ship? Public games (compatible licensing) → manual curation
   → small hand-built book are the three options, in increasing order
   of effort and decreasing order of quality.
2. **Variation handling**. Does the engine pick deterministically (one
   line per position) or weighted-randomly (matches the existing
   `Randomness` knob philosophy)? Latter is cooler but doubles the
   data size and complicates determinism for tests.
3. **Format / size**. WASM bundle size is already a perf concern (see
   `pitfalls/wasm-binary-size-budget.md` if it ever lands). 1k entries
   ≈ 16 KB compressed. 10k entries ≈ 160 KB. 100k entries ≈ 1.6 MB
   — questionable for the GitHub Pages standalone build.
4. **Maintenance**. Books bit-rot; new analysis can deprecate lines.
   Without a living curator, a static book gets stale.

The spike resolves these into a real priority + effort tag.

## What good looks like

`chess_ai::opening_book::Book` consulted **before** the search runs:

```rust
fn analyze(state: &GameState, opts: &AiOptions) -> Option<AiAnalysis> {
    if let Some(mv) = OPENING_BOOK.lookup(state.position_hash, opts.randomness) {
        return Some(AiAnalysis::from_book(mv));
    }
    // ... existing search ...
}
```

Hit characteristics:

- **First ~10 plies**: high hit rate (>80%) for both sides if the book
  covers main lines.
- **Beyond ~10 plies**: book exits, search takes over.
- **Out-of-book positions**: zero overhead (single Zobrist lookup).

The `position_hash` field landed in v5 (2026-05-10) is the natural
key — already deterministic, side-aware, and indexed by the engine.

## Spike scope

A single afternoon to answer:

1. Is there a public-domain or permissive-license xiangqi opening
   database in a parseable format? (Survey: chessdb.cn dumps, ICS
   archives, FairyStockfish opening tables.)
2. If yes, how many positions does it cover and how much would a
   compressed serialization weigh in the WASM bundle?
3. If no, how many hand-curated positions cover the "standard 20
   openings" mentioned in xiangqi club material? (Estimate: ~500-1500
   positions for reasonable coverage.)

Output: a follow-up issue or PR-1 stub with a real priority tag.

## Format proposal (subject to spike)

```rust
struct BookEntry {
    hash: u64,         // position_hash key
    moves: Vec<(Move, u16)>,  // candidate moves + weight (cp-rank or game count)
}

// Stored as bincode'd Vec<BookEntry>, sorted by hash for binary search.
// Loaded lazily on first `analyze()` call.
```

Lookup with `Randomness::STRICT` → top-weighted; with `SUBTLE/VARIED`
→ weighted random within a cp window (mirrors the existing search
randomness API exactly).

## Risks / unknowns

- **Banqi has no opening theory** (random initial flip). Book is
  xiangqi-only. Three-Kingdom is also out (too niche to find a book).
- **Engine-curated vs human-curated**. A self-play tournament could
  generate a "what v5 plays at depth 12 from each opening" book —
  cheap to produce but circular (the book would just reproduce v5's
  preferences). Real value is in human master games.
- **Test fragility**. Any test that asserts a specific AI move at
  ply 1-5 will break the moment the book ships, because the engine
  will play book moves instead of search moves. Audit tests beforehand.

## Test plan (post-spike)

- Unit: `book_lookup_returns_chosen_for_starting_position` — hardcode
  a known book entry, assert the lookup hits it.
- Integration: `analyze_consumes_book_for_first_five_plies` — start
  position, simulate 5 ply, assert book hit count == 5 (when book
  has full coverage).
- Off-book: `analyze_falls_through_to_search_for_unknown_position` —
  random scrambled position; assert no book hit, normal search runs.

## Tasks (post-spike, if greenlit)

1. Spike: source survey + format prototype (this doc's "Spike scope").
2. Define `BookEntry` and the `Book` struct.
3. Build the data file (one of: import script, hand-curated YAML,
   self-play generator).
4. Wire into `analyze()` with feature gate (`opening-book` Cargo
   feature so non-WASM consumers can opt out if size matters).
5. Doc: `docs/ai/opening-book.md` covering source, format, hit-rate
   benchmarks.

## When to revisit

After v5.1 + v6 (pondering) ship. By then the engine will be
near the local-search ceiling for opening play; the book provides
the next strength step without further search engineering.
