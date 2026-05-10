# Picker polish: Banqi seed overflow fix + Xiangqi pass-and-play mirror mode

## Context

Three small UX gaps in `clients/chess-web` surfaced during PWA mobile testing:

1. **Banqi "Seed (optional)" input overflows** the fieldset on tablet/phone-width
   viewports — the `<input>` extends past its parent column (visible in the
   user's iPad screenshot). Root cause: `.text-input` (style.css:563) has no
   `max-width`, and the input's intrinsic width can exceed the column when the
   picker grid hits its `minmax(220px, 1fr)` lower bound.

2. **Xiangqi pass-and-play needs a mirror mode.** Two players sharing one phone
   sitting opposite each other currently both see the board with Red at the
   bottom — fine for Red, upside-down for Black. Adding an opt-in "Mirror Black
   side" toggle rotates Black's piece glyphs 180° so the player on the far seat
   reads their pieces upright. Scoped to xiangqi pass-and-play (not vs-AI, not
   banqi, not online).

3. **In mirror mode, captured pieces should sit on each player's side** rather
   than stacked together below the board. With one player on each end of the
   device, each player's "trophy shelf" (the opponent's pieces they've taken)
   should be visible directly in front of them.

Out of scope: rotating the *board grid* 180° on Black's turn (more disruptive
mid-game). Mirror mode here only flips piece glyphs, not the coordinate system.

## Approach

### 1. Banqi seed input overflow — pure CSS fix

`.text-input` should never grow past its container. Add `width: 100%` +
`max-width: 100%` + explicit `box-sizing: border-box` to the base rule so all
picker inputs honour their column.

- **`clients/chess-web/style.css:563-570`** — extend `.text-input`:
  ```css
  .text-input {
    background: var(--bg-elev);
    color: var(--ink);
    border: 1px solid var(--grid);
    border-radius: 0.4rem;
    padding: 0.5rem 0.75rem;
    font-size: 0.95rem;
    width: 100%;
    max-width: 100%;
    box-sizing: border-box;
  }
  ```
  (`.depth-input` at :576 already does this — promoting to the base avoids
  per-input duplication.) No markup change needed in `picker.rs:419-430`.

### 2. Mirror mode — opt-in checkbox + Black-glyph 180° rotation

**Picker UI** — `clients/chess-web/src/pages/picker.rs`

- Add `mirror: RwSignal<bool>` next to `strict` / `ai_show_hints` in
  `XiangqiCard` (mirrors the pattern at picker.rs:46-87).
- Add a checkbox **inside** the existing Mode group, gated on
  `mode == PassAndPlay`. Label: "鏡像黑方 — flip Black's pieces 180° for a
  player sitting opposite (pass-and-play only)."
- Pass `mirror.get()` into `LocalRulesParams` so the URL query carries it.

**URL plumbing** — `clients/chess-web/src/routes.rs`

- Add `pub mirror: bool` to `LocalRulesParams` (~routes.rs:110) with
  `mirror: false` in `Default`.
- `parse_local_rules()` (~routes.rs:206): parse `?mirror=1` / `mirror=true`.
- `build_local_query()` (~routes.rs:250): emit `mirror=1` only when true.
- Round-trip unit test in the existing `mod tests`.

**Render path** — `clients/chess-web/src/components/board.rs:253-295`

- Thread `mirror: Signal<bool>` into the `<Board>` component props.
- In the per-piece `<g transform=...>` block (~:281), when
  `mirror.get() && piece.side == Side::BLACK`, append `rotate(180 cx cy)`
  to the existing translate so just that glyph spins in place. SVG transform
  composition handles this without restructuring the group.
- `orient::project_cell` is untouched — coordinate math unchanged; only the
  visual glyph for Black pieces flips.

**State plumbing** — `clients/chess-web/src/pages/local.rs:23-32,114-120`

- Add a `mirror: bool` field to `VsAiConfig` (or a sibling `PassAndPlayConfig`).
- `LocalGame` accepts the flag, threads into `<Board mirror=... />` and
  `<CapturedStrip mirror=... />`.

Banqi and three-kingdom ignore the flag.

### 3. Captured pieces split to opposing edges in mirror mode

`<CapturedStrip>` (`components/captured.rs:17-62`) currently stacks both rows
under the board. We add a mirror-aware variant that splits them.

**Component change** — `components/captured.rs`

- Accept `mirror: Signal<bool>`.
- Default (mirror off) → keep current single-strip-below behaviour.
- Mirror on → expose two slot components: `CapturedAbove` (Red pieces taken
  by Black, rendered above the board, **rotated 180°** so the Black player
  reads them upright) and `CapturedBelow` (Black pieces taken by Red, below
  the board, upright as today).

**Layout change** — `clients/chess-web/src/pages/local.rs`

- Mirror off: `<Board>` then `<CapturedStrip>` (today).
- Mirror on: `<CapturedAbove>`, `<Board>`, `<CapturedBelow>` — each captured
  row directly adjacent to the side facing its player.

**CSS** — `clients/chess-web/style.css:287-354`

- Add `.captured-row--mirrored` modifier with `transform: rotate(180deg);
  transform-origin: center;` so glyphs in the top strip read upright from the
  opposite end of the device.
- No `.game-page` grid-template change — captured rows stay in the board
  column, just placed before/after it.

`split_and_sort_captured()` (`state.rs:133-143`) already returns `(red, black)`
— no engine-side change.

## Critical files

| File | Change |
| --- | --- |
| `clients/chess-web/style.css:563-570` | `.text-input` width/max-width fix |
| `clients/chess-web/style.css:287-354` | `.captured-row--mirrored` rotation modifier |
| `clients/chess-web/src/routes.rs:105-176` | `LocalRulesParams::mirror` + parse/build |
| `clients/chess-web/src/pages/picker.rs:46-87,~280` | XiangqiCard mirror checkbox (pass-and-play gate) |
| `clients/chess-web/src/pages/local.rs:23-32,114-120` | thread `mirror` flag into LocalGame |
| `clients/chess-web/src/components/board.rs:253-295` | rotate Black piece glyphs when `mirror && side == BLACK` |
| `clients/chess-web/src/components/captured.rs:17-62` | mirror-aware split rendering |

Reuses (no change):
- `state.rs::split_and_sort_captured()` — already side-split.
- `orient::project_cell` — coordinates untouched; mirror is glyph-only.
- `glyph::glyph()` — same glyph table; rotation is an SVG transform on top.
- `Prefs` context — mirror is per-game (URL param), not a persisted global.

## Verification

```bash
# Unit tests (routes round-trip)
cargo test -p chess-web --lib routes::tests

# Native workspace check
cargo check --workspace

# WASM build still clean
cargo build --target wasm32-unknown-unknown -p chess-web

# Manual UI
make play-web
# 1. http://localhost:8080/ — Banqi column, narrow window to ~220px. Seed
#    input must stay inside its fieldset (no overflow, no column scrollbar).
# 2. Xiangqi → Pass-and-play → check 鏡像黑方 → Start. URL has ?mirror=1.
#    Black pieces render 180°-rotated. Captured strip above the board shows
#    captured RED pieces rotated 180°; strip below shows captured BLACK
#    upright. Make a capture from each side to confirm both populate.
# 3. Vs-AI mode: mirror checkbox MUST NOT appear.
# 4. Banqi / three-kingdom picker: no mirror checkbox.
# 5. Pre-push
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
