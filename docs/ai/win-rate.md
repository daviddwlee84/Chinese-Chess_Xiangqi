# Win-rate display: cp → % conversion + per-ply samples + trend chart

<!-- Filled progressively via Edit; section anchors below are stable for
     cross-links from TODO.md / backlog/ / pitfalls/ / source comments. -->

## 1. Overview

The win-rate display turns the AI's centipawn (cp) score into a Red-side
win probability and surfaces it three ways:

1. **Live sidebar badge** — `紅 52% • 黑 48%` row + thin gradient bar
   that updates after every analysed ply (`<EvalBadge>` on web, one-line
   `Eval: 紅 ████████░░░░ 黑 (52%)` row on TUI).
2. **Per-move history annotation** — each move history row gets an
   optional `+5%` / `-12%` mover-POV delta showing how much that move
   improved or worsened the position (`HistoryEntry.eval_delta_pct`,
   web only — TUI's history doesn't carry deltas yet).
3. **Post-game trend chart** — SVG line chart of every recorded sample
   across the game (`<EvalChart>` on web; press `E` to toggle on TUI for
   an ASCII bar chart).

All three read from a single `Vec<EvalSample>` per game. Samples are
written by whichever code path produced the analysis: vs-AI mode reuses
the AI move pump's existing `chess_ai::analyze` output for free; PvP +
evalbar mode runs an extra synchronous analyse per move (~10–300 ms
native, ~0.5–3 s WASM at default Hard depth).

| Layer | chess-web | chess-tui | chess-net |
|---|---|---|---|
| URL/CLI flag | `?evalbar=1` | `--evalbar` | not supported (P2 follow-up — `backlog/chess-net-evalbar.md`) |
| Conversion math | `chess_ai::cp_to_win_pct` | `chess_ai::cp_to_win_pct` | (would broadcast pre-computed `red_win_pct`) |
| Per-game sample buffer | `RwSignal<Vec<EvalSample>>` in `pages/local.rs` | `GameView.eval_samples: Vec<EvalSample>` in `app.rs` | n/a |
| End-of-game pin | `EvalSample::final_outcome` (Ongoing→ended `create_effect`) | `EvalSample::final_outcome` (`record_eval_final` in `apply_move`) | n/a |

**Variant scope**: xiangqi only. `chess-ai` is xiangqi-only by design
(see `crates/chess-ai/src/lib.rs` module doc); the picker hides the
"Win-rate display" checkbox for banqi / three-kingdom variants. The
`final_outcome` helper does have a defensive Banqi/Green fallback
(returns 50/50 — see §3.3) so a future banqi-AI port wouldn't trip on
the `Won { winner: Side::GREEN, .. }` arm.

**Persistence**: in-memory only. Refresh / quit loses everything. PWA
IndexedDB persistence (P3 in `TODO.md`) will bundle the sample vector
into the same record so resumed games show the full pre-resumption
trend chart.

## 2. The math: `cp_to_win_pct`

The single source of truth for cp→% conversion lives in chess-ai so
both clients render identical numbers from the same cp input:

- **Function**: `pub fn cp_to_win_pct(cp: i32) -> f32` at
  `crates/chess-ai/src/lib.rs:398`
- **Tuning constant**: `pub const WIN_PCT_K: f32 = 400.0` at
  `crates/chess-ai/src/lib.rs:350`
- **Internal saturation threshold**: `const WIN_PCT_CLAMP_CP: i32 =
  25_000` at `crates/chess-ai/src/lib.rs:363`
- **Mate threshold (re-used)**: `pub const MATE: i32 = 1_000_000` at
  `crates/chess-ai/src/search/mod.rs:24`

Lives in chess-ai (not chess-web) because the conversion is engine
math — chess-tui calls the same function from the same crate. The
*data model* (`EvalSample`, the `Vec<>`, the per-ply ordering) lives
in each client because it's UI-shaped (see `clients/chess-web/src/eval.rs`
and `clients/chess-tui/src/eval.rs` — byte-identical structs by intent;
promoting them to a shared client crate is tracked in
`backlog/promote-client-shared.md`).

### 2.1 Formula

Standard chess-engine logistic, base 10:

```text
win_pct = 1 / (1 + 10^(-cp / WIN_PCT_K))
```

with `WIN_PCT_K = 400` borrowed from chess Elo convention (a 400 cp
advantage ≈ 91 % win rate). Worked example:

| cp (stm POV) | Raw logistic | Clamped output |
|---|---|---|
| `+800` | `0.99 +` | `0.99` (final clamp) |
| `+400` | `0.9091…` | `0.91` |
| `+200` | `0.7597…` | `0.76` |
| `+100` | `0.6395…` | `0.64` |
| `0` | `0.5` | `0.50` |
| `-100` | `0.3604…` | `0.36` |
| `-400` | `0.0909…` | `0.09` |
| `-800` | `0.01 −` | `0.01` (final clamp) |

The output is **side-to-move-relative**: a positive cp + the
returned `win_pct` always describe the side that's about to move.
Conversion to "Red wins %" (the chart Y-axis convention) happens at
the call-site via `stm_cp_to_red_win_pct` — see §3.

`cp_to_win_pct(cp)` is **monotonic non-decreasing** by construction
(test: `cp_to_win_pct_is_monotonic` at `crates/chess-ai/src/lib.rs:1276`),
**symmetric around 0** (`cp_to_win_pct(+x) + cp_to_win_pct(-x) ≈ 1.0`),
and **stays in `[0.01, 0.99]`** for any `i32` input including
`i32::MAX` / `i32::MIN` (test: `cp_to_win_pct_stays_in_clamp_range`
at `crates/chess-ai/src/lib.rs:1290`).

### 2.2 Two clamp tiers (`MATE` early-out + `WIN_PCT_CLAMP_CP` early-out + final logistic clamp)

The implementation has three protection layers stacked in one function
(`crates/chess-ai/src/lib.rs:398-418`):

```rust
pub fn cp_to_win_pct(cp: i32) -> f32 {
    // Tier 1: mate-band early-out (search returns mate-in-N as
    //         ±MATE - depth; anything within 1000 of MATE is "mate").
    if cp >= crate::search::MATE - 1000 || cp >= WIN_PCT_CLAMP_CP {
        return 0.99;
    }
    if cp <= -(crate::search::MATE - 1000) || cp <= -WIN_PCT_CLAMP_CP {
        return 0.01;
    }
    // Tier 2: real logistic.
    let exponent = -(cp as f32) / WIN_PCT_K;
    let raw = 1.0 / (1.0 + 10f32.powf(exponent));
    // Tier 3: final safety clamp catches f32 underflow (`raw == 0.0`)
    //         and overflow (`raw == 1.0`) in the long tail between
    //         ±2000 cp and the explicit early-out at ±25_000 cp.
    raw.clamp(0.01, 0.99)
}
```

Each tier protects the chart against a different failure mode:

