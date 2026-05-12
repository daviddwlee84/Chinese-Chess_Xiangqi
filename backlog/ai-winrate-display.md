# AI win-rate display: per-ply samples + post-game trend chart

**Status**: shipped 2026-05-10
**Effort**: M (single PR, ~1k LOC across chess-ai + chess-web + chess-tui)
**Related**: [`chess-net-evalbar.md`](chess-net-evalbar.md) (P2 net-mode follow-up),
[`chess-ai-search.md`](chess-ai-search.md) (parent search roadmap),
[`../docs/ai/win-rate.md`](../docs/ai/win-rate.md) (full reference doc — math, lifecycle, bug history, calibration),
[`../pitfalls/eval-sample-stale-analysis-race.md`](../pitfalls/eval-sample-stale-analysis-race.md) (post-ship race condition fixed in `fc51f24`),
TODO.md "chess-ai: cp→win% calibration for xiangqi" (P3 calibration follow-up),
TODO.md "chess-web PWA: persist replays in IndexedDB / OPFS" (P3 persistence follow-up)

## What shipped

Three pieces driven by one new URL flag (`?evalbar=1` web /
`--evalbar` TUI):

1. **Live vertical eval bar** (web only) — Lichess-style SVG strip
   attached to the right of the board inside `.board-pane__row`. Red
   fills from the bottom; black from the top; boundary line + `紅 %`
   bottom label + `黑 %` top label.
2. **Sidebar headline badge** — compact `紅 52% • 黑 48%` row plus a
   thin gradient bar. Updates every turn as analyses land. Web mounts
   inside `<Sidebar>` via the new optional `eval_badge` prop; TUI
   shows a one-line `Eval: 紅 ████████░░░░ 黑 (52%)` row in the
   status panel.
3. **Trend chart** — auto-mounts once the first sample lands. Web is
   inline SVG (260×140 viewBox, polyline + filled advantage area + 50 %
   reference line + per-sample hover tooltips). TUI is press-`E`-to-toggle
   ASCII bar chart (one row per ply, last 24 plies shown, earlier
   samples elided).

`<MoveHistory>` rows also gain an optional `+5%` / `-12%` delta
annotation in mover's POV when paired before/after samples are available.

Both clients consume the same `chess_ai::cp_to_win_pct(cp: i32) -> f32`
helper (logistic with `WIN_PCT_K = 400`, clamped to `[0.01, 0.99]`)
so numbers match across web ↔ TUI on the same position.

## Design choices made (see chat log for full deliberation)

### Display style: eval bar + sidebar badge BOTH

User picked "兩個都做". The bar's vertical orientation matches the
visceral chess.com / lichess "see who's winning at a glance" UX; the
badge gives a numeric reading without taking screen space. Both feed
from the same `EvalSample`.

### Y-axis POV: always Red

User picked "素以紅方 POV". Consistent across vs-AI / PvP / future
spectator scenarios. Matches xiangqi tradition (Red is the bottom
seat). Implementation: `EvalSample.red_win_pct` is pre-computed at
sample-write time via `stm_cp_to_red_win_pct(cp_stm_pov, side_to_move)`
which negates for Black-to-move positions.

### TUI PvP: opt-in `--evalbar` flag

User picked "加 `--evalbar` flag, 開了才跑". TUI's PvP doesn't run
analyze by default (only vs-AI mode does in `ai_reply`). With
`--evalbar`, `apply_move` calls `run_pvp_eval` after each move
(synchronous, ~10–300 ms native at default Hard depth). Without the
flag, samples vector stays empty and the panel doesn't appear.

In **vs-AI mode**, samples come for free from the analysis the AI
already runs each turn — no extra search cost. The AI's
`analysis.chosen.score` is from the AI's POV after its move; the new
`side_to_move` is the opponent, so `record_eval_sample(g, -score)`
re-expresses it in stm-relative cp.

### Persistence: in-memory only

User picked "暫定純 in-memory, 跟 PWA IndexedDB 持久化一起做". The
`Vec<EvalSample>` lives in chess-web's leptos `RwSignal<Vec<EvalSample>>`
and chess-tui's `GameView.eval_samples: Vec<EvalSample>`. Refresh /
quit loses everything. The `chess-web PWA: persist replays in
IndexedDB / OPFS` P3 TODO entry is updated to note that when it
lands, eval samples should join the persisted record.

### Net mode: deferred to chess-net protocol v6

User picked "先做 chess-web local + TUI; net 開個 follow-up TODO".
The local-mode UX needs to settle first (especially the cp→win%
calibration and the layout polish around the eval bar). Net mode's
key open question is server-side vs client-side analyze (covered in
[`chess-net-evalbar.md`](chess-net-evalbar.md)).

