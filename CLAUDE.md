# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project at a glance

Rust + WASM Chinese chess engine supporting standard xiangqi (象棋), banqi (暗棋), and three-kingdoms banqi (三國暗棋). PR 1 ships the foundational `chess-core` crate end-to-end; clients (`chess-tui`, `chess-web`), networking (`chess-net`), and AI (`chess-ai`) are scaffolded as stubs and tracked in [`TODO.md`](TODO.md).

For the tech-selection rationale see [`docs/architecture.md`](docs/architecture.md); for locked-in design decisions see [`docs/adr/`](docs/adr/).

## Common commands

```bash
# Workspace sanity (all 8 crates compile)
cargo check --workspace

# Format + lint (CI requires both clean; clippy uses -D warnings)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# Engine tests (~71 fast + 1 slow ignored). Default if you change rules.
cargo test -p chess-core

# Run a single test
cargo test -p chess-core --lib coord::tests::direction_deltas_consistent

# Slow xiangqi perft depth-3 — runs in release for speed
cargo test --release -p chess-core --test xiangqi_perft -- --ignored

# WASM cleanliness — proves chess-core has no platform deps
cargo build --target wasm32-unknown-unknown -p chess-core

# Run the REPL test harness (proves the engine end-to-end)
cargo run -p chess-cli
> xiangqi
> moves
> play h2e2
> banqi --preset taiwan --seed 42
> view 0
> quit
```

`rustup target add wasm32-unknown-unknown` once per machine. If your rustup mirror lacks the target (e.g. tuna), prefix the command with `RUSTUP_DIST_SERVER=https://static.rust-lang.org` — see [`pitfalls/wasm-getrandom-unresolved-imp.md`](pitfalls/wasm-getrandom-unresolved-imp.md) for the related `js`-feature gotcha.

## Architecture quick reference

The engine lives entirely in `crates/chess-core`. Everything else (`chess-engine`, `chess-net`, `chess-ai`, `chess-tui`, `chess-web`, `xtask`) is a stub for follow-up PRs. Five non-obvious decisions are locked in — full rationale in `docs/adr/`:

1. **`Square(u16)` linear index** (ADR-0002), not `(file, rank)` tuples. `Board` knows its `BoardShape` and converts. Scales to 19×19 + irregular topology via per-shape mask.
2. **`Move` is a flat enum** (ADR-0004). `Move::Reveal { at, revealed: Option<Piece> }` is the network ABI boundary: clients send `revealed: None`, the authoritative engine fills in `Some(piece)` post-flip. All variants serde clean.
3. **`RuleSet` is plain data + `bitflags`, not a trait** (ADR-0003). Move-gen is free functions in `rules/{xiangqi,banqi,three_kingdom}.rs` dispatching on `Variant` and consulting `HouseRules` flags. Trait-object rule layering was rejected — kills inlining, fights serde.
4. **`GameState` is one concrete struct.** `TurnOrder` holds `SmallVec<[Side; 3]>` so 3-player isn't a special case. `Side(u8)` (not a fixed enum) carries this.
5. **`PlayerView::project(&GameState, observer)` is the only externally-visible state.** Hidden pieces become `VisibleCell::Hidden` with no identity. `tests/view_projection.rs` proptest enforces no-leak in serialized JSON. The future network layer must ship `PlayerView`, never `GameState`.

Move generation pipeline (xiangqi): `pseudo_legal_moves` (geometry only) → clone-the-state-and-probe legality filter → emit. Cheap enough for 9×10; a future AI hot path should switch to make/unmake without cloning.

## Gotchas worth knowing

- **`make_move` does NOT auto-refresh `status`.** The xiangqi legality filter calls `make_move` on a clone to test self-check; auto-refresh would recurse via `legal_moves`. Callers (CLI / TUI / future server) invoke `state.refresh_status()` after each move when they want to know if the game ended. `refresh_status` covers no-progress draws + no-legal-moves; threefold repetition is a TODO.

