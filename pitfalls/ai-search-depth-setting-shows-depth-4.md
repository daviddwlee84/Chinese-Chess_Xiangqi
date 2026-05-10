# AI search depth setting silently truncated — Search depth=N displays as 4

**Symptoms** (grep this section): "Search depth (advanced)", picker
depth field 10, AI debug panel Depth 4, depth setting ignored, set
depth 10 got 4, `?depth=10`, `MAX_AI_DEPTH`, NODE_BUDGET 250000,
`reached_depth`, iterative deepening bailed, v5 budget hit
**First seen**: 2026-05-10
**Affects**: `chess-ai` v5 (`Strategy::IterativeDeepeningTtV5`),
chess-web vs-Computer mode, chess-tui `--ai-depth` flag
**Status**: visibility fix shipped 2026-05-10 (target_depth + budget_hit
surfaced on debug panel); root-cause budget tuning tracked in `TODO.md`

## Symptom

User picks "Xiangqi vs Computer" in the chess-web picker, opens
"Search depth (advanced)", types `10`, clicks Start. The picker
URL-encodes `&depth=10` correctly. After the AI's first move the
🔍 AI Debug panel reports `Depth: 4` instead of 10. There is no
warning, no log line, no error. Toggling Hard difficulty (whose
default is depth 4) gives the same display, so the user can't
distinguish "my depth=10 worked" from "my depth=10 was ignored".

Same story in `chess-tui` with `--ai-debug --ai-depth 10`: the AI
Debug header reports `depth 4` regardless of `--ai-depth`.

## Root cause

The setting is **NOT** being ignored anywhere — the data flow is:

```
picker (clamp 1..=MAX_AI_DEPTH=10)         clients/chess-web/src/pages/picker.rs:108
  → URL ?depth=10                          clients/chess-web/src/routes.rs:264
  → LocalRulesParams.ai_depth              clients/chess-web/src/routes.rs:136
  → AiOptions { max_depth: Some(10) }      clients/chess-web/src/pages/local.rs:457
  → score_root_moves_v5(target_depth=10)   crates/chess-ai/src/engines/mod.rs:198
```

Inside `score_root_moves_v5` the iterative-deepening loop
(`crates/chess-ai/src/search/v5.rs:175-234`) walks `for d in
1..=target_depth` and **only updates `reached_depth = d` when an
iteration completes**. The hard cap `NODE_BUDGET = 250_000`
(`crates/chess-ai/src/search/mod.rs:30`) is enforced both inside
`negamax_v5` (line 75-77, returns static eval) and at the root after
each fully-searched root move (line 219-225, breaks the iteration).
For typical opening / midgame Xiangqi positions, v5 completes depths
1-4 within the 250 k budget but blows the cap partway into depth 5.
The outer loop sees `iter_complete = false`, breaks, and returns
whatever `last_completed` was — so `reached_depth = 4`.

`analyze_v5` then writes `reached_depth` (not `target_depth`) into
both `AiAnalysis.depth` (`engines/mod.rs:215`) and `AiMoveResult.depth`
(`engines/mod.rs:207`). The debug panel reads `analysis.depth`
faithfully and displays 4. **The data path is correct; the displayed
value is honest.** What's missing is any indication that the value is
honest-but-truncated.

The "4" coincides with `Difficulty::Hard.default_depth()` purely by
chance — if a future tweak changes Hard's default to 5, the symptom
will look like "depth=10 produces 5", and the same root cause applies.

## Workaround

Two layers, both shipped 2026-05-10:

1. **Visibility (`AiAnalysis` schema)** — added two new public fields:
   - `target_depth: u8` — what the caller asked for (after
     `Difficulty::default_depth` resolution + `.max(1)` clamp).
   - `budget_hit: bool` — `true` when the search couldn't deliver
     `target_depth` (v5: ID loop bailed; v1-v4: some legal moves
     unscored).
2. **Display (debug panel)** — when `budget_hit && depth <
   target_depth`, the `Depth` cell renders `4 / 10 (cap)` with a
   tooltip explaining that the node budget interrupted iterative
   deepening. Same change in the chess-tui debug header
   (`clients/chess-tui/src/ui.rs:1099`).

For users who actually want depth 10 to take effect, the only fix
today is to **lower their request** until `budget_hit` is `false`, or
wait for the budget tuning work tracked under `TODO.md`. Per-engine
caps mean v5 typically maxes out at depth 4-5 from common positions
under the 250 k budget; v4 (no ID, raw negamax) can reach further
because it doesn't re-search shallower depths.

## Prevention

When a search engine reports a "reached" value distinct from the
caller's "requested" value, **always surface both** at the UI/log
boundary. The single-value form invites this exact bug class — the UI
believes it's reporting truth, the user believes the setting was
honored, and neither is wrong from their local POV but the
end-to-end behaviour is silently truncated.

Companion guideline for future engine versions: if a new strategy
adds a budget that can clip the requested depth, the strategy MUST
populate `AiAnalysis.target_depth` with the unclipped request and
`AiAnalysis.budget_hit = true` when truncation happens. The
`build_analysis` helper handles v1-v4 uniformly; v5 has its own
path because `score_root_moves_v5` returns the reached depth.

Regression tests pinning the schema:

- `chess-ai/src/lib.rs::analyze_reports_target_and_reached_depth` —
  every strategy fills `target_depth` from `max_depth.max(1)` and
  `depth <= target_depth`.
- `chess-ai/src/lib.rs::analyze_target_depth_falls_back_to_difficulty_default`
  — `max_depth: None` resolves through `Difficulty::default_depth`,
  not 0.

## Related

- `pitfalls/alpha-beta-root-score-pollution.md` — sibling "search
  reports a value the user can't reconcile with their setting" bug
  class. There the score was wrong; here the depth is honest but
  silently truncated.
- `crates/chess-ai/src/search/v5.rs` — iterative-deepening loop.
- `crates/chess-ai/src/search/mod.rs:30` — `NODE_BUDGET = 250_000`
  constant; budget tuning is the real fix and lives in `TODO.md`.
- `clients/chess-web/src/components/debug_panel.rs::DebugMeta` —
  current depth-cell rendering with tooltip.
- `clients/chess-tui/src/ui.rs` (search for `depth_str`) — TUI
  equivalent.
