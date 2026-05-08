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
cost (today: `v2`).

## Selecting a version

### chess-web

URL query parameter on `/local/xiangqi`:

```
/local/xiangqi?mode=ai&ai=black&diff=hard               # uses default (v3)
/local/xiangqi?mode=ai&ai=black&diff=hard&engine=v3     # explicit v3
/local/xiangqi?mode=ai&ai=black&diff=hard&engine=v2     # legacy material+PSTs (king-blind in casual)
/local/xiangqi?mode=ai&ai=black&diff=hard&engine=v1     # legacy material-only

/local/xiangqi?mode=ai&diff=hard&variation=strict       # deterministic, never deviates
/local/xiangqi?mode=ai&diff=hard&variation=subtle       # top-3 within ±20 cp (Hard default)
/local/xiangqi?mode=ai&diff=hard&variation=varied       # top-5 within ±60 cp
/local/xiangqi?mode=ai&diff=hard&variation=chaotic      # top-10 within ±150 cp ("weak Hard")
```

The picker exposes radios under "Engine" and "Variation" inside the
vs-AI fieldset. Aliases accepted by the parser:
- engine: `material`, `material-v1` → v1; `material-pst`, `material-pst-v2` → v2; `material-king-safety-pst`, `king-safety` → v3.
- variation: `strict`/`off`/`none`/`deterministic`; `subtle`/`low`; `varied`/`medium`/`med`; `chaotic`/`wild`/`high`.

### chess-tui

CLI flags on the `xiangqi` subcommand:

```
chess-tui xiangqi --ai                              # vs computer (defaults: Black/Normal/v3/default-variation)
chess-tui xiangqi --ai --ai-engine v2               # legacy v2 (no king safety)
chess-tui xiangqi --ai --ai-engine v1               # legacy material-only
chess-tui xiangqi --ai --ai-side red --ai-difficulty hard --ai-engine v3
chess-tui xiangqi --ai --ai-difficulty hard --ai-variation strict   # deterministic Hard
chess-tui xiangqi --ai --ai-difficulty hard --ai-variation chaotic  # high-variety Hard
```

The picker entry "Xiangqi (象棋) vs Computer" uses the v3 default; the
flags above are the power-user surface.

### Library API

```rust
use chess_ai::{AiOptions, Difficulty, Randomness, Strategy};

let opts = AiOptions {
    difficulty: Difficulty::Hard,
    max_depth: None,        // use Difficulty::default_depth
    seed: Some(42),         // reproducible Easy/Normal randomness
    strategy: Strategy::MaterialKingSafetyPstV3,
    randomness: None,       // None = use Difficulty::default_randomness
    // randomness: Some(Randomness::STRICT),  // override: deterministic
};
let result = chess_ai::choose_move(&state, &opts);
```

`AiOptions::default()` and `AiOptions::new(Difficulty::Normal)` both
default `strategy` to `Strategy::default()` (currently `MaterialKingSafetyPstV3`)
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

TL;DR for current default (v3 + Hard + depth 4):
- Native release: 15-30 ms per move
- Browser WASM (estimated): 150-300 ms per move
- Well within the 250k node budget; ample headroom for v4 (iterative
  deepening + TT) to push past depth 4 without UI blocking.

## Versions

| Strategy | Algorithm | Eval | Default | Doc |
|---|---|---|---|---|
| `Strategy::MaterialV1` | negamax + α-β + capture-first | material only | no (legacy) | [`v1-material.md`](v1-material.md) |
| `Strategy::MaterialPstV2` | negamax + α-β + capture-first | material + 7 hand-rolled PSTs | no (king-blind in casual mode) | [`v2-material-pst.md`](v2-material-pst.md) |
| `Strategy::MaterialKingSafetyPstV3` | negamax + α-β + capture-first | v2 + General = 50_000 cp | **yes** (since 2026-05-09) | [`v3-king-safety-pst.md`](v3-king-safety-pst.md) |

## Roadmap

These are *not* shipped — they are the planned future variants. Each
will land as a new `Strategy::*` variant + a new module under
`crates/chess-ai/src/engines/`. Order is approximate; see
`backlog/chess-ai-search.md` for the live priority and dependencies.

| Future | Sketch | Why next |
|---|---|---|
| **v4** = NegamaxIdTtV4 | iterative deepening + Zobrist transposition table | reuses Zobrist work needed for threefold-repetition draw (P1 in `TODO.md`). Bigger strength jump than PSTs. |
| **v5** = NegamaxQuiescenceV5 | quiescence + MVV-LVA capture ordering | stops the horizon-effect blunders v3/v4 still make on captures. |
| **v6** = NegamaxWebWorkerV6 | same engine, hosted in a Web Worker | unblocks UI on `Hard` once ID/quiescence push node counts past the current 250k budget. |
| **v7** = ISMCTSv7 (banqi only) | information-set MCTS | banqi has hidden tiles; alpha-beta would peek. Separate algorithm class. |
| **v8** = PikafishBackendV8 | optional UCI backend (native only) | strongest bar; gated behind a Cargo feature so the WASM bundle stays clean-room. |

Each future version inherits the same plug-and-play contract: pure
`fn choose_move(&GameState, &AiOptions) -> Option<AiMoveResult>`, no
frontend coupling, no globals.

## Crate layout

```
crates/chess-ai/src/
  lib.rs              — public API: choose_move, AiOptions, Difficulty, Strategy
  search/mod.rs       — shared negamax + α-β framework, generic over Evaluator
  eval/mod.rs         — Evaluator trait
  eval/material_v1.rs                — v1 evaluator (preserved verbatim)
  eval/material_pst_v2.rs            — v2 evaluator (material + PSTs)
  eval/material_king_safety_pst_v3.rs — v3 evaluator (v2 + General = 50_000 cp)
  engines/mod.rs      — Engine trait + NegamaxV1 + NegamaxV2 + NegamaxV3
```

Adding v4 means: a new `eval/` impl (or reuse v3), a new module
`engines/negamax_id_tt_v4.rs`, and a new `Strategy::NegamaxIdTtV4`
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
