# chess-ai v6: pondering during human's turn

**Status**: P2 / not started
**Effort**: M (2-3 dev sessions; depends on v5's TT)
**Related**: [`chess-ai-search.md`](chess-ai-search.md) (parent roadmap),
[`docs/ai/v5-id-tt.md`](../docs/ai/v5-id-tt.md) (TT machinery this builds on),
[`pitfalls/leptos-effect-tracking-stale-epoch.md`](../pitfalls/leptos-effect-tracking-stale-epoch.md)
(reactive-runtime trap likely to bite)

## Why this is in backlog

v5's wall-clock cost (1-3 s WASM at Hard depth 4 in busy openings) is
most painful from the **user's** POV when it's their turn to move and
the AI is sitting idle. Pondering predicts the most likely human reply
and pre-searches the resulting position into the TT (which v5 made
possible). When the human actually plays the predicted move, the AI's
response feels near-instant.

This is a P2/M because it's a perceived-speed win without a strength
loss, but the mechanics are subtle enough (cancellation, race against
the human's actual move, TT pollution if the prediction is wrong) that
it deserves its own design pass before code.

## Scope

`Strategy::PonderingV6` — same evaluator and search as v5 (or whatever
the current default is at implementation time), with an additional
ponder loop that runs whenever:

- it's the **human's** turn (vs-AI mode)
- the game is ongoing
- a previous AI move just landed (so a "predicted human reply" is
  meaningful)

Out of scope for v6:

- Pondering in PvP / Pass-and-play (no AI to ponder; covered by hint
  pump already)
- Pondering in net mode (server doesn't tell us "AI's about to move");
  defer until net mode grows a server-side AI opponent
- Multi-PV pondering (consider top-N predicted replies). Could be a
  v6.1 follow-up if the prediction-miss rate is bad.

## Design

### Predicted-reply selection

The simplest workable predictor: run `score_root_moves_v5` on the
human's POV (which the hint pump *already does*) and ponder the
**top-1** reply. Two reuses for free:

1. Hint pump's analysis is already cached in `hint_analysis` —
   pondering can read its `chosen.mv` directly without a second search.
2. The ponder search itself runs `analyze` on the **post-predicted-
   reply** position. Same code path, no new search infrastructure.

Pseudocode:

```
on AI's-move-just-landed:
    predicted_human_mv = hint_analysis.read().chosen.mv
    ponder_position = state.clone()
    ponder_position.make_move(predicted_human_mv)
    spawn(async {
        chess_ai::analyze(&ponder_position, &ai_opts) // populates TT
    })
```

The TT writes inside `analyze` are persistent across calls (single
`TranspositionTable` owned by the `chess-ai` crate), so when the human
actually plays `predicted_human_mv`, the AI pump's subsequent `analyze`
hits the warm TT for free.

### Cancellation

Critical: the ponder task must abort cleanly when the human plays a
**different** move than predicted. Two layers:

1. **Soft (current epoch)**: ponder task captures `move_epoch` at start;
   re-checks before writing TT (or honestly, just lets the wasted work
   continue — TT entries for unreachable positions are harmless, just
   waste cache slots).
2. **Hard (busy budget)**: cap ponder to ≤ 100k nodes (vs 250k for the
   real AI search). Pondering is best-effort speedup, not a guarantee.

The `position_hash` invalidation pattern from the hint pump (added in
the 2026-05-10 hint-mode UX overhaul) is reusable here — see
`clients/chess-web/src/pages/local.rs` hint-pump for the template.

### TT pollution risk

If the prediction is wrong, the TT now contains entries for an
unreachable position. With v5's 2^17-slot always-replace TT, those
entries get evicted within ~1-2 plies. Negligible.

### Hit-rate target

A win if **>50%** of human moves match the prediction (i.e. the
predicted-reply was the bot's top-1 choice). Lower bound — even at
30% hit rate, perceived AI response time drops 30%. Easier to measure
than to engineer; instrument with a `ponder_hits / ponder_attempts`
counter in dev builds.

## Risks / unknowns

- **Battery drain on mobile**: extra search every human turn doubles
  wall-clock CPU usage in vs-AI sessions. Should be opt-in via a
  picker checkbox or `?ponder=1` URL flag, similar to how hints are
  opt-in.
- **Race with hint pump**: hint pump and ponder pump both want to run
  on every human-turn state change. Coordinate so they don't both
  spawn — or just let the hint pump's result *be* the ponder seed
  (they want the same prediction).
- **Reactive-effect ordering bugs**: see
  [`pitfalls/leptos-effect-tracking-stale-epoch.md`](../pitfalls/leptos-effect-tracking-stale-epoch.md).
  Pondering adds a third reactive effect to coordinate (AI pump +
  hint pump + ponder pump); easy to introduce subtle ordering bugs.
  Mitigation: drive everything off `position_hash` (the v5 Zobrist
  field) instead of `move_epoch` — same fix the hint pump used.

## Test plan

- Unit: `ponder_predicts_top1_then_real_move_matches` — mock state
  where the predicted reply is unambiguous; assert the AI's
  subsequent `analyze` reports >0 TT hits.
- Integration: `ponder_recovers_when_human_deviates` — human plays
  the predicted move 5/10 times; assert no panic, no incorrect AI
  response, instrumentation reports ~50% hit rate.
- Manual: open vs-AI Hard, play 20 moves, eyeball that AI moves after
  predicted-replies feel snappier than after surprise moves.

## Tasks

1. Instrument `TranspositionTable` with `hits / probes` counters
   (debug builds only).
2. Add `Strategy::PonderingV6` enum variant; wire `analyze_v6` to
   delegate to v5 with an extra ponder spawn afterwards. Engine
   default stays v5 — pondering is opt-in.
3. New `ponder_pump` reactive effect in `pages/local.rs`, gated on a
   new `?ponder=1` URL flag (and a corresponding picker checkbox in
   "AI insight panels (advanced)" fieldset).
4. CSS: optional "🤔 AI is pondering…" subtle indicator in sidebar
   (faded; doesn't interrupt anything).
5. Doc: `docs/ai/v6-pondering.md` per-version spec.

## When to revisit

After v5.1 (aspiration windows + depth-preferred TT) ships and the
average WASM Hard-depth-4 wall-clock drops. If v5.1 brings opening
search under 500 ms, pondering's perceived value drops sharply and
this can be deprioritized to P3.
