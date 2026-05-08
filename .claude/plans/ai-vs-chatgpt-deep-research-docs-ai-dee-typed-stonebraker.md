# Plan: Local "vs AI" mode (alpha-beta MVP)

## Context

Today the chess-web app supports only pass-and-play (`/local/:variant`) and online play via chess-net. The `chess-ai` crate is a stub. Per `docs/ai-deep-research-report.md` and the user's research, the MVP should be a **clean-room, lightweight alpha-beta engine** running in-process (WASM) — not RL, not Pikafish, not LLM, not even a Web Worker yet. Goal: a player can pick "vs AI" + side + difficulty in the picker, and play locally against a deterministic search engine that plugs into the existing legal-move pipeline.

Scope: **xiangqi only in Phase 1.** Banqi has hidden-tile information asymmetry that this minimax design would silently break (see Risks). Three-kingdom banqi has no engine at all.

## Approach

### Phase 1A — `crates/chess-ai` engine (MVP)

Single new module producing one public function:

```rust
pub fn choose_move(state: &GameState, opts: &AiOptions) -> Option<AiMoveResult>;

pub struct AiOptions { pub difficulty: Difficulty, pub max_depth: Option<u8>, pub randomness: u8 /* 0..=255 */, pub seed: Option<u64> }
pub enum Difficulty { Easy, Normal, Hard }
pub struct AiMoveResult { pub mv: Move, pub score: i32, pub depth: u8, pub nodes: u32, pub elapsed_ms: u32 }
```

Implementation:

- **Search**: negamax + alpha-beta, no transposition table, no quiescence, no iterative deepening (Phase 2 candidates).
- **Move generation**: call `state.legal_moves()` directly (`crates/chess-core/src/state/mod.rs:86`). The xiangqi clone-on-probe legality cost is acceptable for depth ≤ 4.
- **Make/unmake**: `state.make_move(&mv)` + `state.unmake_move()` (`state/mod.rs:104,146`). Mutates in place — no per-node cloning needed for the search itself (cloning happens inside `legal_moves` only, and only in xiangqi strict).
- **Terminal detection**: at leaf, run `state.refresh_status()` once; if `Won { winner }` return `±MATE - depth`; `Drawn` return `0`. CLAUDE.md flags that `make_move` does NOT auto-refresh — we must do it.
- **Evaluation** (xiangqi handcrafted): material table (Chariot 900, Cannon 450, Horse 400, Advisor/Elephant 200, Soldier 100 / 200 past river, General excluded from material — checkmate already handled as ±MATE). Side-relative score: `eval(state) = sum(red_material) - sum(black_material)` then signed for `state.side_to_move`. Iterate `board.squares()` (`board/mod.rs`) to read pieces. No PSTs in MVP — single-line "soldier crossed river" bonus only.
- **Move ordering**: captures first (cheap — match on `Move::Capture` / `Move::CannonJump` variants), then quiet. Skip MVV-LVA for now.
- **Difficulty mapping**:
  - Easy: depth 1, pick uniformly from top-3 by score (deterministic with `seed`).
  - Normal: depth 3, pick best with small ±10cp tiebreak randomness.
  - Hard: depth 4, strict best.
- **Fallback**: if `legal_moves()` is empty → `None` (caller treats as game-over). If something panics-worthy comes up, return a random legal move rather than the engine's pick.
- **Determinism**: use `rand_chacha::ChaCha8Rng::seed_from_u64` (already used by `chess-core` for banqi shuffles per CLAUDE.md). No `thread_rng`. Keeps the engine WASM-clean and reproducible.

Tests (`crates/chess-ai/tests/`):
1. Initial xiangqi position returns a legal move at each difficulty.
2. Mate-in-1 fixture (use `.pos` DSL via `GameState::from_pos_text` per CLAUDE.md): hard mode picks the mate.
3. Forced-recapture fixture: hard mode plays the recapture.
4. No-legal-moves: returns `None` without panicking.
5. Determinism: same `(state, seed, difficulty)` → same move.

### Phase 1B — chess-web wiring

**`Cargo.toml`** (`clients/chess-web/Cargo.toml`): add `chess-ai = { path = "../../crates/chess-ai" }` to base deps (not target-gated — the engine must compile native too for any future shared tests).

**Routing** (`clients/chess-web/src/routes.rs`):
- Extend `LocalRulesParams` (line 95) with `mode: PlayMode`, `ai_side: Side`, `ai_difficulty: Difficulty`. Tokens: `?mode=ai&ai=red|black&diff=easy|normal|hard`. Default `mode=pvp` keeps existing URLs byte-identical (back-compat).
- Update `parse_local_rules` (line 113) and `build_local_query` (line 125) — pure-logic, native-tested.

**Picker** (`clients/chess-web/src/pages/picker.rs`):
- Add a `mode: RwSignal<PlayMode>` radio at the top of `XiangqiCard` (line 43). When `VsAi`, reveal a small fieldset: side radio (red/black), difficulty `<select>`. Banqi/three-kingdom cards: hide the toggle entirely (xiangqi-only in MVP).

