# Pass-and-play polish: viewport fit, dual banner, settings relocation, reserved layout, per-seat resign

## Context

Four UX gaps surfaced after testing the freshly-shipped mirror mode on iPad:

1. **iPad overflow.** Mirror mode adds a captured-strip *above* the board, plus
   the existing strip below. On iPad the page now exceeds viewport height —
   the user has to scroll to see the bottom of the board. Pass-and-play on a
   tablet should fit on a single screen.

2. **Mirror mode confetti / banner addresses only Red.** Today a single
   `<EndOverlay>` renders one centred "VICTORY" / "DEFEAT" banner oriented for
   Red. In mirror mode each player needs their own banner facing them —
   one upright, one rotated 180° — saying VICTORY or DEFEAT relative to *that*
   seat.

3. **In-game sidebar is cluttered with two FX toggles** ("Victory effects"
   and "將軍 / CHECK warning"). They're set once and never touched mid-game,
   yet they take up real estate inside every match. Move them to a collapsed
   settings panel on the picker page so the in-game sidebar stays clean.

4. **Captured strip pops in only after the first capture** (`<Show when=any_captured>`),
   which shifts the board down mid-game. Reserve the space upfront so the
   board never moves once a game starts. (Toggle-hint shifts inside the
   sidebar are tolerable — the user explicitly OK'd those.)

5. **Mirror mode has no resign button per seat.** Local pass-and-play has
   no resign button at all today — the sidebar only offers Undo / New game,
   and the sidebar lives on the right edge facing Red. In mirror mode each
   player needs their own resign button reachable from their seat (Black's
   on the top edge, Red's on the bottom edge). Resigning ends the game
   immediately with the *other* side winning — same end-of-game flow as
   checkmate, so the dual-banner work in (2) lights up automatically.

## Approach

### 1. Make pass-and-play fit a single viewport on iPad

**Cause.** `.game-page` is `align-items: start` with no height cap. Board
SVG fills its column at whatever `aspect-ratio` works out, and the two
captured strips stack on top, pushing total height past `100vh` on
iPad-portrait.

**Fix.** Cap the board pane to viewport height in mirror mode and size the
SVG to whatever's left after the two strips.

- **`clients/chess-web/style.css:197–211`** — add a `.game-page--mirror`
  modifier that sets `min-height: 100vh; max-height: 100dvh; align-items:
  stretch`, and inside it a `.board-pane--mirror` that uses
  `display: grid; grid-template-rows: auto 1fr auto;` so the two strips
  pin to top/bottom and the board takes the remaining row.
- **`clients/chess-web/src/components/board.rs`** — pass through a `compact:
  bool` flag (true in mirror mode) that swaps the SVG sizing CSS from
  `width: 100%` to `max-height: 100%; max-width: 100%; width: auto;
  height: auto;` so it shrinks to fit the row instead of forcing height.
- **`clients/chess-web/src/pages/local.rs:640–699`** — when `mirror`,
  add the `--mirror` modifier classes to `.game-page` and `.board-pane`.

Non-mirror layouts unchanged.

### 2. Dual end-game banner + confetti for mirror mode

`EndOverlay` (`clients/chess-web/src/components/end_overlay.rs:74–111`)
renders one `.endgame-overlay` containing `.endgame-banner` + `<Confetti/>`.
For mirror mode we render *two* overlays absolutely-positioned over the
top and bottom halves of the board pane, each addressing the seat that
half faces.

- **`end_overlay.rs`** — accept `mirror: Signal<bool>` and `half: Half`
  (Top / Bottom). Banner copy resolution is already a function of an
  observer role; pass `Some(Side::RED)` for the Bottom overlay and
  `Some(Side::BLACK)` for the Top so each gets the right VICTORY/DEFEAT
  copy. Top overlay gets a `--top` class with `transform: rotate(180deg);
  transform-origin: center;` so it reads upright from the far seat.
- **`style.css:674–751`** — add `.endgame-overlay--top` (top half,
  rotated) and `.endgame-overlay--bottom` (bottom half, upright). Each
  overlay covers ~50% height with `position: absolute; left/right: 0;`.
- **`local.rs:640–699`** — when `mirror`, render
  `<EndOverlay mirror=true half=Top/>` and `<EndOverlay mirror=true
  half=Bottom/>` instead of the single overlay. Confetti renders inside
  both — two independent `<Confetti/>` mounts means twice the particles,
  which is the desired effect (each player sees their own celebration).
  Non-mirror path unchanged.

Spectator / online play does not get this — mirror is xiangqi pass-and-play
only. Existing single-overlay neutral copy ("Red Wins / Black Wins") stays
for online + non-mirror local.

### 3. Move FX toggles from in-game sidebar to picker (collapsed)

The toggles currently sit in `<Sidebar>` (`components/sidebar.rs:146,154–175`)
*and* in the online sidebar (`pages/play.rs:625–642`) — duplicated. The
underlying signals live on `Prefs` (`prefs.rs:27–34`) and persist via
localStorage, so they're already global; the picker is the right home.

- **Remove** `FxToggles` block from `components/sidebar.rs:146,154–175`.
- **Remove** the inline `<div class="fx-toggles">` from `pages/play.rs:625–642`.
- **Add** to `pages/picker.rs` (after the variant / online cards) a
  collapsible **`<details class="picker-settings">`**:

  ```rust
  <details class="picker-settings">
      <summary>"⚙ Display settings"</summary>
      <div class="fx-toggles">
          <label>
              <input type="checkbox" prop:checked=move || prefs.fx_confetti.get()
                  on:change=move |ev| prefs.fx_confetti.set(event_target_checked(&ev))/>
              <span>"Victory effects (confetti + banner)"</span>
          </label>
          <label>
              <input type="checkbox" prop:checked=move || prefs.fx_check_banner.get()
                  on:change=move |ev| prefs.fx_check_banner.set(event_target_checked(&ev))/>
              <span>"將軍 / CHECK warning"</span>
          </label>
      </div>
  </details>
  ```

  Native `<details>` is collapsed by default, accessible, no JS needed.

- **`style.css`** — add `.picker-settings` (margin, summary cursor pointer,
  reuse the existing `.fx-toggles` rule at :768–773).

`Prefs` is already `provide_context`-shared, so the picker reads/writes
exactly the same RwSignals — no plumbing change. In-game code keeps
*reading* `prefs.fx_confetti` / `prefs.fx_check_banner` to gate behaviour;
it just no longer renders the toggles.

### 4. Reserve captured-strip space upfront

Drop the `<Show when=any_captured>` gate so the strip is always mounted
once the game starts. `.captured-row` already has `min-height: 1.7rem`
(`style.css:316`); pieces simply fill in as they die.

- **`clients/chess-web/src/components/captured.rs:64,76,95,106`** — remove
  the three `<Show when=any_captured>` wrappers. Keep the rows; they
  render an em-dash placeholder via the existing `captured-empty` span
  (lines 141–142) when the side has no captures yet.
- **`style.css:290–296`** — `.captured-strip` keeps its current padding
  + dashed border, so an empty strip still looks like a placeholder shelf
  rather than empty space. No CSS change needed; the row min-height
  already reserves the slot.

This also makes (1) easier — the layout calculation is stable from turn 0,
not just after first capture.

### 5. Per-seat resign buttons in mirror mode

Online play already has resign (`pages/play.rs:564–569` dispatches
`ClientMsg::Resign`); local mode has no equivalent. Add a local
`resign(side)` helper on `GameState` and wire two buttons inside the
mirror board pane.

- **`crates/chess-core/src/state/mod.rs`** — add a small inherent method:

  ```rust
  pub fn resign(&mut self, loser: Side) {
      let winner = /* the other seat from TurnOrder */;
      self.status = GameStatus::Won { winner, reason: WinReason::Resignation };
  }
  ```

  `GameStatus::Won` and `WinReason::Resignation` already exist
  (`state/mod.rs:24,32`); we just need a clean entry point so the web
  layer doesn't reach into private fields. Two-player today (Red/Black);
  for three-kingdom banqi we resign the seat and let the existing
  `refresh_status` decide if the game is now over (out of scope here —
  three-kingdom isn't shipped). Add a unit test in
  `crates/chess-core/tests/end_conditions.rs` (resign Red → Black wins
  with `WinReason::Resignation`; resign while game already over is a
  no-op).

- **`clients/chess-web/src/pages/local.rs`** (~640–699 board pane) —
  inside `.board-pane--mirror`, render two new buttons gated on `mirror`:

  ```rust
  <button class="resign-btn resign-btn--top"
          on:click=move |_| { game.update(|gs| gs.resign(Side::BLACK)); }>
      "投降 Resign"
  </button>
  <button class="resign-btn resign-btn--bottom"
          on:click=move |_| { game.update(|gs| gs.resign(Side::RED)); }>
      "投降 Resign"
  </button>
  ```

  After `resign(...)` we still call `state.refresh_status()` (already the
  pattern for moves) so any downstream listeners pick up the new status.
  Disabled when `status != Ongoing`.

- **`clients/chess-web/style.css`** — `.resign-btn--top` is
  `position: absolute; top: 0.5rem; right: 0.5rem; transform:
  rotate(180deg);` so Black reads it upright; `.resign-btn--bottom` is
  `bottom: 0.5rem; right: 0.5rem;` upright. Small / muted styling so
  they don't draw eyes mid-game; clearer hover state.

- Non-mirror local pass-and-play: keep no resign button (status quo).
  Adding one to non-mirror local is a separate decision; the user only
  asked for mirror.

## Critical files

| File | Change |
| --- | --- |
| `clients/chess-web/style.css:197–211` | `.game-page--mirror` viewport-fit grid |
| `clients/chess-web/style.css:674–751` | `.endgame-overlay--top/--bottom` halves |
| `clients/chess-web/style.css` (new, near :768) | `.picker-settings` for the `<details>` panel |
| `clients/chess-web/src/components/board.rs` | `compact: bool` prop → SVG sizing variant |
| `clients/chess-web/src/components/end_overlay.rs:27–129` | `half` + `mirror` props, dual-render variant |
| `clients/chess-web/src/components/captured.rs:64,76,95,106` | drop `<Show when=any_captured>` wrappers |
| `clients/chess-web/src/components/sidebar.rs:146,154–175` | remove `FxToggles` |
| `clients/chess-web/src/pages/play.rs:625–642` | remove inline FX-toggle block |
| `clients/chess-web/src/pages/picker.rs` | new `<details class="picker-settings">` panel |
| `clients/chess-web/src/pages/local.rs:640–699` | mirror-modifier classes + dual `<EndOverlay>` + per-seat resign buttons |
| `crates/chess-core/src/state/mod.rs` | `pub fn resign(&mut self, loser: Side)` helper |
| `crates/chess-core/tests/end_conditions.rs` | resign-then-status test (paired positive/negative per CLAUDE.md "Rule changes touch three places") |
| `clients/chess-web/style.css` | `.resign-btn--top/--bottom` placement + rotation |

Reuses (no change):
- `Prefs` (`prefs.rs:27–34`) — already shared via context, already persists.
- `Confetti` (`components/confetti.rs:18–46`) — stateless, mount twice.
- `BannerKind` resolution (`end_overlay.rs:74–92`) — already role-aware.
- `captured-empty` placeholder span (`components/captured.rs:141–142`).

## Verification

```bash
cargo check --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --target wasm32-unknown-unknown -p chess-web

# Manual UI
make play-web
# 1. Picker — `⚙ Display settings` collapsed by default. Expand → toggle
#    confetti off → start xiangqi pass-and-play → reach end → no confetti
#    or banner. Sidebar in-game must NOT show FX toggles anywhere.
# 2. Xiangqi → Pass-and-play → 鏡像黑方 → Start. On iPad-portrait (or
#    browser at ~1024×768): full page fits in viewport, no scrollbar.
#    Captured strips visible above AND below from move 0 (em-dash
#    placeholder), no board jump on first capture.
# 3. Play to checkmate (or resign): TWO banners — top rotated 180° with
#    that seat's VICTORY/DEFEAT copy, bottom upright with the other seat's.
#    Confetti rains in both halves.
# 4. Non-mirror xiangqi pass-and-play, vs-AI, online — single banner +
#    single confetti, layout unchanged.
# 5. Mirror mode: tap Red's "投降" → bottom shows DEFEAT (from Red's POV),
#    top (rotated) shows VICTORY (from Black's POV). Reverse for Black's
#    button. Both confetti bursts fire. Buttons disable post-resign.
```