- **Three deferred house rules accept the flag but no-op**: `DARK_CHAIN`, `HORSE_DIAGONAL`, `CANNON_FAST_MOVE`. Only `CHAIN_CAPTURE` and `CHARIOT_RUSH` are wired end-to-end. Don't assume code that consumes `HouseRules` handles every flag — grep `rules/banqi.rs::generate` to confirm. The deferred ones are P1 in `TODO.md`.

- **`Variant::ThreeKingdomBanqi` exists but produces an empty 4×8 board.** The types are ready (3-seat `TurnOrder`, `Side(2)`, `BoardShape::ThreeKingdom`), but the actual mask + capture rules ship in PR 2. The setup builder is `setup.rs::build_three_kingdom_stub`. See `backlog/three-kingdoms-banqi.md` for what the implementation needs to settle.

- **WASM build needs `getrandom = { features = ["js"] }`** for `wasm32-unknown-unknown` browser builds. `chess-core/Cargo.toml` adds this as a target-specific dep. Symptom and root cause documented in `pitfalls/wasm-getrandom-unresolved-imp.md`.

- **Move list is `SmallVec<[Move; 32]>`.** Positions exceeding 32 legal moves spill to heap once — fine for correctness but watch when doing AI work.

- **Banqi shuffle determinism.** `RuleSet::banqi_with_seed(house, seed)` uses `ChaCha8Rng` (deterministic). `RuleSet::banqi(house)` falls back to `rand::thread_rng()` for the seed — fine on native; in browser WASM works because of the `js`-feature dep above.

- **Perft is the canary** for move-gen regressions. `tests/xiangqi_perft.rs` locks depth 1 = 44, depth 2 = 1920, depth 3 = 79666 (matches published values). If any of those change, audit the rule edit before assuming the test is wrong.

- **Test fixtures use `.pos` DSL.** Hand-written positions live in `tests/fixtures/<variant>/*.pos` and load via `GameState::from_pos_text(&str)`. `tests/end_conditions.rs` shows the pattern. Format spec: [`docs/snapshot-format.md`](docs/snapshot-format.md). Don't put new test positions inline as Rust code if a fixture file would do — fixtures are editable, diff-friendly, and double as endgame-puzzle source files.

- **Replay = `(initial, moves[])` not `Vec<MoveRecord>`.** `Replay::from_game(state, meta)` walks `state.history` back to the start via `unmake_move` and records the moves. `Replay::play_to(step)` is the single primitive behind animation playback, multi-ply takeback, fork-from-midpoint, and endgame puzzle "start at this position" — don't reinvent any of those.

## Where to put new work

| Kind | Where |
|---|---|
| Deferred features / "maybe later" | `TODO.md` via `scripts/add-todo.sh` (script lives in the [`project-knowledge-harness`](https://github.com/daviddwlee84/agent-skills) skill) |
| Research/design notes for a TODO item | `backlog/<slug>.md` (use `--backlog` on `add-todo.sh`) |
| Past traps you encountered | `pitfalls/<slug>.md` — **title by symptom** (verbatim error), not by root cause |
| Locked-in design decisions | `docs/adr/000N-<slug>.md` |
| Game rules reference | `docs/rules/<variant>.md` |
| Architectural overview / tech analysis | `docs/architecture.md` |

[`AGENTS.md`](AGENTS.md) describes the full backlog/pitfalls workflow with examples. Do **not** create new `ROADMAP.md` / `IDEAS.md` / `BACKLOG.md` files — `TODO.md` is the single index, validated by `scripts/todo-kanban.sh`.

When implementing a `TODO.md` item, in the same commit run:

```bash
scripts/promote-todo.sh --title "<substring>" --summary "<what shipped>"
```

## Pre-push checklist

CI runs all four. Run them locally before pushing:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --target wasm32-unknown-unknown -p chess-core
```

The depth-3 perft (`cargo test --release -p chess-core --test xiangqi_perft -- --ignored`) also runs in CI.
