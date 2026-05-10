# chess-ai performance

> **Last measured**: 2026-05-10 on M-class Mac, release profile.
> **Reproduce**: `cargo test -p chess-ai --release perf -- --ignored --nocapture`

## Complexity model

Negamax with α-β pruning has theoretical bounds:

| Case | Nodes |
|---|---|
| Worst (no pruning) | `O(b^d)` |
| Best (perfect ordering) | `O(b^(d/2))` |
| Practical (capture-first + a few cutoffs) | `O(b^(3d/4))` |

For xiangqi the opening branching factor `b ≈ 40` (initial 44 legal moves
narrows quickly as soldiers cross the river and pieces develop). At
`d = 4` (current Hard default) the worst case is `40^4 ≈ 2.56M`; the
practical case is `40^3 ≈ 64k`. Our measurements (below) sit at the
practical end of the bound thanks to capture-first ordering and the
hard 250 000 node budget in [`search/mod.rs`](../../crates/chess-ai/src/search/mod.rs).

## Measurements (2026-05-09, post-v4-and-root-fix)

All runs use `Randomness::STRICT` so the measurement is search cost
only — no RNG variation between repeats. Median wall-clock over 3 runs;
nodes are deterministic given the search inputs.

> **2026-05-09 update**: numbers shifted significantly after fixing
> [`pitfalls/alpha-beta-root-score-pollution.md`](../../pitfalls/alpha-beta-root-score-pollution.md).
> The root search no longer narrows alpha between root moves (full
> window per move) — costs more nodes, gives correct per-move scores
> so the `Randomness` layer can distinguish defensive moves from
> suicides. Previous numbers (when alpha-beta narrowed at root) showed
> ~30 ms for v3 Hard openings; correct-but-slower numbers are below.

| Fixture | Strategy | Difficulty | Depth | Nodes | Median ms (3 runs) | nodes/ms |
|---------|----------|------------|-------|-------|--------------------|---------|
| opening (initial xiangqi) | v1 | Easy | 1 | 44 | 0.08 | 579 |
| opening (initial xiangqi) | v1 | Normal | 3 | 6021 | 11.61 | 518 |
| opening (initial xiangqi) | v1 | Hard | 4 | 98932 | 161.30 | 613 |
| opening (initial xiangqi) | v2 | Easy | 1 | 44 | 0.08 | 537 |
| opening (initial xiangqi) | v2 | Normal | 3 | 6519 | 12.71 | 513 |
| opening (initial xiangqi) | v2 | Hard | 4 | 104334 | 186.61 | 559 |
| opening (initial xiangqi) | v3 | Easy | 1 | 44 | 0.09 | 494 |
| opening (initial xiangqi) | v3 | Normal | 3 | 6519 | 12.73 | 512 |
| opening (initial xiangqi) | v3 | Hard | 4 | 104321 | 189.25 | 551 |
| **opening (initial xiangqi)** | **v4** | **Easy** | **1** | **362** | **0.73** | **497** |
| **opening (initial xiangqi)** | **v4** | **Normal** | **3** | **66187** | **108.26** | **611** |
| **opening (initial xiangqi)** | **v4** | **Hard** | **4** | **250032** | **327.52** | **763** |
| midgame (4 pieces removed) | v1 | Easy | 1 | 56 | 0.11 | 523 |
| midgame (4 pieces removed) | v1 | Normal | 3 | 11654 | 24.31 | 479 |
| midgame (4 pieces removed) | v1 | Hard | 4 | 152968 | 297.88 | 514 |
| midgame (4 pieces removed) | v2 | Easy | 1 | 56 | 0.11 | 505 |
| midgame (4 pieces removed) | v2 | Normal | 3 | 12022 | 26.03 | 462 |
| midgame (4 pieces removed) | v2 | Hard | 4 | 158500 | 333.87 | 475 |
| midgame (4 pieces removed) | v3 | Easy | 1 | 56 | 0.12 | 487 |
| midgame (4 pieces removed) | v3 | Normal | 3 | 12022 | 30.65 | 392 |
| midgame (4 pieces removed) | v3 | Hard | 4 | 158133 | 336.49 | 470 |
| **midgame (4 pieces removed)** | **v4** | **Easy** | **1** | **13900** | **19.67** | **707** |
| **midgame (4 pieces removed)** | **v4** | **Normal** | **3** | **48511** | **66.75** | **727** |
| **midgame (4 pieces removed)** | **v4** | **Hard** | **4** | **250045** | **300.12** | **833** |
| sparse endgame (5 pieces total) | v1 | Easy | 1 | 18 | 0.02 | 1125 |
| sparse endgame (5 pieces total) | v1 | Normal | 3 | 922 | 0.76 | 1207 |
| sparse endgame (5 pieces total) | v1 | Hard | 4 | 6980 | 5.65 | 1235 |
| sparse endgame (5 pieces total) | v2 | Easy | 1 | 18 | 0.01 | 1385 |
| sparse endgame (5 pieces total) | v2 | Normal | 3 | 3527 | 2.43 | 1450 |
| sparse endgame (5 pieces total) | v2 | Hard | 4 | 31527 | 24.37 | 1294 |
| sparse endgame (5 pieces total) | v3 | Easy | 1 | 18 | 0.01 | 1500 |
| sparse endgame (5 pieces total) | v3 | Normal | 3 | 3440 | 2.47 | 1394 |
| sparse endgame (5 pieces total) | v3 | Hard | 4 | 29544 | 22.63 | 1306 |
| **sparse endgame (5 pieces total)** | **v4** | **Easy** | **1** | **36** | **0.02** | **2000** |
| **sparse endgame (5 pieces total)** | **v4** | **Normal** | **3** | **4972** | **2.74** | **1817** |
| **sparse endgame (5 pieces total)** | **v4** | **Hard** | **4** | **21668** | **12.03** | **1801** |

