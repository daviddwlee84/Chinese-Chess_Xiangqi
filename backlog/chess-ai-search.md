# chess-engine + chess-ai: search and evaluation

**Status**: P3 / shipped v1+v2 / v3+ pending
**Effort**: L (cumulative across versions)
**Related**: [`docs/ai/README.md`](../docs/ai/README.md) — version index;
`crates/chess-ai/`; [`pitfalls/leptos-effect-tracking-stale-epoch.md`](../pitfalls/leptos-effect-tracking-stale-epoch.md)

## Context

Long-form roadmap for the in-process xiangqi search engine. v1 (material-only)
and v2 (material + PSTs) are shipped; v3..v7 are planned. Each version
is *additive* — `Strategy::*` enum variants stay available for regression
and A/B comparison ("switchable, non-overwriting").

Plug-and-play contract: pure `fn choose_move(&GameState, &AiOptions) -> Option<AiMoveResult>`,
no UI/transport coupling. `chess-tui` (sync, native) and `chess-web`
(WASM, `spawn_local`) consume the same crate.

The strategy-vs-RL/AlphaZero/Pikafish decision is in
[`docs/ai-deep-research-report.md`](../docs/ai-deep-research-report.md)
(2026-05-08 ChatGPT deep-research scan).

## Shipped

- ✅ **v1 — `Strategy::MaterialV1`** (2026-05-08).
  Negamax + α-β + capture-first + material-only eval. See
  [`docs/ai/v1-material.md`](../docs/ai/v1-material.md).
- ✅ **v2 — `Strategy::MaterialPstV2`** (2026-05-08).
  Same search, eval extends v1 with 7 hand-rolled piece-square tables.
  Strictly stronger than v1 at zero extra node cost. See
  [`docs/ai/v2-material-pst.md`](../docs/ai/v2-material-pst.md).
  Was the default 2026-05-08 → 2026-05-09; superseded by v3 because it
  shared v1's casual-mode king-blindness — see
  [`pitfalls/casual-xiangqi-king-blindness.md`](../pitfalls/casual-xiangqi-king-blindness.md).
- ✅ **v3 — `Strategy::MaterialKingSafetyPstV3`** (2026-05-09).
  v2 + General has 50_000 cp value (instead of 0). Fixes the casual-mode
  king-blindness bug. See
  [`docs/ai/v3-king-safety-pst.md`](../docs/ai/v3-king-safety-pst.md).
  Was the default for a few hours on 2026-05-09; superseded by v4 because
  horizon-effect blunders on captures still slipped through.
- ✅ **v4 — `Strategy::QuiescenceMvvLvaV4`** (2026-05-09, default).
  Same v3 evaluator, but the search now uses MVV-LVA capture ordering
  and a quiescence search at the horizon. Stops the "AI wins a chariot
  then loses it back next move" class of horizon-effect blunder.
  Cost: ~9× v3 in busy openings (uses ~71% of the 250k node budget at
  Hard depth 4). See [`docs/ai/v4-quiescence-mvv-lva.md`](../docs/ai/v4-quiescence-mvv-lva.md).

## User-side configuration shipped 2026-05-09

In addition to the engine versions above, the AI exposes:

- `?engine=v1|v2|v3|v4` (web) / `--ai-engine` (TUI) — version selector.
- `?variation=strict|subtle|varied|chaotic` / `--ai-variation` —
  randomness preset (`Randomness { top_k, cp_window }`). Decouples
  variation from difficulty so e.g. `Hard + STRICT` is deterministic
  and `Easy + STRICT` is "depth 1 always best".
- `?depth=N` (1..=10) / `--ai-depth N` (1..=12) — explicit depth
  override. Lets users push past the difficulty default for stress
  testing.

## Roadmap

### v5 — `Strategy::NegamaxIdTtV5` (next)

Iterative deepening + Zobrist transposition table.

- **Why next**: v4's busy-opening cost (280 ms native, 1-3 s WASM) is
  the new ceiling. The single biggest mitigation is a TT — many
  positions are reached via multiple move orders, and v4 re-searches
  each one. With a TT, depth-4 search shrinks back toward v3 cost
  range. Iterative deepening reuses shallow PVs as ordering hints for
  deeper search → fewer nodes for the same depth.
- **Bonus**: shares Zobrist hashing with the P1 TODO
  "threefold-repetition draw detection" — doing them together avoids
  writing Zobrist twice.
