# Win-rate sample never reaches ~100% even at mate-in-1

<!-- Filled progressively via Edit; section anchors below are stable. -->

**Symptoms** (grep this section): win-rate display 1% / 99% never reached, eval bar stops at ~88% even at mate-in-1, sidebar `紅 % • 黑 %` badge stuck on stale percentage, `<EvalChart>` line never touches the floor or ceiling, `EvalSample` cp value mismatched with position, sample-write reactive effect, debug_analysis hint_analysis race, `push_or_replace_sample` dedup-by-ply, stale `state.history.len()`, `?evalbar=1` wrong cp
**First seen**: 2026-05-11
**Affects**: chess-web `?evalbar=1` mode (chess-web before commit `fc51f24`)
**Status**: fixed in `fc51f24` (2026-05-11)

## Symptom

User playing vs AI on `/local/xiangqi?mode=ai&...&evalbar=1` reported
(2026-05-11):

> 「勝率即使一邊已經是 mate-in-1 也到不了 ~100%。」

Visually:

- Side-to-move's POV cp from `chess_ai::analyze` is `+999_999` (mate
  in 1). `cp_to_win_pct` correctly maps that to `0.99`. Direct unit
  tests in `crates/chess-ai/src/lib.rs:1221`
  (`cp_to_win_pct_mate_clamps_to_1_or_99`) all pass.
- But the live sidebar badge in the running app reads `紅 88% • 黑 12%`
  (or similar mid-band value), not `99% / 1%`.
- The `<EvalChart>` line at game end stops 5–10 % shy of the chart
  edge, even though the cp values logged from `chess_ai::analyze`
  clearly hit the early-out band (`|cp| >= MATE - 1000`).
- The most recent `EvalSample.red_win_pct` written into
  `RwSignal<Vec<EvalSample>>` matches the badge, **NOT** the
  current `chess_ai::analyze` output.

i.e., `cp_to_win_pct` is fine; the problem is which cp gets paired
with which ply when constructing the `EvalSample`.

## Root cause

A reactive `create_effect` that watches **three** signals at once and
tries to pair them at fire time produces a stale-pairing race when
two of the signals update on the same tick.

The original `pages/local.rs` design (commit `b195054`) had:

- `state: RwSignal<GameState>` — the engine's current position,
  updated by every `state.update(|s| s.make_move(...))`.
- `debug_analysis: RwSignal<Option<AiAnalysis>>` — last analysis
  written by the AI move pump (post-AI-move analysis from the AI's
  POV, kept for `?debug=1` panel).
- `hint_analysis: RwSignal<Option<AiAnalysis>>` — last analysis
  written by the hint pump (current side-to-move's POV).

When the user (or AI) played a move:

1. `state.update(|s| s.make_move(m))` — `state` signal fires.
2. **Same tick**: any `create_effect` tracking `state` re-runs.
3. The reactive sample-write effect was tracking `(state, hint_analysis,
   debug_analysis)`. It re-ran with the NEW `state` (and therefore the
   NEW `state.history.len()`) but the OLD `hint_analysis` /
   `debug_analysis` values (the analyses computed at the *previous*
   position, before the move).
4. The effect built `EvalSample::new(history.len(), state.side_to_move,
   stale_analysis.chosen.score)` and pushed it. Sample's `ply` field
   is correct (post-move), but its `cp_stm_pov` belongs to the
   previous position.
5. ~100 ms later, the hint pump's spawned task lands the FRESH analysis
   (computed against the new state). The reactive effect re-fires
   with the fresh value — but the dedup-by-ply guard now sees a
   sample already exists at this ply count and **skips**.

Net result: the recorded sample's cp is one ply behind reality. At
mate-in-1, the recorded cp is whatever the previous (non-mate)
position evaluated to — typically a mid-band value like ±300 cp
mapping to ~88 % / ~12 %.

### What the original code looked like

Roughly (reconstructed from the diff in `fc51f24`):

```rust
// REMOVED in fc51f24 — kept here as historical record.
if evalbar_enabled {
    create_effect(move |_| {
        // Track all three so the effect re-runs on any update.
        let history_len = state.with(|s| s.history.len());      // NEW after make_move
        let stm = state.with(|s| s.side_to_move);                // NEW after make_move
        let dbg = debug_analysis.get();                          // STALE on first re-run
        let hnt = hint_analysis.get();                           // STALE on first re-run
        let analysis = hnt.or(dbg);
        let Some(a) = analysis else { return };
        let next_ply = history_len;
        // Dedup-by-ply: skip if a sample for this ply already exists.
        let already_present = eval_samples.with(|v| v.iter().any(|s| s.ply == next_ply));
        if already_present {
            return;
        }
        let sample = EvalSample::new(next_ply, stm, a.chosen.score);
        eval_samples.update(|v| v.push(sample));
    });
}
```

The bug isn't in any single line — every line is locally correct.
The bug is in the *coupling*: the effect treats `state` and the two
analysis caches as *independent* values that just happen to be
reactive, when in reality they're causally chained (an analysis is
computed *for* a specific state, and only valid for that state).

### Why dedup-by-ply made it sticky

The sticky behaviour comes from the dedup guard. Without it, the
fresh analysis ~100 ms later would push a *second* sample at the
same ply, and the chart would still render the latest one
(`samples.last()` for the badge; the line would have an extra dot
visually). The bug would be cosmetic.

With `if already_present { return; }`, the first (stale) sample
*locks* the slot. Subsequent reads always return the stale value;
the fresh analysis is silently discarded. The bug becomes structural
— the chart's last point is stuck at the prior position's eval, no
matter how long you wait.

