# ADR 0001: Rust workspace layout

## Context

Greenfield project. Need a layout that supports a pure-logic core, multiple clients (CLI, TUI, Web), and an authoritative server, with WASM as a first-class target. Splitting later is painful; scaffolding now is cheap.

## Decision

One Cargo workspace with eight members:

- `crates/chess-core` — pure logic, no IO
- `crates/chess-engine` — search/eval (stub)
- `crates/chess-net` — wire protocol (stub)
- `crates/chess-ai` — heuristic + ISMCTS (stub)
- `clients/chess-cli` — REPL harness (real, minimal)
- `clients/chess-tui` — ratatui (stub)
- `clients/chess-web` — Leptos + WASM (stub, `cdylib + rlib`)
- `xtask` — automation (stub)

Workspace `[workspace.dependencies]` is the single source of truth for crate versions. Stub crates have a real `Cargo.toml` and a `lib.rs` (or `main.rs`) with one doc comment so `cargo check --workspace` passes from day one.

## Consequences

- `cargo check --workspace` is the canonical "did I break anything" command.
- New variants and clients add a member, not a new repo.
- `chess-core` must remain platform-agnostic — enforced in CI by `cargo build --target wasm32-unknown-unknown -p chess-core`.
- Slight upfront verbosity (eight `Cargo.toml` files at PR 1).