| Tier | Triggered by | Without it you'd see |
|---|---|---|
| 1a (`±(MATE - 1000)`) | Search reports mate-in-N (e.g. `999_999`, `999_998`, …) | `10f32.powf(-2500)` underflows to `0.0`; chart line touches the literal floor of the SVG strip and visually "disappears" off the bottom edge |
| 1b (`±WIN_PCT_CLAMP_CP`) | v3+ casual mode general-capture eval (`KING_VALUE = 50_000`); see §2.4 | Same underflow class as above, just at a smaller magnitude |
| 3 (`raw.clamp(0.01, 0.99)`) | Long-tail logistic past ~±2000 cp | Same — float underflow lands at exactly `0.0` / `1.0` instead of `0.01` / `0.99`, which the SVG renderer treats as "off the strip" |

Tier 3 is functionally redundant with the other two for any sane cp
input (the logistic itself saturates well before `±WIN_PCT_CLAMP_CP`
— at ±400 cp it's already 91 % / 9 %, at ±2000 cp it's essentially
100 % / 0 % on f32), but it's cheap insurance against future search
changes that might emit cp values in the gap between ±2000 and
±25_000 cp without going through the early-out.

### 2.3 Why `[0.01, 0.99]` and not `[0.0, 1.0]`

Three reasons for the asymmetric clamp band:

1. **SVG strip rendering**: the eval bar (now removed — see §7.3 — but
   conceptually still relevant for `<EvalChart>`) maps the value
   linearly to a `y` percentage inside an SVG element. A literal `0.0`
   or `1.0` puts the marker at the exact pixel boundary; many browsers
   draw 0-width strokes oddly there or anti-alias them off the visible
   strip entirely. Pinning the extremes 1 % inside the strip
   guarantees the chart line / bar marker is always *visible*.
2. **Visual humility**: even mate-in-1 isn't logically certain (the
   moving side could play a non-mate move) and even a quiet 0-cp
   position isn't truly 50/50 (one side is always slightly favoured by
   tempo). Reserving the literal endpoints respects the
   "no-100%-confidence-from-eval-alone" convention used by
   chess.com / lichess.
3. **End-of-game pin escape hatch**: when the game **definitively** ends
   we *do* want to display the literal outcome — but `cp_to_win_pct`
   is no longer the right function call for that. Instead `EvalSample::final_outcome`
   constructs the sample directly with `red_win_pct = 0.99` (winner)
   or `0.5` (draw) bypassing the logistic entirely. See §5. Note that
   even the pin uses `0.99` not `1.0` to keep the chart-render
   invariant simple — anything beyond the strip is implicitly "off
   the chart".

Test: `cp_to_win_pct_stays_in_clamp_range` at
`crates/chess-ai/src/lib.rs:1290` (proves invariant for
`[i32::MIN, -100_000, -10_000, 0, 10_000, 100_000, i32::MAX]`).

### 2.4 Why two early-out tiers (mate-in-N + KING_VALUE swing)

The two early-out conditions cover **structurally different** signals
from the search:

**Mate-in-N (`|cp| >= MATE - 1000`)** — when the search finds a forced
mate, it returns `±MATE - distance_to_mate` so a depth-N search reports
"mate in N" as `MATE - N` cp (positive for the side delivering mate).
`MATE = 1_000_000` and the threshold band of 1000 covers up to mate-in-1000
(realistically anything > 50 is academic; v5 with default node budget
maxes at depth ~9 so mate-in-9 is the practical ceiling).

**KING_VALUE swing (`|cp| >= WIN_PCT_CLAMP_CP = 25_000`)** — v3+
casual-mode evaluator (`crates/chess-ai/src/eval/material_king_safety_pst_v3.rs`)
adds `KING_VALUE = 50_000` cp for the side whose general is on the
board. In **casual** rules ("no terminal status until the general is
*physically* captured", as opposed to legal-checkmate detection in
strict mode) the eval can swing by ±50_000 cp on the move that
captures the general. Without this early-out, that swing would
produce a logistic input of `±50_000 / 400 = ±125`, and `10^(-125)`
underflows to literal `0.0` on f32.

The threshold of 25_000 is `KING_VALUE / 2` — chosen so that
"general is *almost* captured" positions (where the eval might
report ±26_000 cp because of an exchange in flight) clamp the same
way as completed captures. Picked at half rather than at exactly
`KING_VALUE` so the boundary is *inside* the band, not on its edge,
making the early-out robust against minor eval drift in future
strategy versions. (See `crates/chess-ai/src/lib.rs:352-363` for the
full rationale comment.)

Why both tiers and not just one big `WIN_PCT_CLAMP_CP = MATE / 2`?
**Code-as-documentation**: the two early-outs name two *different*
classes of "the eval is past the chart's resolution" — search-side
mate scoring vs eval-side king-loss bookkeeping. A single combined
threshold would obscure that. The runtime cost is one extra `i32`
comparison per call, irrelevant against the analyse cost upstream.

Test: `cp_to_win_pct_mate_clamps_to_1_or_99` (`crates/chess-ai/src/lib.rs:1221`)
exercises the mate-band path; `cp_to_win_pct_king_loss_clamps_to_1_or_99`
(`crates/chess-ai/src/lib.rs:1252`) exercises the KING_VALUE-band
path including the gap between the two early-outs.

## 3. Side-of-board POV convention: always Red

