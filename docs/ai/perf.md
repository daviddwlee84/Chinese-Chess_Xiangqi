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

## Measurements (2026-05-09)

All runs use `Randomness::STRICT` so the measurement is search cost
only — no RNG variation between repeats. Median wall-clock over 3 runs;
nodes are deterministic given the search inputs.

| Fixture | Strategy | Difficulty | Depth | Nodes | Median ms (3 runs) | nodes/ms |
|---------|----------|------------|-------|-------|--------------------|---------|
| opening (initial xiangqi) | v1 | Easy | 1 | 44 | 0.07 | 603 |
| opening (initial xiangqi) | v1 | Normal | 3 | 2087 | 3.45 | 606 |
| opening (initial xiangqi) | v1 | Hard | 4 | 7743 | 14.86 | 521 |
| opening (initial xiangqi) | v2 | Easy | 1 | 44 | 0.12 | 358 |
| opening (initial xiangqi) | v2 | Normal | 3 | 2087 | 3.73 | 560 |
| opening (initial xiangqi) | v2 | Hard | 4 | 15941 | 29.98 | 532 |
| opening (initial xiangqi) | v3 | Easy | 1 | 44 | 0.08 | 543 |
| opening (initial xiangqi) | v3 | Normal | 3 | 2087 | 3.81 | 548 |
| opening (initial xiangqi) | v3 | Hard | 4 | 15938 | 29.39 | 542 |
| midgame (4 pieces removed) | v1 | Easy | 1 | 56 | 0.10 | 554 |
| midgame (4 pieces removed) | v1 | Normal | 3 | 5984 | 11.16 | 536 |
| midgame (4 pieces removed) | v1 | Hard | 4 | 9295 | 20.43 | 455 |
| midgame (4 pieces removed) | v2 | Easy | 1 | 56 | 0.16 | 350 |
| midgame (4 pieces removed) | v2 | Normal | 3 | 6032 | 12.34 | 489 |
| midgame (4 pieces removed) | v2 | Hard | 4 | 9340 | 21.74 | 430 |
| midgame (4 pieces removed) | v3 | Easy | 1 | 56 | 0.15 | 381 |
| midgame (4 pieces removed) | v3 | Normal | 3 | 6032 | 12.51 | 482 |
| midgame (4 pieces removed) | v3 | Hard | 4 | 9132 | 21.03 | 434 |
| sparse endgame (5 pieces total) | v1 | Easy | 1 | 18 | 0.01 | 1385 |
| sparse endgame (5 pieces total) | v1 | Normal | 3 | 448 | 0.46 | 974 |
| sparse endgame (5 pieces total) | v1 | Hard | 4 | 1144 | 0.91 | 1260 |
| sparse endgame (5 pieces total) | v2 | Easy | 1 | 18 | 0.01 | 1385 |
| sparse endgame (5 pieces total) | v2 | Normal | 3 | 2867 | 2.19 | 1310 |
| sparse endgame (5 pieces total) | v2 | Hard | 4 | 16150 | 12.37 | 1305 |
| sparse endgame (5 pieces total) | v3 | Easy | 1 | 18 | 0.01 | 1385 |
| sparse endgame (5 pieces total) | v3 | Normal | 3 | 2040 | 1.49 | 1372 |
| sparse endgame (5 pieces total) | v3 | Hard | 4 | 18047 | 13.96 | 1292 |

## Headroom analysis

The current `Difficulty::Hard` (depth 4) costs **15-30 ms wall-clock**
on native release. Even the worst measured case (opening v3 Hard) is
30 ms — well under the 250k node budget (only 6% used) and well under
any human-perceptible latency.

What we could push without users noticing:

| Target depth | Estimated cost | Verdict |
|---|---|---|
| **5** | ~10× current (200-300 ms) | Snappy. Well within human tolerance. |
| **6** | ~100× current (~3 s) | Borderline. Needs "AI thinking…" UI affordance. |
| **7** | ~1000× current (~30 s) | Unacceptable without iterative deepening + time budget. |

This is why the v4 roadmap entry is **iterative deepening + transposition
table** — the right way to push past depth 4 isn't to bump the depth
constant, it's to deepen until a time budget runs out and reuse work
across iterations via the TT.

## Why v2/v3 visit more nodes than v1

The capture-first move ordering produces α-β cutoffs based on score
*differences*. v1's flat material eval makes positions look
near-symmetric (everything is 0 or ±100 cp), so the cutoff threshold
is reached earlier and many sub-trees get pruned. v2 and v3 add PST
deltas (±5..30 cp), which differentiate moves more finely → α-β has
more "real" candidates to explore before pruning fires.

This is a *strength-vs-cost trade-off*, not a regression. v3 explores
~2× the nodes of v1 at the same depth and plays significantly better
moves; the wall-clock is still under 30 ms.

The endgame v2/v3 spike (16-18k nodes) is because the sparse 5-piece
position has very few captures, so capture-first ordering provides
almost no help — α-β has to lean entirely on score-based cutoffs,
which take longer to develop without high-frequency captures.

## WASM expectations

Native release profile: ~30 ms worst case Hard.
Browser WASM (Trunk + wasm-bindgen, release profile): typically
**5-10× slower** than native ⇒ ~150-300 ms per Hard move. Still snappy.

The 80 ms `gloo_timers::TimeoutFuture` yield in
[`clients/chess-web/src/pages/local.rs`](../../clients/chess-web/src/pages/local.rs)
is for the "AI thinking…" banner repaint, not for engine work. If WASM
search ever creeps past 200-300 ms (e.g. v4 with deeper iterative
deepening), the v6 roadmap entry — moving search to a Web Worker —
becomes the right fix, not bumping the yield.

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
