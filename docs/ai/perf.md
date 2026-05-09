# chess-ai performance

> **Last measured**: 2026-05-09 on M-class Mac, release profile.
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

## Measurements (2026-05-09, post-v4)

All runs use `Randomness::STRICT` so the measurement is search cost
only — no RNG variation between repeats. Median wall-clock over 3 runs;
nodes are deterministic given the search inputs.

| Fixture | Strategy | Difficulty | Depth | Nodes | Median ms (3 runs) | nodes/ms |
|---------|----------|------------|-------|-------|--------------------|---------|
| opening (initial xiangqi) | v1 | Easy | 1 | 44 | 0.08 | 550 |
| opening (initial xiangqi) | v1 | Normal | 3 | 2087 | 3.84 | 543 |
| opening (initial xiangqi) | v1 | Hard | 4 | 7743 | 14.05 | 551 |
| opening (initial xiangqi) | v2 | Easy | 1 | 44 | 0.09 | 518 |
| opening (initial xiangqi) | v2 | Normal | 3 | 2087 | 3.73 | 559 |
| opening (initial xiangqi) | v2 | Hard | 4 | 15941 | 29.99 | 532 |
| opening (initial xiangqi) | v3 | Easy | 1 | 44 | 0.09 | 506 |
| opening (initial xiangqi) | v3 | Normal | 3 | 2087 | 3.75 | 556 |
| opening (initial xiangqi) | v3 | Hard | 4 | 15938 | 30.34 | 525 |
| **opening (initial xiangqi)** | **v4** | **Easy** | **1** | **103** | **0.12** | **844** |
| **opening (initial xiangqi)** | **v4** | **Normal** | **3** | **13277** | **17.11** | **776** |
| **opening (initial xiangqi)** | **v4** | **Hard** | **4** | **177403** | **278.73** | **636** |
| midgame (4 pieces removed) | v1 | Easy | 1 | 56 | 0.14 | 400 |
| midgame (4 pieces removed) | v1 | Normal | 3 | 5984 | 12.17 | 492 |
| midgame (4 pieces removed) | v1 | Hard | 4 | 9295 | 25.11 | 370 |
| midgame (4 pieces removed) | v2 | Easy | 1 | 56 | 0.15 | 368 |
| midgame (4 pieces removed) | v2 | Normal | 3 | 6032 | 14.21 | 424 |
| midgame (4 pieces removed) | v2 | Hard | 4 | 9340 | 22.25 | 420 |
| midgame (4 pieces removed) | v3 | Easy | 1 | 56 | 0.12 | 448 |
| midgame (4 pieces removed) | v3 | Normal | 3 | 6032 | 15.24 | 396 |
| midgame (4 pieces removed) | v3 | Hard | 4 | 9132 | 25.11 | 364 |
| **midgame (4 pieces removed)** | **v4** | **Easy** | **1** | **775** | **1.05** | **736** |
| **midgame (4 pieces removed)** | **v4** | **Normal** | **3** | **6546** | **7.72** | **848** |
| **midgame (4 pieces removed)** | **v4** | **Hard** | **4** | **21370** | **33.21** | **643** |
| sparse endgame (5 pieces total) | v1 | Easy | 1 | 18 | 0.01 | 1385 |
| sparse endgame (5 pieces total) | v1 | Normal | 3 | 448 | 0.32 | 1396 |
| sparse endgame (5 pieces total) | v1 | Hard | 4 | 1144 | 1.33 | 860 |
| sparse endgame (5 pieces total) | v2 | Easy | 1 | 18 | 0.02 | 1125 |
| sparse endgame (5 pieces total) | v2 | Normal | 3 | 2867 | 2.53 | 1133 |
| sparse endgame (5 pieces total) | v2 | Hard | 4 | 16150 | 13.67 | 1181 |
| sparse endgame (5 pieces total) | v3 | Easy | 1 | 18 | 0.01 | 1500 |
| sparse endgame (5 pieces total) | v3 | Normal | 3 | 2040 | 1.41 | 1446 |
| sparse endgame (5 pieces total) | v3 | Hard | 4 | 18047 | 15.10 | 1195 |
| **sparse endgame (5 pieces total)** | **v4** | **Easy** | **1** | **36** | **0.02** | **2000** |
| **sparse endgame (5 pieces total)** | **v4** | **Normal** | **3** | **4972** | **2.74** | **1817** |
| **sparse endgame (5 pieces total)** | **v4** | **Hard** | **4** | **21668** | **12.03** | **1801** |

## v4 cost breakdown

v4 is **significantly more expensive than v3** in busy openings — the
worst case (v4 + Hard + opening) is 9-10× the v3 number:

| Fixture | v3 Hard nodes | v4 Hard nodes | Ratio |
|---|---|---|---|
| Opening | 15,938 | **177,403** | 11× |
| Midgame | 9,132 | 21,370 | 2.3× |
| Endgame | 18,047 | 21,668 | 1.2× |

Why opening is the worst case: the initial xiangqi position has zero
captures available, so quiescence terminates immediately at every leaf
(stand-pat = static eval). All v4 cost lives at the *root*'s MVV-LVA
ordering, which (compared to v3's flat capture-first sort) explores
more equally-rated lines before α-β cutoffs fire. PSTs differentiate
moves by ±5..30 cp, and MVV-LVA's tie-breaking adds further
differentiation, so more sub-trees pass the cutoff threshold.

Midgames and endgames have plenty of captures, so quiescence terminates
quickly at the leaves and MVV-LVA's better ordering pays off — node
counts stay similar to v3.

71% of the 250k node budget is consumed by v4 + Hard + opening. The
"safe" headroom that v3 had is now gone for this case. v5 (iterative
deepening + TT) is the right next step — same depth in fewer nodes via
TT lookups.

## Headroom analysis (post-v4)

| Configuration | Native ms | WASM est. | Verdict |
|---|---|---|---|
| v4 + Hard + depth 4 + busy opening | 280 ms | 1-3 s | Borderline. Users will see the wait. |
| v4 + Hard + depth 4 + midgame | 33 ms | 150-300 ms | Snappy. |
| v4 + Normal + depth 3 | 17 ms | 80-160 ms | Snappy. |
| v4 + Hard + depth 5 | est. 1-3 s | est. 5-30 s | **Don't.** Use v5 (ID+TT) when shipped. |
| v3 + Hard + depth 4 | 30 ms | 150-300 ms | Snappy but horizon-effect blunders return. |

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