This is the dual problem to the more familiar "pump didn't dedup
and produced duplicates" bug: a missing dedup creates noise; an
*over-eager* dedup creates silent staleness.

## Workaround / Fix (shipped 2026-05-11)

Two coupled changes:

**1. Remove the reactive sample-write effect entirely.** Each pump
that produces an analysis now owns its own sample write, scoped to
the moment the analysis completes (and therefore paired with the
exact `(history.len(), side_to_move)` it was computed against).

**AI move pump** (`clients/chess-web/src/pages/local.rs:609-631`):

```rust
if let Some(a) = analysis {
    let chosen_mv = a.chosen.mv.clone();
    let ai_pov_score = a.chosen.score;
    // ... apply move, update state ...
    if evalbar_enabled {
        let new_ply = state.with_untracked(|s| s.history.len());
        let new_stm = state.with_untracked(|s| s.side_to_move);
        push_or_replace_sample(
            eval_samples,
            EvalSample::new(new_ply, new_stm, -ai_pov_score),
            //                                ^^^ negate: stm flipped
        );
    }
}
```

**Hint pump** (`clients/chess-web/src/pages/local.rs:743-764`):

```rust
if let Some(a) = analysis {
    if evalbar_enabled {
        push_or_replace_sample(
            eval_samples,
            EvalSample::new(
                snapshot.history.len(),    // captured before spawn
                snapshot.side_to_move,     // captured before spawn
                a.chosen.score,            // no negation: snapshot's stm IS the analyser's POV
            ),
        );
    }
    hint_analysis.set(Some(a));
}
```

Both writes use values *captured at analyse time* (the AI pump's
`ai_pov_score` was assigned before `make_move`; the hint pump's
`snapshot` was cloned before `spawn_local`), so the `(ply, cp)`
pair is causally consistent regardless of how many ticks elapse
between the analysis completing and the write landing.

**2. Replace dedup-by-ply with `push_or_replace_sample`**
(`clients/chess-web/src/eval.rs:140`):

```rust
pub fn push_or_replace_sample(samples: RwSignal<Vec<EvalSample>>, sample: EvalSample) {
    samples.update(|v| {
        if let Some(idx) = v.iter().position(|s| s.ply == sample.ply) {
            v[idx] = sample;            // <-- replace, not skip
        } else {
            v.push(sample);
        }
    });
}
```

So if the AI pump and the hint pump both write a sample for the
same ply (legitimate in vs-AI mode where the AI pump's analysis
becomes the post-move sample, and the hint pump's analysis from
the human's POV may follow shortly after), the freshest one wins.
Each individual write is causally consistent (point 1 above), so
"freshest wins" is genuinely an improvement, not "last writer
clobbers correct earlier writer".

## Prevention

The general lesson: **don't pair causally-related signals at
reactive-effect fire time.** A reactive effect that watches multiple
signals re-runs whenever ANY of them change — but the read of the
*other* signals returns whatever value was current at the moment of
the read, which may be stale relative to the trigger.

Specific patterns to use instead:

- **Capture-and-spawn** (used by the hint pump): when an effect needs
  to consume a signal value alongside a triggered async task,
  `state.with_untracked(|s| s.clone())` BEFORE the `spawn_local`
  and use the snapshot inside the task. The snapshot is causally
  pinned to the trigger time; the live signal is free to evolve.
- **Direct write at producer site** (used by both pumps post-fix):
  whichever code path produces the data also writes the derived
  artefact. No reactive effect needed; the write is in the same
  scope as the data, so there's no opportunity for the data to
  drift before the write happens.
- **Replace, don't skip**: when dedup is necessary, prefer
  "replace if exists" over "skip if exists" unless you can
  *prove* every potential writer is producing equivalent data.
  Skip-if-exists is a one-way ratchet that turns transient
  staleness into permanent staleness.

A code-smell heuristic: any reactive effect tracking 2+ signals AND
calling `.with()` / `.get()` on those signals (rather than just
deriving from them) is at risk. Either:

- Reduce to one tracked signal (the cause; derive the rest).
- Move the work out of the effect into the signal-update site
  (the producer).
- Use `with_untracked` for the dependent reads to make the staleness
  *intentional* and visible in code review.

## Related

- [`docs/ai/win-rate.md`](../docs/ai/win-rate.md) §7.3 — full bug
  history including the other three issues fixed in `fc51f24`.
- [`docs/ai/win-rate.md`](../docs/ai/win-rate.md) §4.1–4.4 — current
  sample-write architecture (the post-fix design).
- [`pitfalls/leptos-effect-tracking-stale-epoch.md`](leptos-effect-tracking-stale-epoch.md)
  — sibling Leptos reactive-runtime trap (the AI move pump's
  `move_epoch` race). Same class of bug — async tasks consuming
  reactive signals — different specific manifestation. Read both
  before writing a new reactive effect that spawns an async task.
- [`pitfalls/alpha-beta-root-score-pollution.md`](alpha-beta-root-score-pollution.md)
  — different layer entirely (search side, not UI side), but the
  meta-lesson is similar: a value's correctness depends on the
  context it was computed in; consumers that re-interpret it in a
  different context (different alpha-beta window there, different
  position here) get stale-but-plausible results that take a long
  time to surface as user-visible bugs.
- `clients/chess-web/src/pages/local.rs:609-805` — current sample-
  write code paths (AI pump + hint pump + Ongoing→ended pin).
- Commit `fc51f24` — verbatim diff of the fix.