**Local page** (`clients/chess-web/src/pages/local.rs`):
- Read `mode`/`ai_side`/`ai_difficulty` from the parsed params.
- Add `ai_thinking: RwSignal<bool>`.
- After every `state.update(...)` that lands a player move (line 147 site), check `mode == VsAi && side_to_move == ai_side && status == Ongoing`. If so, set `ai_thinking=true`, `spawn_local` an async task that:
  1. `set_timeout` ~250ms (cosmetic delay, keeps "AI thinking" visible),
  2. snapshots the current `state.get_untracked()`,
  3. calls `chess_ai::choose_move(&snap, &opts)`,
  4. on the main task, re-checks the state hasn't changed (token/epoch counter — increment on every undo/reset/move), then `state.update(|s| { s.make_move(&ai_mv).unwrap(); s.refresh_status(); })`,
  5. clears `ai_thinking`.
- Cancellation: bump a `move_epoch: RwSignal<u32>` on undo/reset/restart; the AI task captures the epoch at start and discards its result if the epoch changed. Avoids stale moves landing after the player undid.

**Board** (`clients/chess-web/src/components/board.rs`):
- Add `#[prop(default = false)] disabled: bool`. Wrap the hit-cell layer (line 51) in `<Show when=move || !disabled>`. Local page passes `disabled = ai_thinking.get() || (mode == VsAi && side_to_move != player_side)` to prevent the user from clicking during AI's turn.

**Sidebar** (`clients/chess-web/src/components/sidebar.rs`):
- New `<Show when=ai_thinking>` row above the turn label (line 98) reusing `.check-badge` styling: text "AI thinking…". When idle in VsAi mode, append "(vs AI — Hard)" to the turn label.

### Phase 1C — chess-tui (deferred — not in this PR)

Single-player AI in chess-tui is a natural follow-up but not in scope; record as a TODO entry. Engine API is reusable from native targets.

## Critical files

- `crates/chess-ai/Cargo.toml` — add `rand`, `rand_chacha`; depend on `chess-core` only.
- `crates/chess-ai/src/lib.rs` — `choose_move`, `negamax`, `evaluate`, `Difficulty`, `AiOptions`.
- `crates/chess-ai/tests/{smoke,mate_in_one,determinism}.rs` — fixtures under `tests/fixtures/xiangqi/`.
- `clients/chess-web/Cargo.toml` — add `chess-ai` dep.
- `clients/chess-web/src/routes.rs` — `LocalRulesParams` + parser/builder (pure-logic, native tests).
- `clients/chess-web/src/pages/picker.rs` — mode/side/difficulty controls (xiangqi card only).
- `clients/chess-web/src/pages/local.rs` — AI-driven move pump, epoch-cancellation, disabled gating.
- `clients/chess-web/src/components/board.rs` — `disabled` prop.
- `clients/chess-web/src/components/sidebar.rs` — "AI thinking…" indicator.
- `TODO.md` — record banqi-AI follow-up + chess-tui vs-AI follow-up.

## Risks

- **Banqi must NOT use this engine in MVP.** `Move::Reveal { revealed: None }` becomes `Some(true_piece)` inside `make_move` — a minimax that searches reveal nodes effectively *cheats* by seeing future tiles. Proper banqi AI needs determinization / ISMCTS. Picker hides the toggle on the banqi card; local page ignores `mode=ai` when variant != xiangqi.
- **Hard mode at depth 4 may exceed ~1s on slower phones.** Mitigation: cap `nodes` at e.g. 200k and short-circuit; or drop hard to depth 3 if benchmarking shows trouble. Web Worker is Phase 2 (`backlog/`).
- **Stale AI move after undo.** Mitigated by `move_epoch` token check before applying.
- **No threefold-repetition handling** in the engine — chess-core itself doesn't implement it (CLAUDE.md). Engine may oscillate in drawn endgames; acceptable for MVP, document in `pitfalls/` if it surfaces.
- **Casual xiangqi** (`xiangqi_allow_self_check`) — the AI calls `legal_moves()` which respects the rule set, so it'll happily blunder into self-check in casual mode. That's actually the desired UX (matches what humans can do).

## Verification

```bash
# Engine
cargo test -p chess-ai
cargo build --target wasm32-unknown-unknown -p chess-ai

# Pre-push (per CLAUDE.md)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --target wasm32-unknown-unknown -p chess-core

# Manual
make play-web
# In browser: picker → xiangqi → mode=AI, side=Red, difficulty=Hard → /local/xiangqi?mode=ai&ai=red&diff=hard
# Play a few moves; verify AI moves arrive ~250ms after each player move,
# board is click-locked during "AI thinking…", undo cancels in-flight AI move,
# reset returns to picker cleanly.
```
