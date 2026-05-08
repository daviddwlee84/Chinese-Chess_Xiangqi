# v2 — Material + piece-square tables

> **Status**: shipped 2026-05-08 (default).
> **Strategy**: `chess_ai::Strategy::MaterialPstV2` (URL/CLI token: `v2`).
> **Code**: [`crates/chess-ai/src/eval/material_pst_v2.rs`](../../crates/chess-ai/src/eval/material_pst_v2.rs),
> [`crates/chess-ai/src/engines/mod.rs::NegamaxV2`](../../crates/chess-ai/src/engines/mod.rs).

## What's new vs v1

Same search code (negamax + α-β + capture-first ordering, see
[`v1-material.md`](v1-material.md)). Only the evaluator changed:

```text
score_v2(piece, square) = score_v1(piece, square) + pst_delta(piece, square)
```

PST deltas are small (±5..30 cp) so the v1 material values still
dominate. The deltas only break ties when material is equal — which
is exactly the opening-position case v1 handled badly (every move
scored 0).

## Why this fixes the "random opening" feel

In v1, the initial xiangqi position has every legal move evaluated at
score 0 (material is symmetric, no soldiers crossed). The difficulty
randomisation policy then samples uniformly across all moves on Easy
and Normal — which looks like the AI is just rolling a die.

With v2, e.g. 馬八進七 lands the horse on a higher-PST square (+30 vs
the home-rank +6), so even at depth 1 the move score is +24 cp from
Red's POV. The Hard pick is now the principled-best move; Easy and
Normal sample within the top set but that set is pre-filtered by PST.

## Piece-square table heuristics

All seven tables are 9-file × 10-rank `i8` arrays in
[`material_pst_v2.rs`](../../crates/chess-ai/src/eval/material_pst_v2.rs).
Tables encode Red's perspective; Black squares are looked up after
[`mirror_rank(r) = 9 - r`](../../crates/chess-ai/src/eval/material_pst_v2.rs).

Numbers are *clean-room hand-derived* from xiangqi opening principles —
they are NOT copied from any GPL engine (Pikafish / Elephantfish /
Xiangqigame are explicitly avoided per the project's clean-room
mandate). The justification per table:

### 將/帥 General — `PST_GENERAL`

Confined to the 3×3 palace by move-gen. Tiny incentive (+0..6) to stay
on the home rank rather than fly out to the palace edge — exposing on
the 4th file invites 飛將 (general-faces-general) tactics.

### 仕/士 Advisor — `PST_ADVISOR`

Palace-only. Centre square gets +5, corners get +3 — the centre
advisor double-covers the general from both diagonals.

### 相/象 Elephant — `PST_ELEPHANT`

Three-step diagonals, can't cross river. Centre elephant on (file 4,
rank 2) is the most defensive square (covers 4 elephant-points). Edge
elephants on file 0 / 8 cover only 2 squares — 0 delta. Slight bonus
for back-rank file 2 / 6 (covers central diagonals).

### 車 Chariot — `PST_CHARIOT`

Open-line piece. Reward presence on advanced ranks (rank 7 = +20 at
file 3/5) and central files (file 3,4,5 better than 0,8). Modest
values overall — the chariot's strength is mostly its 900cp material;
PST nudges it towards activity, doesn't override material.

### 馬 Horse — `PST_HORSE`

Steepest gradient of any table. Horse on a corner has 2 legal jumps;
horse on a central advanced square has 8. Reward central files
(+24..30 at file 3,4,5 ranks 4-6) and crossing the river (rank 5 → +28
central, rank 6 → +30). Penalty for back-rank corners (-4 at file 0/8
rank 0/9) — the dreaded 邊馬 trap.

### 炮 Cannon — `PST_CANNON`

Wants ranks where a screen exists. Centre file is the classic 中炮
opening bonus (+18 at rank 2 file 4). Reward middle ranks where shots
exist; mild penalty by zeroing out the 5th rank (river crossing is
neutral for the cannon — needs screens, not territory).

### 卒/兵 Soldier — `PST_SOLDIER`

Shaped to layer cleanly on top of v1's +100 "crossed river" bonus
(no double-count). Pre-river ranks 0-2 are zero. Rank 3 (Red home
soldiers) gets +4..6 to encourage advance. Post-river: +10..20 at
rank 5, +14..22 at rank 6, peaking at +30..34 on rank 7-8 along the
central file (where soldiers threaten the palace).

## Difficulty mapping

Same as v1 — `Difficulty::default_depth()` returns 1 / 3 / 4. The
randomisation rules also unchanged. v2 just makes the pre-randomisation
score distribution informative.

## Tests

In [`material_pst_v2.rs::tests`](../../crates/chess-ai/src/eval/material_pst_v2.rs)
(table-shape sanity):

- `red_horse_central_advanced_better_than_corner`
- `black_pst_mirrors_red`
- `central_cannon_better_than_flank`
- `general_outside_palace_is_zero`
- `advanced_soldier_beats_river_soldier`

In [`crates/chess-ai/src/lib.rs::tests`](../../crates/chess-ai/src/lib.rs)
(behaviour vs v1):

- `v2_breaks_v1_opening_tie_on_hard` — at least one root score is non-zero.
- `strategy_dispatch_is_distinguishable` — guard against accidentally
  wiring both `Strategy` variants to the same evaluator.

## Performance vs v1

PST lookup is one indirection + one constant table read per piece per
node. On opening positions (~32 pieces) the eval cost goes from ~32
multiplies (v1) to ~32 multiplies + ~32 table reads (v2) — well under
1% of the search time. **Same node budget, same wall-clock.**

## Tuning

The tables in `material_pst_v2.rs` are educated guesses, not
machine-tuned. Future tuning could:

- learn deltas from self-play (CMA-ES on the 7 × 90 = 630 parameters)
- import a tuned set from a public-domain xiangqi engine (post a
  licensing review)
- split into opening / midgame / endgame variants (game-phase aware)

These belong in v3 (or a v2.1 patch). The `Evaluator` trait isolates
search from any change here, so tuning rounds don't risk regressing
the search code.

## Known weaknesses

- **Static PSTs.** A horse on a great square that the opponent will
  capture next move still scores high. Quiescence (v4) fixes this.
- **No king safety, no mobility, no pawn-structure.** v2 only adds
  positional preference for individual pieces, not relations.
- **Hand-tuned numbers.** Some deltas may be over- or under-stated.
  See "Tuning" above.

## When to use v2

- **Default.** This is what `chess_ai::choose_move` runs unless the
  caller overrides `AiOptions::strategy`. Every interactive entry
  point (web picker, TUI picker, both CLIs) defaults to v2.

For comparative play / strength-delta measurement, run v1 alongside.