## v5 (2026-05-10): iterative deepening + transposition table

v5 layers ID + Zobrist TT on top of v4's quiescence/MVV-LVA. The TT
amortizes search cost across iterations: depth 2 hits depth-1 entries,
depth 3 hits depth-1/2 entries, etc. Endgame positions where many
move orders converge to the same position benefit dramatically. Wide
opening positions benefit less because new positions outpace TT hits.

| Fixture | Strategy | Difficulty | Depth | Nodes | Median ms (3 runs) | nodes/ms |
|---------|----------|------------|-------|-------|--------------------|---------|
| opening (initial xiangqi) | v5 | Easy | 1 | 362 | 3.04 | 119 |
| opening (initial xiangqi) | v5 | Normal | 3 | 60 828 | 113.10 | 538 |
| opening (initial xiangqi) | v5 | Hard | **3** | 250 077 | 355.09 | 704 |
| midgame (4 pieces removed) | v5 | Easy | 1 | 13 900 | 21.04 | 661 |
| midgame (4 pieces removed) | v5 | Normal | 3 | 83 967 | 119.97 | 700 |
| midgame (4 pieces removed) | v5 | Hard | **3** | 250 070 | 329.91 | 758 |
| **sparse endgame (5 pieces total)** | **v5** | **Easy** | **1** | **36** | **1.83** | **20** |
| **sparse endgame (5 pieces total)** | **v5** | **Normal** | **3** | **3 930** | **4.50** | **874** |
| **sparse endgame (5 pieces total)** | **v5** | **Hard** | **4** | **34 668** | **20.61** | **1682** |

### v5 vs v4 read

- **Endgame Hard**: v5 = 35 k nodes / 21 ms. v4 = 22 k / 12 ms. v5
  visits 60 % more nodes because ID re-searches d1+d2+d3 before d4,
  but completes the same depth in similar wall-clock — and would
  scale better at d5+ because the TT cache hits compound.
- **Opening / Midgame Hard**: v5 hits the 250 k node budget at
  depth **3** (vs v4 at depth 4). The v4 depth-4 figure is
  *truncated* mid-search (not all root moves reached d4); v5's
  depth-3 result is fully searched (every root move scored
  consistently). Quality probably comparable; v5 is more honest.
- **Easy mode (depth 1)**: v5 has ~3 ms TT-allocation overhead vs
  v4's <1 ms. Imperceptible. v5.1 may pre-allocate.