The Y axis on every UI surface is **Red wins %** (top of the chart =
"Red is winning", bottom = "Black is winning"; sidebar badge always
puts the Red-side number first). This was a deliberate design choice
during the initial ship (see `backlog/ai-winrate-display.md` §"Y-axis
POV: always Red"):

- Matches xiangqi tradition (Red is the bottom seat conventionally).
- Consistent across **all** game modes — vs-AI, PvP, future spectator
  net mode — so a screenshot's interpretation doesn't depend on
  "whose turn was it when this was captured?".
- Lets a single colour scheme work for the chart line, the gradient
  bar, and the sample dots (red gradient up, black gradient down,
  midline reference at 50 %).

The cost: every cp value coming out of `chess_ai::analyze` is in the
**side-to-move's** POV (positive = "side to move is winning"), so a
negation step is required when the side to move is Black. That step
is `stm_cp_to_red_win_pct` — see §3.1.

### 3.1 `stm_cp_to_red_win_pct` — the negation rule

`clients/chess-web/src/eval.rs:115`:

```rust
pub fn stm_cp_to_red_win_pct(cp_stm_pov: i32, side_to_move: Side) -> f32 {
    let stm_pct = chess_ai::cp_to_win_pct(cp_stm_pov);
    match side_to_move {
        Side::RED => stm_pct,
        _ => 1.0 - stm_pct,
    }
}
```

The chess-tui mirror at `clients/chess-tui/src/eval.rs:28-35` inlines
the same logic into `EvalSample::new` (no separate helper).

The negation rule:

| `cp_stm_pov` | `side_to_move` | `stm_pct` (chess-ai) | `red_win_pct` (Red POV) |
|---|---|---|---|
| `+300` | Red | `0.85` | `0.85` (Red is +300, Red is winning) |
| `+300` | Black | `0.85` | `1 − 0.85 = 0.15` (Black is +300 = Red is losing) |
| `-300` | Red | `0.15` | `0.15` (Red is -300, Red is losing) |
| `-300` | Black | `0.15` | `1 − 0.15 = 0.85` (Black is -300 = Red is winning) |
| `0` | either | `0.50` | `0.50` |

Test: `red_win_pct_symmetric_across_side_negation` at
`clients/chess-web/src/eval.rs:158` pins the diagonal symmetry
(`stm_cp_to_red_win_pct(+300, RED) == stm_cp_to_red_win_pct(-300, BLACK)`)
so a future refactor that flips a sign breaks loudly.

### 3.2 Why `EvalSample.red_win_pct` is precomputed at sample time

`EvalSample` (`clients/chess-web/src/eval.rs:36`) carries the field
`red_win_pct: f32` even though it could be re-derived from
`(cp_stm_pov, side_to_move_at_pos)` on every render:

```rust
pub struct EvalSample {
    pub ply: usize,
    pub side_to_move_at_pos: Side,
    pub cp_stm_pov: i32,
    pub red_win_pct: f32,         // <-- redundant with the three above
}
```

Three reasons for the redundancy:

1. **Render-time cheapness**: the chart redraws every frame during
   hover-tooltip animation. Pre-computing the percentage avoids a
   `powf` per sample per frame on every chart redraw.
2. **End-of-game pin asymmetry**: `EvalSample::final_outcome` (§5)
   sets `red_win_pct` to `0.99` / `0.5` directly, **bypassing the
   logistic clamp**. If the percentage were derived from `cp_stm_pov`
   on read, the override couldn't reach exactly `0.99` for a draw
   (since `cp_stm_pov = 0` for a draw must logically map to 50 %, but
   for a win we want to override the % away from what cp would give).
   The separate field gives the constructor full control.
3. **Wire-shape compatibility**: when the future net protocol v6
   broadcasts pre-computed samples (see `backlog/chess-net-evalbar.md`),
   the wire schema *must* carry the explicit % — server-side analyse
   may use a different K than client-side rendering wants. Carrying
   the field locally first means the eventual wire format is a
   straightforward serialisation of the existing struct.

To keep the two fields *consistent*, the `final_outcome` constructor
sets `cp_stm_pov` to `±30_000` (sentinel landing inside the
`WIN_PCT_CLAMP_CP` early-out band) so a defensive consumer that
*does* re-derive from cp gets the same answer to two decimals.
Pinned by the test
`final_outcome_red_wins_pins_red_high` at
`clients/chess-web/src/eval.rs:213` (asserts
`(stm_cp_to_red_win_pct(s.cp_stm_pov, s.side_to_move_at_pos) - s.red_win_pct).abs() < 0.05`).

### 3.3 Banqi's third colour (Green) — 50/50 fallback

`chess-core` defines `Side` with three variants (`Red`, `Black`,
`Green`) to leave room for the experimental three-kingdom variant.
chess-ai is xiangqi-only so this code path is unreachable today, but
the conversion helpers are defensively coded for it:

- `stm_cp_to_red_win_pct` (`clients/chess-web/src/eval.rs:115`):
  the `_ => 1.0 - stm_pct` arm catches `Side::GREEN`. This treats
  Green-to-move as Black-to-move for the purposes of "if Green is
  +300, then Red is -300 from Red's POV" — defensible because Red is
  the chart axis and Green is non-Red.
- `EvalSample::final_outcome` (`clients/chess-web/src/eval.rs:88` /
  `clients/chess-tui/src/eval.rs:58`): the `(_, _) => (0, 0.5)` arm
  in the `match (winner, side_to_move)` block treats a
  `Won { winner: Side::GREEN, .. }` outcome as 50/50 on the Red-vs-Black
  axis — the end-game banner still announces "Green wins" correctly,
  but the chart's last point lands at the midline rather than
  pretending Red won.

If three-kingdom ever gets a real AI port, this needs revisiting —
the right model is probably a per-side win-% triple
`(red_pct, black_pct, green_pct)` rather than a single Red-axis
value. Tracked implicitly in the `three-kingdoms-banqi.md` backlog
entry.

## 4. `EvalSample` lifecycle

A sample is appended to the per-game `Vec<EvalSample>` whenever a
fresh `chess_ai::analyze` result lands for a position. The vector is
**indexed by ply** (sample at index `N` describes the position **after**
ply `N` was played; `ply == 0` is the initial position — not currently
sampled in either client, but the convention reserves the slot).

The vector is reset on:

- **New game** — `eval_samples.set(Vec::new())` in
  `clients/chess-web/src/pages/local.rs:502`
- **Variant switch** — same path, `clients/chess-web/src/pages/local.rs:515`
- **Undo** — chess-tui `app.rs:1862` truncates `eval_samples.retain(|s|
  s.ply <= new_len)`; chess-web's reactive history derive
  (`pages/local.rs:829`) implicitly truncates by re-pairing
  `samples[ply-1]` with `samples[ply]` and dropping orphaned tail
  entries from the visible derived signal (the underlying
  `eval_samples` vector is itself untouched by undo on web — minor
  inefficiency, not a correctness issue, since New Game / variant
  switch zeros it).

### 4.1 Three sample-write sites

| Site | Trigger | File:line | What gets written |
|---|---|---|---|
| **AI move pump** (vs-AI mode) | After the AI's `analyze + make_move` | `clients/chess-web/src/pages/local.rs:624-631`; chess-tui `app.rs:2125` | `EvalSample::new(new_ply, new_stm, -ai_pov_score)` — see §4.3 for the negation |
| **Hint pump** (PvP + evalbar, or vs-AI when human is to move) | After the snapshot's `analyze` completes | `clients/chess-web/src/pages/local.rs:756-764`; chess-tui `app.rs:2010` (via `run_pvp_eval`) | `EvalSample::new(snapshot.history.len(), snapshot.side_to_move, a.chosen.score)` — see §4.4 for why no negation |
| **End-of-game pin** | `Ongoing → Won/Drawn` transition | `clients/chess-web/src/pages/local.rs:793-805`; chess-tui `app.rs:2019-2021` (`record_eval_final`) | `EvalSample::final_outcome(ply, stm, &status)` — bypasses logistic, see §5 |

The AI pump is **free**: the analyse already ran for move selection,
the sample just records its by-product. The hint pump in PvP +
evalbar mode pays the full cost of an extra synchronous analyse
(~10–300 ms native, ~0.5–3 s WASM at default Hard depth), but the
pump runs at most once per ply and the user explicitly opted in via
`?evalbar=1` / `--evalbar`. In vs-AI mode the hint pump *also* runs
on the human's turn (so the chart has data points for "what was the
position right before I moved?") — same cost shape, same opt-in.

The hint pump's gate is **`hints_enabled || evalbar_enabled`**
(`clients/chess-web/src/pages/local.rs:658`) so the same analyse
serves both features. Without that "or", users with `?evalbar=1` but
not `?hints=1` would silently get no samples.

### 4.2 `push_or_replace_sample` — dedup by ply

Both web pumps go through `push_or_replace_sample` at
`clients/chess-web/src/eval.rs:140`:

```rust
pub fn push_or_replace_sample(samples: RwSignal<Vec<EvalSample>>, sample: EvalSample) {
    samples.update(|v| {
        if let Some(idx) = v.iter().position(|s| s.ply == sample.ply) {
            v[idx] = sample;
        } else {
            v.push(sample);
        }
    });
}
```

Why dedup-by-ply rather than always-push:

- vs-AI mode produces samples from **two** sources for adjacent plies:
  the AI move pump records ply N's sample at the moment it plays the
  N-th move, and the hint pump (if also enabled) records ply N's
  sample again ~100 ms later from a fresh analyse during the human's
  thinking time. Without dedup, the vector would grow non-monotonically
  in ply count and the chart's per-sample-position calculation
  (`sample_to_xy` in `clients/chess-web/src/components/eval_chart.rs:162`)
  would space the dots wrong.
- The end-of-game pin **must** replace any pre-existing sample at the
  same ply (the per-move pump may have just landed a stale "Red 10 %"
  for that ply before the game ended — see §5).
- The "freshest analyse wins" semantics matters when one pump used a
  shallower search (e.g. quick eval at depth 1) and the other used
  Hard at depth 4 — both correctly pair `(ply, stm) ↔ cp`, but the
  deeper one's number is more trustworthy.

chess-tui's `record_eval_sample` (`clients/chess-tui/src/app.rs:2043`)
does the opposite — **skips** if a sample already exists for that ply
— and `record_eval_final` (`app.rs:2061`) explicitly replaces. The
asymmetry exists because chess-tui's PvP pump runs synchronously in
the same event-loop tick as `apply_move`, so there's no race window
where the AI pump and a separate hint pump could disagree on `(ply,
cp)`. The web's `push_or_replace_sample` handles the more general
case where two async tasks may write the same ply concurrently.

### 4.3 The AI move pump's `-ai_pov_score` negation

`clients/chess-web/src/pages/local.rs:624-631`:

```rust
if evalbar_enabled {
    let new_ply = state.with_untracked(|s| s.history.len());
    let new_stm = state.with_untracked(|s| s.side_to_move);
    push_or_replace_sample(
        eval_samples,
        EvalSample::new(new_ply, new_stm, -ai_pov_score),  // <-- minus
    );
}
```

Why the `-`:

1. `chess_ai::analyze` returns `chosen.score` from the **side-to-move's**
   POV at the position it analysed. When the AI is to move, "side to
   move" is the AI; positive `chosen.score` means "the AI is winning".
2. The pump runs **after** `make_move()`, which flips `side_to_move` to
   the opponent (the human). So `new_stm` is the human, and a sample
   with `cp_stm_pov = +ai_pov_score` would lie — it would say "the
   human is winning by `ai_pov_score`" when really the AI is.
3. Negating `ai_pov_score` re-expresses the same evaluation in the
   *new* side-to-move's POV: "the human is losing by `ai_pov_score`".
4. `EvalSample::new` then runs `stm_cp_to_red_win_pct(-ai_pov_score,
   new_stm)` (§3.1), which converts to the Red-axis value the chart
   wants.

chess-tui's mirror at `clients/chess-tui/src/app.rs:2125`:

```rust
// negamax's `score` is from the AI's POV after the move it just
// played; the new `side_to_move` is the opponent, so to express the
// sample in stm-relative cp we negate.
Self::record_eval_sample(g, -score);
```

Same logic, same negation.

### 4.4 The hint pump's `+a.chosen.score` (no negation)

`clients/chess-web/src/pages/local.rs:756-764`:

```rust
if evalbar_enabled {
    push_or_replace_sample(
        eval_samples,
        EvalSample::new(
            snapshot.history.len(),
            snapshot.side_to_move,
            a.chosen.score,                  // <-- NO negation
        ),
    );
}
```

The hint pump captures `snapshot` (the state it analysed) **before**
spawning the analyse task. The snapshot's `side_to_move` is the side
the analyser scored — the same side the returned `chosen.score` is in
the POV of. No negation needed: `(snapshot.side_to_move,
a.chosen.score)` is already a self-consistent `(stm, cp_stm_pov)`
pair.

Contrast with the AI pump: the AI pump wrote the sample for the
**post-move** position (`new_stm = opponent`), so it had to flip the
sign to undo the `make_move`'s side-flip. The hint pump writes for
the **pre-move** position (the snapshot) so no flip is needed.

This asymmetry is the most error-prone part of the whole system —
the original `b195054` ship had a different bug class here (a
reactive effect that watched both pumps' analyses *and* state, then
tried to pair them at effect-fire time). That race is the subject of
§7.3 and `pitfalls/eval-sample-stale-analysis-race.md`. The current
"each pump owns its own sample write at the moment its analyse
completes" model is the post-fix design.

## 5. End-of-game pin (`EvalSample::final_outcome`)

The third sample-write site is special enough to deserve its own
section. Code: `clients/chess-web/src/eval.rs:88` /
`clients/chess-tui/src/eval.rs:58`.

### 5.1 The bug it fixes

User feedback (2026-05-12, fixed in `44885bb`):

> 紅方剛吃掉黑將，VICTORY 橫幅顯示「紅方獲勝 — 將被吃」，但 sidebar
> 的勝率還停在「紅 10%」。

Both clients' per-move sample writers (web's AI pump + hint pump,
TUI's `apply_move` + `ai_reply` + `execute_resign`) **bail when
`state.status != Ongoing`** — they don't analyse a finished position.
So the **last** sample they recorded is from the position **before**
the game-ending move:

- Black's analyser at ply N-1 thought it was doing great (Red had
  walked into a losing trap; Black's POV cp = +400, Red-axis 9 %).
- Red found the only escape — a cannon-jump that captures Black's
  general — at ply N.
- The status flips to `Won { winner: Red, reason: GeneralCaptured }`.
- The per-move pumps see `status != Ongoing` and bail. The chart's
  last point stays at "Red 9 %" even though Red just won.

Visually: VICTORY banner says "紅方獲勝", sidebar badge says "紅 9% • 黑
91%", chart line ends well below the midline. The user (correctly)
filed it as a bug.

### 5.2 Bypass logistic; pin to outcome

```rust
pub fn final_outcome(ply: usize, side_to_move: Side, status: &GameStatus)
    -> Option<Self>
{
    const TERMINAL_CP: i32 = 30_000;
    let (cp_stm_pov, red_win_pct) = match status {
        GameStatus::Ongoing => return None,
        GameStatus::Drawn { .. } => (0, 0.5),
        GameStatus::Won { winner, .. } => match (*winner, side_to_move) {
            (Side::RED, Side::RED) => (TERMINAL_CP, 0.99),
            (Side::RED, _)         => (-TERMINAL_CP, 0.99),
            (Side::BLACK, Side::BLACK) => (TERMINAL_CP, 0.01),
            (Side::BLACK, _)           => (-TERMINAL_CP, 0.01),
            (_, _) => (0, 0.5),  // Banqi 3rd colour fallback (§3.3)
        },
    };
    Some(Self { ply, side_to_move_at_pos: side_to_move, cp_stm_pov, red_win_pct })
}
```

Key design points:

- **Returns `Option<Self>`** so the caller can use it from inside an
  `Ongoing → Won/Drawn` reactive effect without a separate guard.
  `None` for `Ongoing` means "no terminal sample to record"; the
  callers are paranoid enough to pass `state.status` rather than asking
  the helper to assume `!Ongoing`.
- **Sets `red_win_pct` directly** — does NOT call `cp_to_win_pct`.
  This is the only path that lands a sample with `red_win_pct ==
  0.99` for a winning side regardless of the cp magnitude. The
  in-game pump samples can't reach 0.99 even at mate-in-1 because
  `cp_to_win_pct` clamps to 0.99 — but the `final_outcome` value is
  *guaranteed* 0.99 because it's hard-coded.
- **The `(winner, side_to_move)` match** answers a subtle question:
  in a `Won { winner: Red, .. }` outcome, who is "the side to move"?
  It's the **loser** — the side that has no legal reply (in casual
  mode, the side whose general was just captured, which means it's
  conceptually their turn to "do something" but they can't). The
  match arms cover both possibilities so the cp sentinel sign is
  always correct from the loser's POV (loser-to-move with negative cp
  means Red won; loser-to-move with positive cp means Black won).
  Tests cover both `stm = Red` and `stm = Black` cases for
  `winner = Red` (`final_outcome_red_wins_pins_red_high` at
  `clients/chess-web/src/eval.rs:213`).

