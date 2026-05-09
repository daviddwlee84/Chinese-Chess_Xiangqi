# Alpha-beta at the root pollutes non-best move scores

**Symptom**

User playing vs AI on `/local/xiangqi?mode=ai&...` reports the AI making
catastrophic blunders that the deeper search should easily avoid. Specifically (2026-05-09):

- Move 2: Black AI plays `c9e7` (elephant) — sets up Red's cannon screen.
- Move 3: User (Red) plays `e2xe6` (cannon captures Black soldier).
- Now Red's 炮 at e6 has screen 象 at e7, threatening cannon-jump mate on Black 將 at e9.
- Move 4: Black AI plays `g9i7` (other elephant moves to corner) — does NOT defend.
- Move 5: User plays cannon-jump → captures Black General → game over.

The bug appears on **v4** (`Strategy::QuiescenceMvvLvaV4`) much more
than v3 because v4's quiescence narrows the search window faster, but
both have the underlying flaw.

**Root cause**

`score_root_moves` and `score_root_moves_qmvv` were narrowing alpha
based on previously-scored root moves:

```rust
let mut alpha = -MATE - 1;
let beta = MATE + 1;
for mv in ordered {
    let v = -negamax(state, depth-1, -beta, -alpha, ...);
    scored.push(ScoredMove { mv, score: v });
    if v > alpha { alpha = v; }   // ← BUG: pollutes subsequent scores
}
```

This is **standard alpha-beta** — correct for finding the *best* move,
but **wrong for non-best move scores**. After a defensive move sets
`alpha = -132`, every subsequent root move's recursive search is called
with `beta_inner = -alpha = 132`. The inner search may then **fail-high**
(stand-pat ≥ beta) and return `beta = 132` instead of the move's true
value. That fail-high score (`-132` from the root's POV) is recorded
as the move's `score`.

Concrete repro: at the `e6 cannon screen` position, depth 3:

```
v4 root scores BEFORE fix:
  TOP   d9e8 score=-132   (defensive, true value ≈ -132 ✓)
  TOP   e9e8 score=-132   (defensive, true value ≈ -132 ✓)
  TOP   f9e8 score=-132   (defensive, true value ≈ -132 ✓)
  TOP   g9i7 score=-132   ← LIE. True value ≈ -50_000 (suicide).
  TOP   h9i7 score=-132   ← LIE.
  ...
  BOT   b7b3 score=-49710 (suicide, escaped the lie because its true
                           value was found before alpha narrowed)
```

Critical interaction with the [`Randomness`](../crates/chess-ai/src/lib.rs)
layer: `Difficulty::Hard.default_randomness()` is `Randomness::SUBTLE`
= "top-3 within ±20 cp of best". With genuine defensive moves (-132)
and lying suicide moves (also reported as -132), the picker can't tell
them apart and may pick the suicide ~⅓ of the time.

**Why v3 was less affected**

v3's flat capture-first ordering tried capture moves first. Captures
that lead to king-loss (e.g., `b7xb0` cannon-cap → Red counter-captures
General) returned their true `-49500` very early, setting alpha very
low (-49500). Subsequent quiet moves had a wide window (`-MATE-1 ..
49500`) so they returned their true scores without fail-highing.

v4's MVV-LVA puts the king-capture *for the opponent* first in inner
search (it has the highest possible MVV-LVA score), so the inner
fail-high triggers earlier and at smaller alpha, making the bug more
visible.

**Fix (shipped 2026-05-09)**

At the root, search every move with a **full window** (`-MATE-1 ..
MATE+1`). Don't propagate alpha between root moves:

```rust
for mv in ordered {
    let v = -negamax(state, depth-1, -(MATE+1), MATE+1, ...);
    scored.push(ScoredMove { mv, score: v });
    // No alpha update.
}
```

Cost: more nodes (no inter-root pruning). Empirically: ~1.5-2× cost in
busy openings, but v4 was already in budget headroom and this stays
within the 250k cap. Worth it for correct scores.

After the fix:
```
v4 root scores AFTER fix:
  TOP   d9e8 score=-132   (true defensive)
  TOP   f9e8 score=-132
  TOP   e7c9 score=-204   (also defensive; breaking the screen)
  TOP   e7g5 score=-204
  TOP   e7c5 score=-204
  ...
  BOT   b7b3 score=-50180 (true suicide)
  BOT   b7b6 score=-50180
  BOT   h7h4 score=-50108
```

Suicides correctly ranked; SUBTLE randomness (top-3 within ±20 cp)
picks only from the genuine defensive moves.

**Why this is a class of bug, not a one-off**

ANY consumer of `score_root_moves(_qmvv)` that interprets the returned
`score` as the move's *true* value is at risk. Today the consumer is
the `Randomness` layer. Future consumers (UI showing per-move scores,
tournament-style move analysis, human-in-the-loop tools) would all be
affected.

The general lesson: **alpha-beta returns bounds, not exact values**.
At any non-PV node, the returned score is "lower bound" or "upper
bound" depending on where the cut fired. Code that wants exact scores
needs full-window search (or PVS with re-search on fail-high).

**Test that would have caught it**

Now in [`crates/chess-ai/src/lib.rs::tests`](../crates/chess-ai/src/lib.rs):

- `v4_defends_general_after_red_central_cannon_lands_at_e6` —
  reproduces the exact game from the user's bug report; iterates all
  difficulties × strategies × 8 seeds; asserts that
  `Difficulty::Normal` and `Difficulty::Hard` never play a move that
  leaves the General capturable in 1 ply. (Easy is allowed to walk
  into the trap — depth 1 can't see it.)

This complements the `v3_avoids_one_ply_general_capture_in_casual_mode`
test which tests the eval-side fix (KING_VALUE = 50_000), not the
search-side fix.

**See also**

- [`docs/ai/v4-quiescence-mvv-lva.md`](../docs/ai/v4-quiescence-mvv-lva.md) —
  v4 spec including the search structure that exposed the bug.
- [`docs/ai/perf.md`](../docs/ai/perf.md) — cost analysis (the
  full-window-at-root fix is the reason v4 numbers shifted).
- `crates/chess-ai/src/search/mod.rs::score_root_moves[_qmvv]` —
  fix landed here.
- [`pitfalls/casual-xiangqi-king-blindness.md`](casual-xiangqi-king-blindness.md) —
  the v3 eval fix; the search-side bug was hidden by the eval fix
  having been applied first (so v3 looked OK) but resurfaced in v4.

**Future work**

Principal Variation Search (PVS) at the root would give the alpha-beta
benefit back: try the first move with full window, subsequent moves
with null window, re-search on fail-high. Worth it once we see the
node-count cost matter on the WASM side. For now full-window root is
simple and correct.
