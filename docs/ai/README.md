# chess-ai version index

`chess-ai` is the workspace's clean-room search engine for xiangqi. It is
*plug-and-play*: a single function `chess_ai::choose_move(state, opts)`
returns one move, with no knowledge of UI, async runtime, or transport.
Both `chess-tui` (native, sync) and `chess-web` (WASM, `spawn_local`)
consume the same `chess-ai` crate.

## Switchable, non-overwriting

When a new evaluation strategy or search algorithm lands, it is **added**
as a new `Strategy::*` variant — the older versions stay reachable
forever. This makes the engine easy to:

- regress against a known baseline (set `?engine=v1` / `--ai-engine v1`)
- A/B compare strengths
- pin a particular release's behaviour for a tournament run

The default is whichever version is currently strongest at zero extra
cost (today: `v5`).

## Selecting a version

### chess-web

URL query parameter on `/local/xiangqi`:

```
/local/xiangqi?mode=ai&ai=black&diff=hard               # uses default (v5)
/local/xiangqi?mode=ai&ai=black&diff=hard&engine=v5     # explicit v5 (ID + TT)
/local/xiangqi?mode=ai&ai=black&diff=hard&engine=v4     # legacy (no ID/TT)
/local/xiangqi?mode=ai&ai=black&diff=hard&engine=v3     # legacy (no quiescence)
/local/xiangqi?mode=ai&ai=black&diff=hard&engine=v2     # legacy (king-blind in casual)
/local/xiangqi?mode=ai&ai=black&diff=hard&engine=v1     # legacy material-only

/local/xiangqi?mode=ai&diff=hard&variation=strict       # deterministic, never deviates
/local/xiangqi?mode=ai&diff=hard&variation=subtle       # top-3 within ±20 cp (Hard default)
/local/xiangqi?mode=ai&diff=hard&variation=varied       # top-5 within ±60 cp
/local/xiangqi?mode=ai&diff=hard&variation=chaotic      # top-10 within ±150 cp ("weak Hard")

/local/xiangqi?mode=ai&depth=6                          # override search depth (1..=10)
```

The picker exposes radios under "Engine" and "Variation", plus a
"Search depth (advanced)" number input, inside the vs-AI fieldset.
Aliases accepted by the parser:
- engine: `material`, `material-v1` → v1; `material-pst`, `material-pst-v2` → v2; `material-king-safety-pst`, `king-safety` → v3; `quiescence`, `quiescence-mvv-lva`, `qmvv` → v4; `id-tt`, `iterative-deepening`, `iterative-deepening-tt` → v5.
- variation: `strict`/`off`/`none`/`deterministic`; `subtle`/`low`; `varied`/`medium`/`med`; `chaotic`/`wild`/`high`.

### chess-tui

CLI flags on the `xiangqi` subcommand:

```
chess-tui xiangqi --ai                              # vs computer (defaults: Black/Normal/v5)
chess-tui xiangqi --ai --ai-engine v4               # legacy v4 (no ID/TT)
chess-tui xiangqi --ai --ai-engine v3               # legacy v3 (no quiescence)
chess-tui xiangqi --ai --ai-engine v2               # legacy v2 (no king safety)
chess-tui xiangqi --ai --ai-engine v1               # legacy material-only
chess-tui xiangqi --ai --ai-side red --ai-difficulty hard --ai-engine v5
chess-tui xiangqi --ai --ai-difficulty hard --ai-variation strict   # deterministic Hard
chess-tui xiangqi --ai --ai-difficulty hard --ai-variation chaotic  # high-variety Hard
chess-tui xiangqi --ai --ai-depth 6                                 # override depth (1..=12)
```

The picker entry "Xiangqi (象棋) vs Computer" uses the v3 default; the
flags above are the power-user surface.

### Library API

```rust
use chess_ai::{AiOptions, Difficulty, Randomness, Strategy};

let opts = AiOptions {
    difficulty: Difficulty::Hard,
    max_depth: None,        // use Difficulty::default_depth (or override)
    seed: Some(42),         // reproducible Easy/Normal randomness
    strategy: Strategy::QuiescenceMvvLvaV4,
    randomness: None,       // None = use Difficulty::default_randomness
    // randomness: Some(Randomness::STRICT),  // override: deterministic
};
let result = chess_ai::choose_move(&state, &opts);
```

