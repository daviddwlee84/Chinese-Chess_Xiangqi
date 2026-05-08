# Casual-rules xiangqi: AI walks into 1-ply general-capture mate

**Symptom**

User plays vs the in-process AI on `/local/xiangqi?mode=ai&...` (which uses
`RuleSet::xiangqi_casual()` — the picker default). On the AI's turn, the AI makes a move that *creates a screen* between the opponent's cannon and its own General, OR fails to move out of an already-existing 1-ply mate threat. Specific reproductions seen 2026-05-09:

- AI=Black plays 象 (file 6, rank 9) → (file 4, rank 7), placing the elephant directly between Red's central cannon at (file 4, rank 4) and Black's General at (file 4, rank 9). Red's next move captures the General via cannon-jump.
- AI in check ignores the threat and develops a piece on the other side of the board.
- AI losing on material attacks instead of defending.

The bug shows in **all** of v1 (`Strategy::MaterialV1`) and v2 (`Strategy::MaterialPstV2`). It does NOT appear in strict mode (`?strict=1`) because move-gen there filters out self-check.

**Root cause — version of the bug**

v1 / v2 evaluators give the General a piece value of **0 cp** with the comment:

```rust
// General is excluded from material — checkmate is handled by the
// mate score. Casual xiangqi (where a missing general is the loss
// condition) still works because `legal_moves` going empty after
// capture is the recursion's terminal.
```

That comment is **wrong for casual mode**. After Red captures Black's General:

- Black still has a chariot, horse, advisor, etc. → `legal_moves()` is non-empty
- The "no legal moves → -MATE + depth" terminal in `search/mod.rs::negamax` is **never reached**
- Eval just sums material with General = 0; "Black is missing the General" registers as a 0-cp swing
- The AI sees the post-capture line as roughly equal and happily plays the move that creates the screen

In strict mode this can't happen because the legality filter rejects any move that leaves the mover's General capturable, so the General is never *physically* captured during search — `legal_moves()` going empty really does only fire at checkmate / stalemate, and the mate-score terminal handles it.

**Fix (shipped 2026-05-09)**

Add v3 = `Strategy::MaterialKingSafetyPstV3` ([`crates/chess-ai/src/eval/material_king_safety_pst_v3.rs`](../crates/chess-ai/src/eval/material_king_safety_pst_v3.rs)):

```rust
PieceKind::General => KING_VALUE,   // 50_000 cp
```

Chosen so:

- `KING_VALUE` (50_000) >> Σ all other xiangqi material on one side (~5_300 cp). Capturing the General is by far the largest single eval swing possible — search will spend any amount of material to defend it.
- `KING_VALUE` (50_000) << `MATE / 2` (500_000). Negamax mate scores (`-MATE + depth` ≈ -1_000_000) remain comfortably distinguishable from "lost the General" eval scores.

v3 is the new default. v1 and v2 stay reachable via `?engine=v1|v2` (web) and `--ai-engine v1|v2` (TUI) for regression / comparison. See [`docs/ai/v3-king-safety-pst.md`](../docs/ai/v3-king-safety-pst.md) for the full spec.

**Why this is a class of bug, not a one-off**

Any evaluator that excludes the King from material on the assumption that "search terminals catch it" must verify the assumption holds for every active rule set. We had two of them (strict + casual) and the assumption only held in one. Future evaluators (v4 with king-safety, v5 with quiescence, etc.) inherit v3's `KING_VALUE` so the bug class is closed.

The general lesson: **don't lean on search terminals to model game-end states the eval doesn't see directly**. If the eval "thinks" a position is fine, the search will trust it. Make the eval correct first.

**Test that would have caught it**

In hindsight: a fixture under casual rules where the AI is one move from being mated, asserting the AI plays one of the defending moves. We had no such test in the v1/v2 PR — the `hard_prefers_capture_when_free` test only covers the symmetric "the AI is the one mating" case.

Now in [`crates/chess-ai/src/lib.rs::tests`](../crates/chess-ai/src/lib.rs):

- `v3_avoids_one_ply_general_capture_in_casual_mode` — fixture mirroring this report's scenario.
- `missing_general_swings_eval_by_king_value` (in `eval/material_king_safety_pst_v3.rs::tests`) — direct unit test of the KING_VALUE swing.

**See also**

- [`docs/ai/v3-king-safety-pst.md`](../docs/ai/v3-king-safety-pst.md) — full v3 spec, decision log on the 50_000 number.
- [`docs/ai/README.md`](../docs/ai/README.md) — version index, how to switch.
- [`backlog/chess-ai-search.md`](../backlog/chess-ai-search.md) — roadmap (v4 = ID+TT, v5 = quiescence, …).
- `crates/chess-ai/src/eval/material_v1.rs` — preserved comment showing the original (incorrect) assumption.
