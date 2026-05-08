# v1 — Material-only evaluator

> **Status**: shipped 2026-05-08 (initial MVP).
> **Strategy**: `chess_ai::Strategy::MaterialV1` (URL/CLI token: `v1`).
> **Code**: [`crates/chess-ai/src/eval/material_v1.rs`](../../crates/chess-ai/src/eval/material_v1.rs),
> [`crates/chess-ai/src/engines/mod.rs::NegamaxV1`](../../crates/chess-ai/src/engines/mod.rs).

## Algorithm

Negamax with α-β pruning, capture-first move ordering, and a 250k node
budget. No iterative deepening, no transposition table, no quiescence,
no opening book. Search code lives in
[`search/mod.rs`](../../crates/chess-ai/src/search/mod.rs) and is shared
verbatim with v2.

| Knob | Value |
|---|---|
| Move ordering | captures sorted before quiet moves (single pass) |
| Pruning | α-β fail-soft |
| Node budget | 250 000 |
| Mate score | `-MATE + depth` (prefer faster mates) |
| TT / quiescence / ID | none (planned for v3+) |

## Difficulty mapping

| Difficulty | Default depth | Move-pick rule | Notes |
|---|---|---|---|
| Easy | 1 | uniform random pick from top-3 by score | obvious blunders allowed |
| Normal | 3 | random pick within ±10cp of best | mostly best, occasional sidesteps |
| Hard | 4 | strict best (first ply on tie) | deterministic given a seed |

`AiOptions::seed` controls the Easy / Normal randomness; identical
`(state, opts)` produces identical output for a given seed.

## Evaluation

```text
score = Σ_squares  (piece.side == me ? +value : -value)
```

| Piece | Value |
|---|---|
| 將/帥 (General) | excluded — checkmate handled by mate score |
| 仕/士 (Advisor) | 200 |
| 相/象 (Elephant) | 200 |
| 車 (Chariot) | 900 |
| 馬 (Horse) | 400 |
| 炮 (Cannon) | 450 |
| 卒/兵 (Soldier) | 100, +100 once crossed the river |

The river bonus on soldiers is the **only** positional term in v1.
Everything else is material.

## Known weaknesses

- **Random-feeling opening.** Every reasonable opening move evaluates to
  exactly 0 cp (material is symmetric and uncrossed soldiers tie).
  - Easy / Normal then sample uniformly across the top set.
  - Hard returns the first capture-ordered legal move, which deterministically
    picks something inert like 兵三進一.
  - Fixed in v2 by adding piece-square tables.
- **No king safety.** v1 has no notion of "general exposed by 飛將" or
  "stack of defenders missing". Slated for v3 / v4.
- **No mobility.** A trapped horse has the same value as an active one,
  modulo material. Slated for v3 / v4.
- **Horizon effect on captures.** Without quiescence the search may stop
  one ply after a capture and miss the recapture. Slated for v4.
- **Banqi unsupported.** v1 (and v2) are xiangqi-only; banqi has hidden
  tiles which alpha-beta would peek through. Slated for v6 (ISMCTS).

## Performance

Native (M-class Mac, debug profile): ≤50 ms per `Hard` move at depth 4
on opening / midgame positions; node budget rarely hit before depth 5.

WASM (browser, release profile): ≤300 ms per `Hard` move; the 80 ms
animation-frame yield in `clients/chess-web/src/pages/local.rs` is for
the "AI thinking…" banner repaint, not for engine work.

## Tests

In [`crates/chess-ai/src/lib.rs::tests`](../../crates/chess-ai/src/lib.rs)
(strategy-parameterised so v1 and v2 both pass):

- `opening_xiangqi_returns_a_legal_move_each_difficulty_each_strategy`
- `no_legal_moves_returns_none`
- `determinism_same_seed_same_move`
- `hard_prefers_capture_when_free` — the canonical material-aware sanity check.

## When to use v1

- **Repro of a 2026-05-08 game**. v1 is preserved bit-for-bit.
- **A/B comparison with v2+** to estimate the strength delta of PSTs.
- **Easy mode that really IS easy** — v1 + Easy is more chaotic than
  v2 + Easy because there's nothing tying down the random pick.

For all other purposes, use v2 (the default).
