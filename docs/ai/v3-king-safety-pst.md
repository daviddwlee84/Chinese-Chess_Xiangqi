# v3 — Material + PSTs + king safety

> **Status**: shipped 2026-05-09, **default**.
> **Strategy**: `chess_ai::Strategy::MaterialKingSafetyPstV3` (URL/CLI token: `v3`).
> **Code**: [`crates/chess-ai/src/eval/material_king_safety_pst_v3.rs`](../../crates/chess-ai/src/eval/material_king_safety_pst_v3.rs),
> [`crates/chess-ai/src/engines/mod.rs::NegamaxV3`](../../crates/chess-ai/src/engines/mod.rs).

## Why v3 exists — the king-blindness bug

v1 and v2 both gave the General a piece value of 0 cp, on the assumption
that "checkmate is handled by the mate score in negamax". This works in
**strict** xiangqi (`xiangqi_allow_self_check = false`), where the
legality filter rejects any move that would leave the mover's General in
check, so the General is *physically* never captured during search.

In **casual** xiangqi (`xiangqi_allow_self_check = true`, the picker
default and `RuleSet::xiangqi_casual()`) the legality filter is off.
Capturing the General becomes a real move, and the position after
General-capture has the side that just lost the General with several
non-king pieces still able to move — so the "no legal moves → mate
score" terminal in [`search/mod.rs::negamax`](../../crates/chess-ai/src/search/mod.rs)
is **never reached**. The AI evaluates the post-capture position with a
0-cp General and concludes it's basically a wash.

Concretely (the user-reported scenario from 2026-05-09):

- Black General at (file 4, rank 9), Red Cannon at (file 4, rank 4).
- Black plays 象 (file 6, rank 9) → (file 4, rank 7). Now there's exactly
  one screen between the cannon and the general.
- Red plays 炮 cannon-jump capture → Black General removed.
- v1/v2 evaluation of the post-capture position: Black has elephant
  (200) + chariot (900) + … vs Red's cannon (450) + … — looks roughly
  even. The General's removal is invisible.
- v1/v2 search at the root then evaluates "Black plays 象 → (4,7)" as a
  perfectly fine move. Black walks into a 1-ply mate.

Full pitfall write-up: [`pitfalls/casual-xiangqi-king-blindness.md`](../../pitfalls/casual-xiangqi-king-blindness.md).

## What v3 changes

Exactly one line of evaluator differs from v2:

```rust
PieceKind::General => KING_VALUE,   // v3: 50_000 cp
PieceKind::General => 0,            // v1/v2
```

`KING_VALUE = 50_000` is chosen so:

- **`KING_VALUE >> Σ all other material`** ≈ 9_600 cp. The eval prefers
  losing every other piece over losing the General. Search will spend
  20 chariots to save the King and still come out "ahead".
- **`KING_VALUE < MATE / 2`** = 500_000 cp. Negamax's depth-relative
  mate scores (`-MATE + depth` ≈ -1_000_000) remain comfortably
  distinguishable from "lost the General" eval scores. No collisions.

Search code, PSTs, difficulty mapping, and node budget are all
identical to v2. So v3 is a strict superset of v2 in playing strength
— never weaker, fixes one specific class of blunder.

## Difficulty mapping

Same depths as v1 / v2; the **randomness** defaults changed in 2026-05-09
to make Hard feel less repetitive (user feedback: "AI 幾乎會走一樣的步").

| Difficulty | Default depth | Default randomness | Move-pick rule |
|---|---|---|---|
| Easy | 1 | `Randomness::CHAOTIC` | top-10 moves within ±150 cp of best |
| Normal | 3 | `Randomness::VARIED` | top-5 within ±60 cp |
| Hard | 4 | `Randomness::SUBTLE` | top-3 within ±20 cp (was strict-best pre-2026-05-09) |

Hard's new `SUBTLE` default avoids the "AI plays the same opening every
game" complaint while keeping strength loss imperceptible (±20 cp at
depth 4 ≈ noise compared to typical position evaluation swings of
50-300 cp). Pass `randomness: Some(Randomness::STRICT)` in
[`AiOptions`](../../crates/chess-ai/src/lib.rs) to opt back into
deterministic best-move play (regression tests, replays, tournaments).

`KING_VALUE` (50_000 cp) is much larger than any of these tolerance
windows, so all variation presets still avoid king-blindness — losing
the General is filtered out by the `cp_window` even at `Randomness::CHAOTIC`.

## Tests

In [`material_king_safety_pst_v3.rs::tests`](../../crates/chess-ai/src/eval/material_king_safety_pst_v3.rs):

- `king_value_dominates_full_material` — KING_VALUE > 4× one side's max material.
- `missing_general_swings_eval_by_king_value` — removing one side's
  General changes the eval by ±KING_VALUE (within PST slack).

In [`crates/chess-ai/src/lib.rs::tests`](../../crates/chess-ai/src/lib.rs):

- `v3_avoids_one_ply_general_capture_in_casual_mode` — fixture mirroring
  the user's bug report. v3 must NOT walk into the elephant-screens-cannon
  trap. (v2 may or may not on a given seed; the underlying eval gap is
  asserted by `missing_general_swings_eval_by_king_value`.)
- `default_strategy_is_v3` — guards `Strategy::default()` regression.

## Performance vs v2

Identical: one extra `match` arm in `piece_value_v3` returns a constant
instead of 0. No new tables, no extra work per node. Same node budget,
same wall-clock.

## What v3 does NOT fix

- **Horizon effect on captures.** Without quiescence, the search may
  stop one ply after a capture and miss the recapture. Slated for v4.
- **Mobility / king safety beyond raw "is the King alive".** A King in
  a wide-open palace with all defenders gone scores the same as a King
  with full advisor + elephant cover. Slated for v4 or v5 with
  king-safety terms.
- **Strict-mode regressions.** v3's KING_VALUE is harmless under strict
  rules (the King is never captured), but adds nothing — strict players
  may see no behaviour change vs v2.
- **Banqi.** Still alpha-beta, still peeks at hidden tiles. v6 (ISMCTS)
  is the planned banqi engine.

## When to use v3

- **Default.** This is what `chess_ai::choose_move` runs unless the
  caller overrides `AiOptions::strategy`. Every interactive entry
  point (web picker, TUI picker, both CLI flags) defaults to v3 since
  2026-05-09.

For comparative play, v1 and v2 remain selectable via `?engine=v1` /
`?engine=v2` (web) or `--ai-engine v1` / `--ai-engine v2` (TUI).