`AiOptions::default()` and `AiOptions::new(Difficulty::Normal)` both
default `strategy` to `Strategy::default()` (currently `QuiescenceMvvLvaV4`)
and `randomness` to `None` (use the difficulty default).

## Difficulty + Randomness defaults

[`Difficulty`](../../crates/chess-ai/src/lib.rs) controls two things:
search depth (`default_depth`) and the move-pick policy
(`default_randomness`). The randomness policy is encoded as
[`Randomness { top_k, cp_window }`](../../crates/chess-ai/src/lib.rs):
filter to moves within `cp_window` cp of the best, then take the top
`top_k`, then RNG picks one uniformly.

| Difficulty | Default depth | Default randomness | Notes |
|---|---|---|---|
| `Easy` | 1 | `Randomness::CHAOTIC` (top-10 within ±150 cp) | Wild — encourages varied games for human learners |
| `Normal` | 3 | `Randomness::VARIED` (top-5 within ±60 cp) | Mostly best, occasional sidesteps |
| `Hard` | 4 | `Randomness::SUBTLE` (top-3 within ±20 cp) | Imperceptible strength loss; avoids repetitive games. **Pass `Some(Randomness::STRICT)` for deterministic play.** |

Override `AiOptions::randomness` to decouple variation from difficulty
— e.g. `Difficulty::Hard` + `Randomness::STRICT` for tournament-style
deterministic Hard, or `Difficulty::Easy` + `Randomness::STRICT` for
"depth 1 always best move".

Built-in presets (in canonical "name token" form for URL/CLI):

- `strict` / `off` / `none` / `deterministic`
- `subtle` / `low`
- `varied` / `medium` / `med`
- `chaotic` / `wild` / `high`

## Performance

See [`perf.md`](perf.md) for measured nodes-per-search, wall-clock per
move, and headroom analysis.

TL;DR for current default (v4 + Hard + depth 4):
- Native release: 12-280 ms per move (varies wildly by position — opening is the worst case because quiescence has nothing to do at the leaves so the cost lives at the root)
- Browser WASM (estimated): 60 ms - 3 s per move
- v4 is ~9× more expensive than v3 in busy openings; midgame/endgame parity. Use `?engine=v3` if you want v3's speed without the horizon-effect fix.

## Crate layout

```
crates/chess-ai/src/
  lib.rs              — public API: choose_move, AiOptions, Difficulty, Randomness, Strategy
  search/mod.rs       — shared negamax + α-β framework, generic over Evaluator (v1-v3 search + v4 qmvv search)
  search/ordering.rs  — MVV-LVA helper (v4)
  search/quiescence.rs — quiescence search (v4)
  eval/mod.rs         — Evaluator trait
  eval/material_v1.rs                — v1 evaluator (preserved verbatim)
  eval/material_pst_v2.rs            — v2 evaluator (material + PSTs)
  eval/material_king_safety_pst_v3.rs — v3 evaluator (v2 + General = 50_000 cp); v4 reuses
  engines/mod.rs      — Engine trait + NegamaxV1 + NegamaxV2 + NegamaxV3 + NegamaxQuiescenceMvvLvaV4
```

Adding v5 means: keep v3 evaluator (or write a new one), new module
`engines/negamax_id_tt_v5.rs` (which wires Zobrist + iterative deepening
+ TT into the existing search), and a new `Strategy::NegamaxIdTtV5`
variant + dispatch arm in `lib.rs::choose_move`. *No code is deleted.*

## Background research

The original strategy decision (alpha-beta over RL / AlphaZero / LLM /
Pikafish) is documented in [`../ai-deep-research-report.md`](../ai-deep-research-report.md)
(ChatGPT deep-research scan, 2026-05-08). The version index here covers
the *implementation* pipeline; the research report covers the *choice
of approach*.

## Pitfalls

- [`pitfalls/leptos-effect-tracking-stale-epoch.md`](../../pitfalls/leptos-effect-tracking-stale-epoch.md)
  — class-of-bug doc for the AI move pump in the web client. Not specific
  to any engine version; lives in pitfalls/ because it's a Leptos
  reactive-runtime issue, not a chess-ai issue.