- **Tasks**:
  - Implement Zobrist hash on `Board` + `side_to_move` (lives in
    `chess-core`, not `chess-ai`, so the repetition detector can share).
  - Add a small TT (~64 MB) keyed by Zobrist, storing
    `{score, depth, bound, best_move}`.
  - Iterative deepening loop in `engines/negamax_id_tt_v5.rs`:
    deepen until `Difficulty::default_depth` or time budget hits.
  - Use TT best-move as primary move ordering hint (PV-first), fall
    back to MVV-LVA.
- **Risk**: TT memory in WASM. 64 MB is fine for desktop browsers but
  noticeable on mobile. Cap at 16 MB for WASM target via a `cfg`.

### v6 — `Strategy::NegamaxWebWorkerV6`

Same engine as v4/v5, but hosted in a Web Worker.

- **Why**: by v5 the time budget approach makes "deeper at busy
  positions" cheap, but the main-thread search will still drop animation
  frames during a 500 ms+ search. Move it off the UI thread.
- **Notes**:
  - chess-tui doesn't need this — native is fast enough.
  - chess-web change: a worker glue file plus a `MessageChannel`-based
    transport. The engine itself is reused as-is.
  - Cancellation: the existing `move_epoch` epoch token in
    `clients/chess-web/src/pages/local.rs` is enough — ignore stale
    worker results.

### v7 — `Strategy::ISMCTSv7` (banqi only)

Information-set Monte Carlo Tree Search for banqi.

- **Why**: banqi has hidden tiles. Alpha-beta over the deterministic
  resolution would let the engine peek at face-down piece identities
  (a search bug, not a feature). ISMCTS samples plausible
  determinisations and runs MCTS on each.
- **Effort**: research-heavy. Distinct algorithm class from v1-v6.
- **Tasks**:
  - Rework `Move::Reveal { revealed: None }` handling so the AI never
    sees the flipped piece during simulation.
  - PUCT / UCB1 selection.
  - Determinisation sampler with the right prior (count-based on still-
    unflipped tiles).
- **Standalone**: doesn't depend on v5-v6; can ship in parallel.

### v8 — `Strategy::PikafishBackendV8` (optional)

Pikafish UCI backend — gated behind a Cargo feature.

- **Why**: strongest available. For users who want world-class play and
  accept the dependency footprint.
- **Constraints**:
  - Native only (UCI subprocess). No WASM.
  - Gated behind a Cargo feature (`pikafish-backend`) so the default
    workspace build / WASM bundle stays clean-room.
  - Pikafish is GPLv3 — runtime dependency only, no source linking.
- **Open questions**: licensing/distribution of Pikafish binaries;
  setup UX for end users; whether to ship a `chess-ai-server` separate
  process so the WASM client can talk to it via WebSocket.

## Cross-cutting future work

- **Tuning rounds for PSTs (v2.1?)**. Self-play tournaments to refine
  the hand-derived numbers. Could land before v3 if v3 slips.
- **Opening book**. Pre-computed table for the first ~10 plies. Cheap
  way to make all difficulties feel "professional" in the opening.
- **Endgame tablebase**. K+R vs K and similar — out of scope until we
  have a real player base who notices missed wins.
- **Game-phase split** (opening / midgame / endgame PSTs). Tag for v3
  or v4 as appropriate.

## Testing strategy

- Unit tests per evaluator (PST shape sanity, deterministic outputs).
- `strategy_dispatch_is_distinguishable` test in `lib.rs::tests` — guards
  against wiring two `Strategy` variants to the same evaluator (silent
  bug).
- `wasm-bindgen-test` and Playwright E2E live in
  [`backlog/web-playwright.md`](web-playwright.md). E2E is the right
  place to test the AI move pump (the Leptos epoch dance) — pure-Rust
  tests can't simulate the reactive runtime.

## References

- [`docs/ai/README.md`](../docs/ai/README.md) — version index, selection
  syntax, crate layout.
- [`docs/ai/v1-material.md`](../docs/ai/v1-material.md) and
  [`docs/ai/v2-material-pst.md`](../docs/ai/v2-material-pst.md) — per-version
  specs.
- [`docs/ai-deep-research-report.md`](../docs/ai-deep-research-report.md) —
  the original "why alpha-beta, why not RL/Pikafish/LLM" decision.
- [`pitfalls/leptos-effect-tracking-stale-epoch.md`](../pitfalls/leptos-effect-tracking-stale-epoch.md) —
  the AI move pump's reactive-runtime trap (frontend, not engine).
