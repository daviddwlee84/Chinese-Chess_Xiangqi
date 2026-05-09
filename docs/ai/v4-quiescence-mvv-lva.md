# v4 — Quiescence + MVV-LVA

> **Status**: shipped 2026-05-09, **default**.
> **Strategy**: `chess_ai::Strategy::QuiescenceMvvLvaV4` (URL/CLI token: `v4`).
> **Code**: [`crates/chess-ai/src/search/quiescence.rs`](../../crates/chess-ai/src/search/quiescence.rs),
> [`crates/chess-ai/src/search/ordering.rs`](../../crates/chess-ai/src/search/ordering.rs),
> [`crates/chess-ai/src/engines/mod.rs::NegamaxQuiescenceMvvLvaV4`](../../crates/chess-ai/src/engines/mod.rs).

## What's new vs v3

Same evaluator (`MaterialKingSafetyPstV3` — material + PSTs + King safety).
Search changes:

1. **MVV-LVA move ordering** at every node. Replaces v1-v3's flat
   "captures before non-captures" sort with `victim_value × 10 -
   attacker_value`. Cheap-attacker-takes-valuable-piece is tried first
   → α-β finds the cutoff faster.

2. **Quiescence search** at the horizon (depth = 0). Instead of returning
   the static eval — which is wildly wrong mid-capture-exchange — the
   search now recurses into a capture-only sub-search until the
   position is *quiet* (no captures available). Stand-pat as the lower
   bound, MVV-LVA-ordered captures explored, α-β-pruned. Bounded by
   `Q_MAX_PLIES = 12` so a long capture chain can't blow the stack.

The combination targets the **horizon effect**: previously the AI
would gladly play "Cxa5 winning a soldier" with no awareness that the
defending chariot recaptures next move. v3 search at depth 4 sometimes
hides this (the search is deep enough to see the recapture if the
exchange is short), but at depth 1-3 it leaks; even at depth 4 there
are positions where a 2-deep capture chain crosses the horizon.

## Why this fixes the user complaint

User reported v3 occasionally making "obviously wrong" capture trades.
Quiescence is the textbook fix:

- v3 at depth 1: "Red's Cxa5 wins +100 cp soldier. Best move." → blunder
  (Black recaptures with chariot, net -800 cp).
- v4 at depth 1: stand-pat says 0 cp. Try Cxa5: recurse into quiescence,
  Black plays Rxa5 (forced — only capture), final position evaluated as
  -800 cp from Red's POV. v4 sees -800 < 0, declines the capture.

Regression test: `v4_avoids_horizon_effect_recapture_at_depth_1` in
[`crates/chess-ai/src/lib.rs::tests`](../../crates/chess-ai/src/lib.rs).

## Difficulty mapping

Same defaults as v3 (depth 1/3/4, randomness Chaotic/Varied/Subtle).
**v4 is significantly more expensive than v3** — see below — so depth
4 in busy openings can hit ~280 ms native (~1-3 s in browser WASM).

| Difficulty | Default depth | Default randomness | Approx. native ms (Hard, opening) |
|---|---|---|---|
| Easy | 1 | `CHAOTIC` | <1 ms |
| Normal | 3 | `VARIED` | ~17 ms |
| Hard | 4 | `SUBTLE` | ~280 ms (was ~30 ms in v3) |

## Performance vs v3

See [`perf.md`](perf.md) for the full table. Highlights:

| Fixture | v3 Hard | v4 Hard | Ratio |
|---|---|---|---|
| Opening (initial xiangqi) | 30 ms / 16k nodes | 279 ms / 177k nodes | 9-10× |
| Midgame (4 pieces removed) | 21 ms / 9k nodes | 33 ms / 21k nodes | 1.5× |
| Endgame (5 pieces total) | 14 ms / 18k nodes | 12 ms / 22k nodes | ~1× |

The opening case is by far the worst — v4 Hard hits 71% of the 250k
node budget. Why: openings have no captures available at the root, so
quiescence is fast at the leaves, but MVV-LVA at the root explores
more equally-rated lines before α-β cutoffs fire (vs v3's flat sort
that prunes earlier on equal scores). Midgames and endgames have
plenty of captures, so quiescence terminates quickly and the order of
magnitude is unchanged.

WASM browser estimate (5-10× slower than native): ~1-3 s for v4 Hard
in busy openings. The "AI thinking…" banner already paints; users will
see the wait. If 1-3 s feels too long, dropping to `Difficulty::Normal`
+ `&depth=3` keeps v4's quality at ~17 ms native (~100 ms WASM).

## What v4 still doesn't fix

- **Iterative deepening / time budget.** v4 still does fixed-depth
  search. With v4's much higher per-depth cost, depth 5+ is painful.
  Iterative deepening (v5) reuses shallow PVs to deepen efficiently
  AND lets you set a wall-clock budget instead of a depth.
- **Transposition table.** Many positions are reached via multiple
  move orders; v4 re-searches each one. Zobrist hashing + TT (v5)
  gives a large speedup AND unblocks the P1 threefold-repetition draw
  detection in `TODO.md` (shared infra).
- **King safety beyond raw "is it alive".** Inherited from v3's
  evaluator. v6 might add king-zone attack counts.
- **Banqi.** Still alpha-beta, still peeks. v7 (ISMCTS) is the planned
  banqi engine.

## When to use v4

- **Default.** This is what `chess_ai::choose_move` runs unless
  overridden. Picker / TUI / CLI all default to v4 since 2026-05-09.

For comparison or for the "v3 was fast, v4 is slow on Hard openings"
case, downgrade explicitly:

- Web: `?engine=v3` (or `v2`, `v1`)
- TUI: `--ai-engine v3`

For deterministic reproductions: pair with `&variation=strict` or
`--ai-variation strict`.

## Implementation notes for future readers

- `pick_with_randomness` in `engines::mod.rs` is shared between v1-v3
  and v4 (no policy change).
- The v1-v3 search functions (`negamax`, `score_root_moves`) are kept
  intact — v4 has its own `negamax_qmvv` / `score_root_moves_qmvv`
  parallel pair. Adding v5 (ID+TT) will likely add yet another pair
  rather than threading flags into the existing ones.
- MVV-LVA score is intentionally separate from eval piece values — the
  ordering table doesn't need to match the eval table. Tweaking
  ordering is purely a search-cost knob; tweaking eval changes
  perceived strength.
- Quiescence respects the same `NODE_BUDGET` (250k) as the main search,
  so a runaway capture chain still bails to a static eval.
