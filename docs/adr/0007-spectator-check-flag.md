# ADR 0007 — `PlayerView.in_check` flag (protocol v4)

Status: accepted (shipped 2026-05-07)
Supersedes: nothing
Related: ADR-0004 (PlayerView projection), ADR-0006 (spectators + chat)

## Context

`GameState::is_in_check(side)` has always been public on `chess-core`, but
the chess-tui local renderer was the only consumer (`ui.rs::draw_sidebar`
shows a "⚠ CHECK 將軍" line when the side-to-move's general is attacked).
Two new clients want the same hint:

1. chess-tui in **Net mode**: `draw_sidebar_net` only has a `PlayerView` in
   hand, not a `GameState`, so it cannot call `is_in_check` itself.
2. chess-web (both Local and Net): the WASM crate operates on `PlayerView`
   for parity between modes.

Three options were considered:

A. **Send the full `GameState` over the wire** — rejected, breaks ADR-0004
   (banqi hidden-piece identity must never leave the server).
B. **Recompute on the client** by porting `is_in_check` to operate on
   `PlayerView` — possible (xiangqi has no hidden pieces), but doubles the
   surface area for a check-detection bug and risks server/client drift.
C. **Project `in_check` at the server and ship it as a derived field on
   `PlayerView`** — server already runs `is_in_check` during status refresh,
   so the marginal cost is one bool per `Update` frame.

## Decision

Pick C. Add `pub in_check: bool` (with `#[serde(default)]`) to `PlayerView`,
computed in `PlayerView::project()` as `state.is_in_check(observer)`. The
field is wire-compatible with v3 — older clients deserialize unknown fields
as ignored, and older servers' messages deserialize cleanly into v4 clients
with `in_check = false`. Bump `PROTOCOL_VERSION` to 4 for documentation.

For non-xiangqi variants, `is_in_check` already returns `false` (banqi has
no general), so observers always read `in_check = false` — no variant gate
in `project()`. Spectators project from `Side::RED` (per ADR-0006); they'll
see `in_check = true` whenever Red is in check and `false` otherwise. UIs
treating spectators as neutral observers should ignore the flag entirely
when rendering banners.

## Consequences

- One extra `bool` per server message — negligible bandwidth.
- `chess-tui` Net mode and `chess-web` (Local + Net) now share the same
  banner-trigger condition: `view.in_check && variant == Xiangqi`.
- Future variants that introduce a check-like concept (e.g. three-kingdom
  banqi if it grows a general) get the field for free once `is_in_check`
  learns about them.
- Adds the field to the `view_projection.rs` proptest's no-leak surface;
  `bool` carries no piece identity so the proptest stays happy without
  edits.