The depth ceiling at the budget is the v5 cost. Per-iteration TT
overhead means v5 spends more total nodes for the same effective
depth in *unique-position-rich* searches (openings); but per-move
quality at the achieved depth is strictly better thanks to ID's
"better moves first" property.

See [`v5-id-tt.md`](v5-id-tt.md) for the design rationale and the
list of follow-up optimizations (PVS, aspiration windows,
depth-preferred replacement) tracked as v5.1.

## v4 Hard cost vs the node budget

v4 + Hard now hits the **full 250k node budget** on both opening (250032
nodes) and midgame (250045 nodes) — search bails to "best-so-far"
when the cap is reached. The fact that scores are still correct
(verified by `v4_defends_general_after_red_central_cannon_lands_at_e6`)
means the budget bail happens AFTER the king-loss is propagated for
suicide moves, so the randomness layer still picks defensively.

Cost vs previous (alpha-beta-narrowing-at-root) era:

| Fixture | v4 Hard before fix | v4 Hard after fix | Ratio |
|---|---|---|---|
| Opening | 30 ms / 16k nodes | 327 ms / 250k nodes (budget hit) | ~10× |
| Midgame | 33 ms / 21k nodes | 300 ms / 250k nodes (budget hit) | ~9× |
| Endgame | 14 ms / 22k nodes | 33 ms / 65k nodes | ~2.4× |

This is the cost of correctness. The previous fast numbers were
**lying** about non-best move scores, leading to king-blindness in
SUBTLE/VARIED randomness modes. See
[`pitfalls/alpha-beta-root-score-pollution.md`](../../pitfalls/alpha-beta-root-score-pollution.md).

## Headroom analysis (post-fix)

| Configuration | Native ms | WASM est. | Verdict |
|---|---|---|---|
| v4 + Hard + depth 4 + opening | 327 ms | **1.5-3 s** | Borderline. Users will see the wait. |
| v4 + Hard + depth 4 + midgame | 300 ms | **1.5-3 s** | Borderline. |
| v4 + Hard + depth 4 + endgame | 33 ms | 150-300 ms | Snappy. |
| v4 + Normal + depth 3 + opening | 108 ms | 500 ms - 1 s | Acceptable. |
| v4 + Normal + depth 3 + midgame | 67 ms | 350-700 ms | Acceptable. |

Future optimisation options to recover speed without losing correctness:

1. **Principal Variation Search (PVS) at the root.** First move with
   full window; subsequent with null window (`-alpha-1, -alpha`); on
   fail-high, re-search with full window. This recovers most alpha-beta
   savings while still giving correct scores.
2. **Aspiration windows.** After the first iteration of an iterative-
   deepening loop, narrow the window around the previous PV's score.
3. **v5 ID + TT.** Same depth in dramatically fewer nodes via TT cache
   hits across iterations. The right long-term fix.

For now the cost is acceptable; the correctness fix takes priority.

If a user reports v4 Hard "too slow" in the opening, the available
mitigations until v5 lands:

- Drop to `Difficulty::Normal` + `&depth=3` for v4-quality at v3-cost.
- Switch to `?engine=v3` for v3 speed (accepting the horizon-effect
  blunders).
- Set `?depth=3` to cap depth without changing difficulty's randomness.

## Reproducing

```sh
cargo test -p chess-ai --release perf -- --ignored --nocapture
```

The test is `#[ignore]` so it stays out of normal `cargo test` runs.
There are no hard pass/fail thresholds — these numbers exist to catch
order-of-magnitude regressions and to inform decisions like "can we
afford depth 5?". Asserting on wall-clock would make CI flaky across
machines.

To compare two implementations:

```sh
git stash; cargo test -p chess-ai --release perf -- --ignored --nocapture > before.txt
git stash pop; cargo test -p chess-ai --release perf -- --ignored --nocapture > after.txt
diff before.txt after.txt
```

## See also

- [`README.md`](README.md) — version index & switching guide
- [`v3-king-safety-pst.md`](v3-king-safety-pst.md) — current default engine spec
- [`backlog/chess-ai-search.md`](../../backlog/chess-ai-search.md) — roadmap (v4 = ID+TT, v5 = quiescence, …)
