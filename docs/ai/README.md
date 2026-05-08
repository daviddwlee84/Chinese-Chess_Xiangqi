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
/local/xiangqi?mode=ai&ai=black&diff=hard               # uses default (v2)
/local/xiangqi?mode=ai&ai=black&diff=hard&engine=v2     # explicit v2
/local/xiangqi?mode=ai&ai=black&diff=hard&engine=v1     # legacy material-only
```

The picker also exposes a radio under "Engine" inside the vs-AI fieldset.
Aliases accepted by the parser: `material`, `material-v1` → v1;
`material-pst`, `material-pst-v2` → v2.

### chess-tui

CLI flags on the `xiangqi` subcommand:

```
chess-tui xiangqi --ai                              # vs computer (defaults: Black/Normal/v2)
chess-tui xiangqi --ai --ai-engine v1               # legacy material-only
chess-tui xiangqi --ai --ai-side red --ai-difficulty hard --ai-engine v2
```

The picker entry "Xiangqi (象棋) vs Computer" uses the v2 default; the
flags above are the power-user surface.

### Library API

```rust
use chess_ai::{AiOptions, Difficulty, Strategy};

let opts = AiOptions {
    difficulty: Difficulty::Hard,
    max_depth: None,        // use Difficulty::default_depth
    seed: Some(42),         // reproducible Easy/Normal randomness
    strategy: Strategy::MaterialPstV2,
};
let result = chess_ai::choose_move(&state, &opts);
```

`AiOptions::default()` and `AiOptions::new(Difficulty::Normal)` both
default `strategy` to `Strategy::default()` (currently `MaterialPstV2`).

## Versions

| Strategy | Algorithm | Eval | Default | Doc |
|---|---|---|---|---|
| `Strategy::MaterialV1` | negamax + α-β + capture-first | material only | no (legacy) | [`v1-material.md`](v1-material.md) |
| `Strategy::MaterialPstV2` | negamax + α-β + capture-first | material + 7 hand-rolled PSTs | **yes** | [`v2-material-pst.md`](v2-material-pst.md) |

## Roadmap

These are *not* shipped — they are the planned future variants. Each
will land as a new `Strategy::*` variant + a new module under
`crates/chess-ai/src/engines/`. Order is approximate; see
`backlog/chess-ai-search.md` for the live priority and dependencies.

| Future | Sketch | Why next |
|---|---|---|
| **v3** = NegamaxIdTtV3 | iterative deepening + Zobrist transposition table | reuses Zobrist work needed for threefold-repetition draw (P1 in `TODO.md`). Bigger strength jump than PSTs. |
| **v4** = NegamaxQuiescenceV4 | quiescence + MVV-LVA capture ordering | stops the horizon-effect blunders v2/v3 still make on captures. |
| **v5** = NegamaxWebWorkerV5 | same engine, hosted in a Web Worker | unblocks UI on `Hard` once ID/quiescence push node counts past the current 250k budget. |
| **v6** = ISMCTSv6 (banqi only) | information-set MCTS | banqi has hidden tiles; alpha-beta would peek. Separate algorithm class. |
| **v7** = PikafishBackendV7 | optional UCI backend (native only) | strongest bar; gated behind a Cargo feature so the WASM bundle stays clean-room. |

Each future version inherits the same plug-and-play contract: pure
`fn choose_move(&GameState, &AiOptions) -> Option<AiMoveResult>`, no
frontend coupling, no globals.

## Crate layout

```
crates/chess-ai/src/
  lib.rs              — public API: choose_move, AiOptions, Difficulty, Strategy
  search/mod.rs       — shared negamax + α-β framework, generic over Evaluator
  eval/mod.rs         — Evaluator trait
  eval/material_v1.rs        — v1 evaluator (preserved verbatim)
  eval/material_pst_v2.rs    — v2 evaluator (material + PSTs)
  engines/mod.rs      — Engine trait + NegamaxV1 + NegamaxV2
```

Adding v3 means: a new `eval/` impl (or reuse v2), a new module
`engines/negamax_id_tt_v3.rs`, and a new `Strategy::NegamaxIdTtV3`
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
