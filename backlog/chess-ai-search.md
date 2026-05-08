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
- ✅ **v3 — `Strategy::MaterialKingSafetyPstV3`** (2026-05-09, default).
  v2 + General has 50_000 cp value (instead of 0). Fixes the casual-mode
  king-blindness bug where the AI would walk into 1-ply general-capture
  mates because the eval didn't penalise losing the General. See
  [`docs/ai/v3-king-safety-pst.md`](../docs/ai/v3-king-safety-pst.md).

## Roadmap

### v4 — `Strategy::NegamaxIdTtV4` (next)

Iterative deepening + Zobrist transposition table.

- **Why next**: biggest single-version strength jump. Also reuses the
  Zobrist hashing infrastructure needed for the P1 TODO
  "threefold-repetition draw detection" — doing them together avoids
  writing Zobrist twice.
- **Tasks**:
  - Implement Zobrist hash on `Board` + `side_to_move` (lives in
    `chess-core`, not `chess-ai`, so the repetition detector can share).
  - Add a small TT (~64 MB) keyed by Zobrist, storing
    `{score, depth, bound, best_move}`.
  - Iterative deepening loop in `engines/negamax_id_tt_v4.rs`:
    deepen until `Difficulty::default_depth` or time budget hits.
  - Use TT best-move as primary move ordering hint (PV-first).
- **Risk**: TT memory in WASM. 64 MB is fine for desktop browsers but
  noticeable on mobile. Cap at 16 MB for WASM target via a `cfg`.

### v5 — `Strategy::NegamaxQuiescenceV5`

Quiescence search + MVV-LVA capture ordering.

- **Why**: stops horizon-effect blunders v3/v4 still make on capture
  exchanges. The single biggest source of "AI gave up its chariot for a
  pawn" complaints.
- **Tasks**:
  - Capture-only search at horizon nodes until quiet.
  - Replace `is_capture` flat ordering with MVV-LVA
    (most-valuable-victim, least-valuable-attacker) → better α-β cutoffs.
- **Depends on**: v4 (TT).

### v6 — `Strategy::NegamaxWebWorkerV6`

Same engine as v4/v5, but hosted in a Web Worker.

- **Why**: by v5 the node budget will routinely hit 250k+ on Hard, and
  the main-thread search will start dropping animation frames. Move it
  off the UI thread.
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
- **Standalone**: doesn't depend on v4-v6; can ship in parallel.

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