### Calibration: K = 400 starting point

Borrowed from chess Elo. Likely too aggressive for xiangqi (where
chariot trades are more swingy than queen trades), but fine for v1.
Calibration is a new P3/S TODO ("chess-ai: cp→win% calibration for
xiangqi") — methodology described there.

## Architecture notes

### `EvalSample` lives in BOTH client crates (mirror struct)

Cleaner than promoting it to chess-ai (which is engine logic, not UI
data) or to a shared `clients/chess-client-shared` crate (doesn't
exist yet — see backlog `promote-client-shared.md`). The two structs
are byte-identical by intent; if a third client appears or the
clients diverge on shape, that's the trigger to promote.

`chess_ai::cp_to_win_pct` IS shared (engine math) — both clients call
it.

### Hint pump reuse

`pages/local.rs` already had a hint pump (`?hints=1`) that ran
`chess_ai::analyze` from the side-to-move's POV every turn. This
feature reuses it: the gate is now `if hints_enabled || evalbar_enabled`.
A separate sample-write effect watches `(debug_analysis, hint_analysis,
state.history.len())` and pushes a fresh `EvalSample` when a new
analysis lands AND no sample exists for that ply yet.

This means PvP + `?evalbar=1` (no `?hints=1`) costs ~one analyze per
ply — same as PvP + `?hints=1`. Pure vs-AI + `?evalbar=1` is FREE
(samples consumed from the AI move pump's existing analysis).

### Layout: `.board-pane__row` wrapper

Local mode now wraps `<Board>` + `<EvalBar>` (conditional) in a flex
row inside `.board-pane`. CSS rule for `svg.board` was updated from
`.board-pane > svg.board` (direct child) to `.board-pane svg.board`
(descendant) so net mode's `pages/play.rs` (which doesn't use the
row wrapper) stays working.

## Tasks completed

1. ✅ chess-ai: `cp_to_win_pct` + `WIN_PCT_K` const + 6 unit tests
2. ✅ chess-web: `?evalbar=1` URL flag (parse + emit + 5 round-trip tests)
3. ✅ chess-web: picker checkbox "📊 Win-rate display"
4. ✅ chess-web: `eval.rs` module with `EvalSample` + 4 conversion tests
5. ✅ chess-web: `<EvalBar>` SVG component
6. ✅ chess-web: `<EvalChart>` SVG component
7. ✅ chess-web: `pages/local.rs` plumbing — `InsightConfig.evalbar`,
   sample-write effect, undo/new-game reset, layout wrapper
8. ✅ chess-web: `<Sidebar>` `eval_badge` prop + internal `<EvalBadge>`
9. ✅ chess-web: `<MoveHistory>` per-row `eval_delta_pct` annotation
10. ✅ chess-web: ~150 LOC of CSS for the three components + history delta
11. ✅ chess-tui: `eval.rs` mirror struct
12. ✅ chess-tui: `--evalbar` CLI flag
13. ✅ chess-tui: `app.rs` plumbing — `evalbar_enabled` / `evalbar_open`
    AppState fields, `eval_samples` GameView field, `record_eval_sample`,
    `run_pvp_eval`, `Action::EvalbarToggle` handler
14. ✅ chess-tui: `input.rs` `E` key binding
15. ✅ chess-tui: `ui.rs` `push_eval_headline` + `push_eval_chart_lines`,
    HELP_LINES + sidebar hint update
16. ✅ Workspace `cargo fmt --check`, `cargo clippy -D warnings`,
    `cargo test --workspace`, `cargo check --target wasm32-unknown-unknown`
    all green

## Known limitations

- **Net mode unsupported**: see `chess-net-evalbar.md` for the v6
  protocol design.
- **Banqi / three-kingdom unsupported**: chess-ai is xiangqi-only
  (per `crates/chess-ai/src/lib.rs` module doc); the picker hides
  the checkbox there.
- **Calibration is rough**: K=400 produces visibly aggressive swings
  in tactical positions. Acceptable for v1 — see calibration TODO.
- **No initial-position sample in TUI**: chess-tui doesn't run an
  opening-position analyze, so the first sample is `ply == 1`.
  chess-web has the same limitation today (no proactive analyze on
  game start; the first sample lands after the first move). Would
  need a small "analyze on game start" hook in both clients to fix.

## When to revisit

- Net mode follow-up (`chess-net-evalbar.md`): when concurrent net
  rooms become a real workflow.
- Calibration follow-up: when users complain that the eval bar
  swings too much / too little. Current default is "good enough for
  exploration".
- PWA persistence: when `chess-web PWA: persist replays in IndexedDB
  / OPFS` lands — bundle eval samples into the same record so a
  resumed game shows the full pre-resumption trend chart.