Wiring:

- **chess-web** (`pages/local.rs:793-805`): a dedicated
  `create_effect` watches `state.status`; when it transitions from
  `Ongoing` to anything else, it builds and pushes the
  `final_outcome` sample. Tracked-state initial value is `prev: false`,
  so the effect fires once per game-end transition (and not on
  re-renders that don't change status). New Game / Undo zeroes
  `eval_samples` so the effect can fire again on the next game.
- **chess-tui** (`app.rs:2019-2021`): `record_eval_final` is called
  at the **end of `apply_move`** whenever evalbar is on AND the
  resulting status is non-Ongoing. Same idempotent semantics — the
  sample either replaces (if a per-move sample was already pushed
  for this ply) or appends (if not).

### 5.3 The `±30_000 cp` sentinel

`TERMINAL_CP = 30_000` is a magic number. Why specifically 30_000?

- Must land **inside** `cp_to_win_pct`'s explicit early-out band
  (`|cp| >= WIN_PCT_CLAMP_CP = 25_000`) so a downstream consumer that
  re-derives `red_win_pct` from `(cp_stm_pov, side_to_move_at_pos)`
  gets the same answer to two decimals. `30_000 > 25_000` ✓.
- Must **not** be in the `MATE` band (`|cp| >= MATE - 1000 = 999_000`)
  because conceptually the game just ended in **a real position with
  a real eval**, not in a search-discovered mate. Using a value in
  the KING_VALUE band rather than the MATE band keeps the semantic
  distinction clean for any future debug tooling that distinguishes
  "search said mate" from "rules said game over". `30_000 < 999_000` ✓.
- Must comfortably exceed the realistic eval magnitude of any
  *non-terminal* position so a debugging session that prints
  `cp_stm_pov` for every sample doesn't have terminal samples
  visually mixing with mid-game samples. Realistic mid-game cp values
  are O(1000); v3+ casual mode king-loss positions can briefly reach
  ±50_000 cp (search horizon-effect during the captured ply itself);
  `30_000` sits cleanly above the mid-game range without colliding
  with `KING_VALUE` exactly.

`30_000` is local to `final_outcome` (one constant per client mirror,
not exported). If you ever change `WIN_PCT_CLAMP_CP` you must keep
`TERMINAL_CP > WIN_PCT_CLAMP_CP` — but the consistency-test in
`final_outcome_red_wins_pins_red_high` (`clients/chess-web/src/eval.rs:230-236`)
asserts the re-derivation invariant so a wrong value would fail loudly:

```rust
let derived = stm_cp_to_red_win_pct(s.cp_stm_pov, s.side_to_move_at_pos);
assert!(
    (derived - s.red_win_pct).abs() < 0.05,
    "cp-derived {} should match red_win_pct {}",
    derived, s.red_win_pct,
);
```

## 6. UI surfaces

The same `Vec<EvalSample>` feeds three different UI shapes per client.
This section is a quick reference for finding the rendering code; the
*data* path (sample writes) is the same regardless of UI.

### 6.1 chess-web

| Component | File | Mounted by | Reads |
|---|---|---|---|
| `<EvalBadge>` (sidebar `紅 % • 黑 %` row + gradient bar) | `clients/chess-web/src/components/sidebar.rs:190` (`fn EvalBadge`) | `<Sidebar>` when `eval_badge: Option<Signal<...>>` is `Some` | `current_eval` (last sample) |
| `<EvalChart>` (post-game line chart) | `clients/chess-web/src/components/eval_chart.rs:30` | `pages/local.rs:987` inside an `<Show when=chart_show>` (chart_show = `evalbar_enabled && !samples.is_empty()`) | `eval_samples_signal` (full vector) |
| `<MoveHistory>` row delta `+5%` / `-12%` | `clients/chess-web/src/components/move_history.rs:91` (delta_view) | Always present in `<Sidebar>`; delta is `None` when evalbar off | `HistoryEntry.eval_delta_pct` (computed in `pages/local.rs:829-855` by pairing `samples[ply-1]` with `samples[ply]`) |

The vertical eval bar (`<EvalBar>`) that originally shipped in
`b195054` was **removed** in `fc51f24` after iPad user feedback —
see §7.3. The sidebar `<EvalBadge>` is now the single live-display
surface during play; the `<EvalChart>` is the post-game surface.

URL gate: `?evalbar=1` (`clients/chess-web/src/routes.rs:174`,
`253`). Picker checkbox at `clients/chess-web/src/pages/picker.rs:237`
("📊 Win-rate display" — the bilingual emoji label is intentional;
the picker UI uses emoji throughout for visual scanability).

CSS surfaces (no logic, but useful when chasing visual bugs):
`.eval-badge*` and `.eval-chart*` rule families in
`clients/chess-web/style.css`.

### 6.2 chess-tui

| Surface | File:fn | Trigger |
|---|---|---|
| Sidebar `Eval: 紅 ████░░░░ 黑 (52%)` headline | `clients/chess-tui/src/ui.rs:1492` (`push_eval_headline`) | Always shown when `--evalbar` is on, regardless of `evalbar_open` |
| ASCII trend chart (one row per ply, last 24 plies) | `clients/chess-tui/src/ui.rs:1549` (`push_eval_chart_lines`) | Shown when `--evalbar` AND `evalbar_open` (toggled by `E` key) |

Key wiring:

- CLI flag: `--evalbar` (parsed in `clients/chess-tui/src/main.rs`,
  threads into `AppState.evalbar_enabled` at `app.rs:497`)
- `E` key handler: `clients/chess-tui/src/input.rs` → `Action::EvalbarToggle`
  → `app.rs:1066` (toggles `evalbar_open` with helpful messages when
  evalbar isn't enabled or no samples exist yet)
- Help text: `clients/chess-tui/src/ui.rs:2643` ("E — toggle AI
  win-rate trend chart (requires --evalbar)")

The ASCII chart renders one row per ply with the bar position
proportional to `red_win_pct`. Row colour reflects the swing direction
(`ui.rs:1616-1620`): red if `red_win_pct >= 0.55`, black if `<= 0.45`,
neutral otherwise. The headline always shows, the chart toggles —
matches the web's "live badge always, post-game chart only when
asked" pattern but compressed into a single text-mode panel.

## 7. Bug history (chronological)

Four commits, all on `main`, all in three days. The conversion math
(`cp_to_win_pct`) was right on the first try; every subsequent fix
was about *when* and *which sample* gets pushed.

### 7.1 `b195054` (2026-05-10) — initial ship

> `feat: AI win-rate display — eval bar + sidebar badge + per-ply
> samples + post-game trend chart (web local + chess-tui)`

Single PR, +1937/-45 LOC across 21 files. Shipped:

- `chess-ai`: `cp_to_win_pct(cp: i32) -> f32` + `WIN_PCT_K = 400`
  + `WIN_PCT_CLAMP_CP = 25_000` + 6 unit tests covering monotonicity,
  mate clamp, KING_VALUE clamp, range invariants.
- `chess-web`: `?evalbar=1` URL flag + picker checkbox; new `eval.rs`
  module with `EvalSample` struct; **two** SVG components shipped
  initially — `<EvalBar>` (vertical strip attached right of the
  board inside a new `.board-pane__row` flex wrapper) AND `<EvalChart>`
  (line chart); sidebar `<EvalBadge>`; `<MoveHistory>` per-row
  delta annotation.
- `chess-tui`: `--evalbar` flag + mirror `EvalSample` struct + `E`
  key toggle + `push_eval_headline` / `push_eval_chart_lines`.

Sample-write path on this version: a single `create_effect` watched
`(state, debug_analysis, hint_analysis)` and pushed a sample whenever
all three were available and a sample for the current ply didn't yet
exist. This was the source of the bug fixed in §7.3.

Design record: `backlog/ai-winrate-display.md`.

### 7.2 `0741915` (2026-05-10) — eval-bar layout breakage (CSS only, no math change)

> `fix(chess-web): move eval bar out of .board-pane to fix layout
> breakage + unclickable bottom-row pieces`

iPad screenshot reported by the user: captured strip overlapping the
bottom-row pieces, board appearing shifted, some pieces unclickable.

Root cause: the new `.board-pane__row` flex-row wrapper around
`<Board>` + `<EvalBar>` interacted badly with the column-flex sizing
of `.board-pane`. The SVG board with `preserveAspectRatio="xMidYMid
meet"` got letterboxed inside a row container, leaving empty
letterbox space inside the row that the layout below didn't account
for. Click hit-testing on cells near the letterbox boundary became
unreliable since the SVG element extended past the visible board.

Fix: relocated `<EvalBar>` out of `.board-pane` into a new
top-level grid column on `.game-page--with-evalbar`. Pure CSS +
template restructure; no change to `cp_to_win_pct`, `EvalSample`, or
the sample-write paths.

This fix was **superseded** the next day by `fc51f24` which removed
`<EvalBar>` entirely (see §7.3) — at that point the layout work
became moot but the descendant-selector revert in this commit
remained relevant for net mode.

### 7.3 `fc51f24` (2026-05-11) — four bugs from iPad testing

> `fix(chess-web): four win-rate / threat-overlay bugs from user iPad testing`

Bundled fix for four user-reported issues. The first is a tangential
threat-overlay bug; the rest are win-rate.

**Bug 1 (threat overlay, mentioned for completeness)**: Threat
highlight = Off but rings still showed on the board. Hover-preview
was wired independently from the threat mode selector. Fix: gate
`hover_threat_squares` on `mode != ThreatMode::Off`. No win-rate
impact.

**Bug 2: Two duplicate win-rate displays**. Vertical eval bar AND
sidebar badge both visible — visually redundant and (per `0741915`)
the bar required tricky layout work to not break the board.
Resolution: keep the sidebar badge for layout consistency
regardless of `?evalbar=1`; remove `<EvalBar>` entirely. Removed:
`components/eval_bar.rs` (-112 LOC), its `mod.rs` entry, the
`.game-page--with-evalbar` 3-column grid modifier, ~80 lines of
`.eval-bar*` CSS.

**Bug 3: Empty `紅 — • 黑 —` badge when evalbar disabled**. The
sidebar always rendered `<EvalBadge>` because `current_eval` was
unconditionally passed; the badge fell back to "—" placeholders
when no samples existed. Fix: pass `eval_badge: None` to the
sidebar when `evalbar_enabled` is false (`pages/local.rs:882-883`),
relying on the sidebar's existing `.map(|sig| ...)` short-circuit
to omit the badge entirely.

**Bug 4 (the critical math fix): Win-rate didn't reach ~100% even
at mate-in-1**. Root cause: stale-analysis race in the sample-write
effect. The previous design (`b195054`) watched `(state,
debug_analysis, hint_analysis)` reactively. When `state` changed (a
move was played), the effect fired immediately with the **OLD**
analysis (computed for the previous position) but the **NEW**
`state.history.len()`. The sample landed at the correct ply slot
but carried the wrong cp. The dedup-by-ply guard then prevented
the fresh analysis (when it landed ~100 ms later) from updating
the slot.

Fix: write samples **directly** from each pump after the analysis
completes, using the position info captured at analyze-time. New
helper `push_or_replace_sample` (§4.2) replaces the always-skip
guard with replace-by-ply so later analyses *can* refine the slot.

The reactive sample-write effect was removed entirely. The hint
pump and AI move pump now own their sample writes (§4.1), each
scoped to the moment they have fresh, paired `(position, cp)`.

Class-of-bug captured in
`pitfalls/eval-sample-stale-analysis-race.md` — see that doc for
the verbatim symptom + grep-friendly root cause writeup.

### 7.4 `44885bb` (2026-05-12) — game-end pin

> `fix(chess-web,chess-tui): pin win-rate to outcome on game-end +
> default Threat highlight to Off`

User report: red won by general capture, victory banner mounted,
sidebar still showed "紅 10 %". The threat-default tweak in this
commit is unrelated to win-rate; the win-rate part is the
`EvalSample::final_outcome` helper covered in §5.

The root cause was structural: every per-move sample writer bails
when `status != Ongoing` (so they don't analyse a finished position),
but neither the spec nor the implementation had defined what should
happen on the **transition** from Ongoing to a terminal state.

Fix shipped in three pieces:

1. `EvalSample::final_outcome(ply, stm, status) -> Option<Self>` —
   constructor that bypasses `cp_to_win_pct` and pins `red_win_pct`
   directly to `0.99` / `0.01` / `0.5` based on the outcome (§5.2).
2. **chess-web** (`pages/local.rs:793-805`): a new `create_effect`
   detects the `Ongoing → ended` transition and pushes the terminal
   sample.
3. **chess-tui** (`app.rs:2019-2021` + new `record_eval_final` at
   `app.rs:2061`): same logic at the end of `apply_move` and
   `ai_reply` whenever the resulting status is non-Ongoing.

Three new tests cover the three terminal branches:
`final_outcome_none_when_ongoing`, `final_outcome_red_wins_pins_red_high`,
`final_outcome_draw_is_50_50` (`clients/chess-web/src/eval.rs:202-252`).

This is the **current shipped behaviour** as of 2026-05-12.

## 8. Calibration: why `WIN_PCT_K = 400` is provisional

The `400` constant is borrowed verbatim from chess Elo convention,
where it derives from the formula relating Elo difference to score
expectation. The "right" K for **xiangqi** is unknown:

- Chess and xiangqi have different material distributions: xiangqi
  has no queen (the chess piece that dominates Elo studies), the
  chariot (車) is proportionally more important than rook, the
  cannon (炮) is positional in a way no chess piece is, advisors and
  elephants are restricted to the palace / own half.
- The cp scale itself differs: chess engines centipawn-anchor on the
  pawn (=100); chess-ai's evaluator uses pawn=100 too (`crates/chess-core/src/eval/see.rs`)
  but the relative weights of other pieces differ from chess.
- "Winning by 400 cp" in xiangqi may translate to a different actual
  win-probability than in chess.

### Calibration methodology

Tracked as a `[?/S]` follow-up TODO ("chess-ai: cp→win% calibration
for xiangqi"). The method:

1. Run a self-play tournament using `chess-ai/tests/perf.rs` fixtures
   at fixed depth (e.g. Hard/v5/depth 4, `Randomness::STRICT` for
   determinism). Bake N games per opening fixture so each game ends
   in a definitive outcome.
2. For each ply of each game, record `(cp_after_move, eventual_winner)`
   — eventual_winner is known because the game has finished.
3. Bin samples by cp magnitude (e.g. buckets of 50 cp wide); compute
   the **empirical** win-fraction per bucket.
4. Fit `K` by maximum likelihood against the logistic
   `1 / (1 + 10^(-cp/K))`. Likely lands K somewhere in 200–600.

### Impact of getting K wrong

**Strictly UX, not correctness**:

- Wrong K just makes the eval bar swing too aggressively (K too small)
  or feel sluggish (K too large).
- All search behaviour is unchanged — `cp_to_win_pct` is consumer-side
  display only; the move selector never reads it.
- All clamp endpoints stay at `[0.01, 0.99]` regardless of K.

So this is genuinely a P3/S task — worth doing for polish, not
blocking anything.

### When the calibration result lands

Update `WIN_PCT_K` in `crates/chess-ai/src/lib.rs:350`. Update
the worked-example table in §2.1 of this doc with the new percentages.
Mention the fitted K + N-games + RMSE in this section as historical
record. No protocol bump needed (web and TUI both consume
`cp_to_win_pct` directly; net mode would need to use the same K both
sides, but that's covered by `backlog/chess-net-evalbar.md`'s
"server-side analyse" recommendation).

## 9. Test coverage map

The win-rate code is covered by **16 tests** across three crates plus
4 URL round-trip tests for the `?evalbar=1` flag. All run as part of
`cargo test --workspace` (no `#[ignore]` markers).

### chess-ai (`crates/chess-ai/src/lib.rs`)

Six unit tests for `cp_to_win_pct`:

| Test | Line | Pins |
|---|---|---|
| `cp_to_win_pct_zero_is_50_percent` | `1198` | 0 cp → 0.50 |
| `cp_to_win_pct_400cp_is_about_91_percent` | `1207` | ±400 cp → ~0.91 / 0.09; sums to 1.0 |
| `cp_to_win_pct_mate_clamps_to_1_or_99` | `1221` | `±(MATE-1)`, `±MATE` → 0.99 / 0.01 |
| `cp_to_win_pct_king_loss_clamps_to_1_or_99` | `1252` | At `±WIN_PCT_CLAMP_CP`, at `±KING_VALUE`, mid-tail (`±10_000`) all clamp |
| `cp_to_win_pct_is_monotonic` | `1276` | Logistic non-decreasing across cp range |
| `cp_to_win_pct_stays_in_clamp_range` | `1290` | `[i32::MIN .. i32::MAX]` always in `[0.01, 0.99]` |

### chess-web (`clients/chess-web/src/eval.rs`)

Six unit tests for `EvalSample` + helpers:

| Test | Line | Pins |
|---|---|---|
| `red_win_pct_symmetric_across_side_negation` | `158` | `(+300, RED)` ↔ `(-300, BLACK)` produce same Red % |
| `red_win_pct_50_50_when_even` | `173` | 0 cp → 0.50 regardless of side |
| `sample_red_win_pct_matches_helper` | `183` | `EvalSample::new` agrees with direct `stm_cp_to_red_win_pct` |
| `red_win_pct_mate_for_black_clamps_red_to_1pct` | `192` | Black mating → Red < 5 % |
| `final_outcome_none_when_ongoing` | `202` | Ongoing → `None` (no spurious sample) |
| `final_outcome_red_wins_pins_red_high` | `213` | Red wins → Red > 0.95 for both stm cases; cp re-derive matches |
| `final_outcome_draw_is_50_50` | `242` | Draw → 0.50 + cp_stm_pov = 0 |

(Web `eval.rs` test count is 7; the table above lists 7 entries — table
ordering matches file order.)

### chess-web routes (`clients/chess-web/src/routes.rs`)

Four round-trip tests for the `?evalbar=1` URL flag:

| Test | Line | Pins |
|---|---|---|
| `xiangqi_evalbar_round_trips_in_vs_ai` | `843` | `?evalbar=1` parsed and re-emitted in vs-AI mode URL |
| `xiangqi_pvp_evalbar_emitted_even_without_ai_mode` | `854` | PvP mode also gets the flag (mirrors the `?hints=1` PvP fix) |
| `xiangqi_evalbar_default_off_omitted` | `865` | Off by default; not emitted when not set |
| `xiangqi_evalbar_truthy_aliases_parse` | `873` | `1`/`true`/`on` all parse as true |

### chess-tui

No dedicated unit tests for the eval module today — the TUI mirror
struct is byte-identical to chess-web's by inspection, and the
chess-ai tests cover the conversion math both clients call into.
Adding cross-client agreement tests is tracked under
`backlog/promote-client-shared.md` (which would deduplicate the two
mirror structs and let one set of tests cover both).

## 10. Known limitations & future work

- **Net mode unsupported.** chess-net's `PlayerView` doesn't carry
  per-ply samples, and running client-side `chess_ai::analyze` on
  every `Update` for every spectator would waste CPU and produce
  inconsistent numbers per spectator. Tracked as P2/M
  `chess-net protocol v6 — broadcast win-rate samples + spectator-side
  eval bar`. Full design: `backlog/chess-net-evalbar.md`.

- **Banqi / three-kingdom unsupported.** chess-ai is xiangqi-only;
  the picker hides the win-rate display checkbox for non-xiangqi
  variants. The `final_outcome` Banqi/Green fallback (§3.3) is
  defensive scaffolding for a future port, not a working feature.

- **No `ply == 0` initial-position sample.** Neither client runs an
  opening-position analyse before the first move. The first sample
  always lands at `ply == 1` after the first move is played. The
  history-row delta annotation for `ply == 1` is therefore always
  `None` (since pairing requires `samples[ply-1]`). Adding a small
  "analyse on game start" hook in both clients would close this gap;
  not currently a TODO entry but the cost is a single analyse per
  new game, which is the same shape as PvP+evalbar's per-move cost.

- **Calibration is rough (K = 400 borrowed from chess Elo).** See §8
  for the methodology. Strictly UX, not correctness.

- **In-memory only.** Refresh / quit loses everything. PWA IndexedDB
  persistence (P3 in `TODO.md`) will bundle the sample vector into the
  same record so resumed games show the full pre-resumption trend chart.

- **Web's undo doesn't truncate `eval_samples`.** chess-tui truncates
  on undo (`app.rs:1862`); chess-web relies on the derived signal
  re-pairing dropping orphaned samples from the *visible* chart but
  the underlying `RwSignal<Vec<EvalSample>>` keeps them. Cosmetic
  inefficiency only — New Game zeroes the vector and the chart never
  shows beyond `state.history.len()` plies. Worth aligning when
  promoting to a shared `chess-client-shared` crate.

## 11. Cross-references (file → line)

Quick index for jumping to the relevant code from this doc.

### Conversion math (chess-ai)

- `crates/chess-ai/src/lib.rs:331-418` — `cp_to_win_pct` + `WIN_PCT_K`
  + `WIN_PCT_CLAMP_CP` + comment block explaining the design
- `crates/chess-ai/src/lib.rs:1194-1295` — six unit tests
- `crates/chess-ai/src/search/mod.rs:24` — `pub const MATE: i32 = 1_000_000`
  (referenced from the early-out)
- `crates/chess-ai/src/eval/material_king_safety_pst_v3.rs` —
  `KING_VALUE = 50_000` (the casual-mode swing the early-out exists for)

### Sample plumbing (chess-web)

- `clients/chess-web/src/eval.rs:36` — `EvalSample` struct
- `clients/chess-web/src/eval.rs:51` — `EvalSample::new`
- `clients/chess-web/src/eval.rs:88` — `EvalSample::final_outcome`
- `clients/chess-web/src/eval.rs:115` — `stm_cp_to_red_win_pct`
- `clients/chess-web/src/eval.rs:140` — `push_or_replace_sample`
- `clients/chess-web/src/pages/local.rs:179` — `eval_samples` signal allocation
- `clients/chess-web/src/pages/local.rs:502, 515` — vector reset on
  New Game / variant switch
- `clients/chess-web/src/pages/local.rs:609-631` — AI move pump's sample write
- `clients/chess-web/src/pages/local.rs:743-764` — hint pump's sample write
- `clients/chess-web/src/pages/local.rs:773-805` — comment + Ongoing→ended pin effect
- `clients/chess-web/src/pages/local.rs:829-855` — derived `HistoryEntry`
  with `eval_delta_pct`
- `clients/chess-web/src/pages/local.rs:879-885` — `current_eval` /
  `eval_samples_signal` / `sidebar_eval_badge` derived signals

### URL / picker plumbing (chess-web)

- `clients/chess-web/src/routes.rs:174` — `LocalRulesParams.ai_evalbar`
- `clients/chess-web/src/routes.rs:253` — `?evalbar` parsing
- `clients/chess-web/src/routes.rs:337` — `?evalbar=1` emission
- `clients/chess-web/src/routes.rs:843-893` — round-trip tests
- `clients/chess-web/src/pages/picker.rs:237` — picker checkbox

### UI components (chess-web)

- `clients/chess-web/src/components/eval_chart.rs` — `<EvalChart>`
- `clients/chess-web/src/components/sidebar.rs:51, 130, 190` —
  `eval_badge` prop + `<EvalBadge>` component
- `clients/chess-web/src/components/move_history.rs:35, 91` —
  `HistoryEntry.eval_delta_pct` + delta_view
- `clients/chess-web/style.css` — `.eval-badge*` / `.eval-chart*` /
  `.move-history__delta*` rule families

### Sample plumbing (chess-tui)

- `clients/chess-tui/src/eval.rs:19` — mirror `EvalSample` struct
- `clients/chess-tui/src/eval.rs:28` — `EvalSample::new` (inline negation)
- `clients/chess-tui/src/eval.rs:58` — mirror `EvalSample::final_outcome`
- `clients/chess-tui/src/app.rs:335` — `GameView.eval_samples` field
- `clients/chess-tui/src/app.rs:497, 502` — `evalbar_enabled` /
  `evalbar_open` AppState fields
- `clients/chess-tui/src/app.rs:1066` — `E` key handler
- `clients/chess-tui/src/app.rs:1862` — undo truncation
- `clients/chess-tui/src/app.rs:1980-2021` — `apply_move` with PvP
  pump + final-outcome wiring
- `clients/chess-tui/src/app.rs:2026` — `run_pvp_eval`
- `clients/chess-tui/src/app.rs:2043` — `record_eval_sample`
- `clients/chess-tui/src/app.rs:2061` — `record_eval_final`
- `clients/chess-tui/src/app.rs:2125` — AI reply sample write (negation)

### UI rendering (chess-tui)

- `clients/chess-tui/src/ui.rs:1492` — `push_eval_headline`
- `clients/chess-tui/src/ui.rs:1549` — `push_eval_chart_lines`
- `clients/chess-tui/src/input.rs` — `Action::EvalbarToggle`

### Related docs

- `backlog/ai-winrate-display.md` — original design record (POV
  decision, persistence decision, K=400 starting point)
- `backlog/chess-net-evalbar.md` — net-mode follow-up design
- `backlog/promote-client-shared.md` — would dedupe the two mirror
  `EvalSample` structs
- `pitfalls/eval-sample-stale-analysis-race.md` — the §7.3 race
  written up as a grep-able symptom doc
- `TODO.md` "chess-ai: cp→win% calibration for xiangqi" — §8
  follow-up task entry
- `TODO.md` "chess-net protocol v6 — broadcast win-rate samples" —
  §10 follow-up task entry
- `TODO.md` "chess-web PWA: persist replays in IndexedDB / OPFS" —
  §10 persistence follow-up
- `docs/ai/README.md` — chess-ai version index (links into this doc
  from the Pitfalls section)
- `docs/ai/v3-king-safety-pst.md` — defines `KING_VALUE = 50_000`
  whose magnitude motivated the `WIN_PCT_CLAMP_CP` early-out
