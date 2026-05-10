# v5 — iterative deepening + Zobrist transposition table

**Shipped 2026-05-10 · default since 2026-05-10**

v5 layers two classical search refinements on top of v4's
quiescence + MVV-LVA infrastructure:

1. **Iterative deepening (ID)**: instead of jumping straight to the
   target depth, search depths 1, 2, …, target in sequence. Each
   completed iteration seeds the next with move-ordering hints and
   value bounds, drastically reducing total node count for the same
   effective depth in repeating positions.
2. **Zobrist transposition table (TT)**: a hash-keyed cache of
   (depth, score, bound, best-move) records. Same position reached
   by two move orders → second visit returns the cached result
   without re-searching.

The evaluator (material + PSTs + king safety) is unchanged from v3/v4.

## What's new in code

- `crates/chess-core/src/state/zobrist.rs` — compile-time
  SplitMix64-seeded table, ~21 KB static. `GameState.position_hash`
  field, incrementally maintained by `make_move` / `unmake_move`.
- `crates/chess-ai/src/search/tt.rs` — `TranspositionTable` with
  always-replace policy, default 2^17 = 131 072 slots (~5 MB on
  WASM heap). Mate scores adjusted by ply on store/probe.
- `crates/chess-ai/src/search/v5.rs` — `negamax_v5` (TT probe +
  TT-best-move ordering bonus + bounds-aware store) and
  `score_root_moves_v5` (ID loop, returns last completed depth).
- `crates/chess-ai/src/engines/mod.rs::analyze_v5` —
  separate from v1-v4's `build_analysis` because v5 reports the
  *actually-reached* depth (which can be less than the requested
  target when the node budget hits mid-iteration).

## Trade-offs

| | v4 | v5 |
|---|---|---|
| **Endgame Hard (5 pieces)** | 65 k nodes / 34 ms | **35 k / 21 ms** |
| **Opening Hard** | 250 k nodes / 326 ms (truncated d4) | 250 k / 355 ms (full d3) |
| **Easy mode (depth 1)** | 0.86 ms | 3 ms (TT alloc overhead) |

In the **endgame**, TT hits dominate (many move orders converge to
the same position) — v5 wins decisively on both nodes and wall-clock.

In the **opening**, the branching factor is wide (~40 root moves,
~5–10 unique transpositions). v5 spends nodes on shallow iterations
(d1 ≈ 400, d2 ≈ 5 k, d3 ≈ 60 k) before attempting d4, and the
remaining ~185 k budget isn't enough to complete d4. v5 returns the
*fully-completed* depth-3 result instead of v4's *partial* depth-4.
For correctness this is a strict improvement (every root move
scored consistently), even though the displayed depth is lower.

In **Easy mode** (depth 1), the 5 MB TT allocation overhead dominates
the 0.86 ms search. Imperceptible to humans (~3 ms total) but worth
noting. Future v5.1 may pre-allocate the TT once and clear it
between searches.

## Why ID is necessary

Without ID, you'd search depth 4 directly, miss any deep blunders
where a shallow tactic refutes a strategic plan. With ID:

- Depth 1 finds "any free piece I can grab now"
- Depth 2 finds "captures that lose the captured piece next move"
- Depth 3 finds "exchange sequences" — TT hits explode here
- Depth 4 prunes huge subtrees thanks to TT hints

The result is a search that's **strictly stronger at the same
depth**, even when total node count is similar.

## Why TT must be in chess-core (not chess-ai)

The Zobrist hash needs incremental update on every `make_move` /
`unmake_move`. Recomputing from scratch (90-cell scan) per node
would dominate the search cost. Hooking into `apply_inner` /
`unapply_inner` keeps it O(1) per move.

This also unblocks the long-pending **threefold repetition**
detection (P1 in TODO.md): once `position_hash` is in `GameState`,
a `position_history` Vec → count map naturally fits.

## Limitations

- **TT is per-`analyze` call.** Searches don't share TT across calls
  (no "ponder" pondering opponent's move on AI's time). v5.x might
  add this; trade-off is memory residency between moves.
- **Always-replace** policy. Doesn't preserve high-depth entries
  when low-depth ones evict them. Good enough at v5 depths;
  depth-preferred + age replacement scheduled for v5.1.
- **No aspiration windows.** Root search uses full window
  `[-MATE-1, MATE+1]`. Aspiration windows are a follow-up
  optimization.
- **Banqi is still out of scope** — alpha-beta would peek at
  hidden tiles regardless of search refinements.

## Tests

- `crates/chess-core/src/state/zobrist.rs::tests` — 11 tests:
  table determinism, no Type-1 collisions on (side, kind, square),
  initial-hash stability, side-toggle alters hash, make→unmake
  round-trip across ALL 44 opening moves, 20-ply incremental
  matches full recompute, 44 distinct opening positions → 44
  distinct hashes.
- `crates/chess-ai/src/search/tt.rs::tests` — 7 tests: store/probe
  round-trip, always-replace overwrites slot, collision returns
  None, mate-score adjustment round-trips, capacity is power-of-two,
  bits clamped to safe range.
- `crates/chess-ai/src/search/v5.rs::tests` — 4 tests: opening
  returns scored moves, ID reaches target depth in reasonable
  positions, TT hits during ID (node-count smoke), determinism
  across calls.

All 30 chess-ai regression tests stay green (especially
`v4_defends_general_after_red_central_cannon_lands_at_e6` and
`analyze_chosen_move_has_pv_at_depth_2_and_above`).
