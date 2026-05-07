# Promote orient.rs / glyph.rs to a shared client crate

## Why this is in backlog

`clients/chess-tui/src/orient.rs` and `clients/chess-web/src/orient.rs`
are byte-identical (same for `glyph.rs`). Both tests too. The duplication
is intentional — see the note at the top of each web-side copy: a third
client and/or a first divergence is the trigger to consolidate.

## Why we duplicated rather than promoted into chess-core

CLAUDE.md is explicit: "chess-tui orientation lives in
`clients/chess-tui/src/orient.rs`, not chess-core. The engine stays
presentation-free." That rule comes from ADR-0001 (workspace layout).
Banqi transposition and per-observer flipping are presentation concerns;
the engine returns the model coords and `BoardShape`. So a shared crate
between *clients* is the right boundary, not chess-core.

## What good looks like

A new `clients/chess-client-shared/` crate (or `crates/chess-presentation`,
if we'd rather group with chess-core / chess-net under crates/). Public
surface:

- `pub fn display_dims(shape) -> (rows, cols)`
- `pub fn project_cell(sq, observer, shape) -> (row, col)`
- `pub fn square_at_display(row, col, observer, shape) -> Option<Square>`
- `pub fn glyph(kind, side, style) -> &'static str`
- `pub fn hidden(style) -> &'static str`
- `pub fn side_name(side, style) -> &'static str`
- `pub enum Style { Cjk, Ascii }`

Both chess-tui and chess-web depend on it. Tests move to the shared crate
(or stay in both with the shared crate as the source of truth).

## Trigger

- A third client (e.g. mobile native via tauri) wants the same logic.
- The first time `orient.rs` or `glyph.rs` diverges between TUI and web
  for any reason — at that point the cost of keeping them in sync exceeds
  the cost of factoring them out.

## Cost estimate

Mechanical refactor: ~30 minutes including rewiring `Cargo.toml` deps,
moving the test files, updating CLAUDE.md gotchas, and re-running the
pre-push gates. No behavior change.
