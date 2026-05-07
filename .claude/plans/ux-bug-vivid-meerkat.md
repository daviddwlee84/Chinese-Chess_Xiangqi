# Banqi: captured-pieces ("graveyard") panel in sidebar

## Context

During a banqi (暗棋) game — especially when 連吃 chain captures are
active — players can't tell which pieces have already been taken. A
single chain move can capture 2–4 pieces atomically and the only
record is the move log, which lists ICCS notation, not piece glyphs.

Add a "captured pieces" panel to both clients (chess-tui + chess-web),
showing per-side glyphs of every captured piece. Useful for xiangqi
too (material count at a glance), so we ship for both variants.

User asked for two sort orders: chronological (capture time) and by
rank (largest → smallest). Confirmed via AskUserQuestion: ship both
behind a toggle.

## Approach

Capture data already lives in `state.history` — every `Move` variant
that captures preserves the captured `Piece` for `unmake_move`. So we
add a thin engine helper that walks the history and emits a flat
`Vec<Piece>` in chronological order, expose it on `PlayerView`, and
render it in both clients with a sort toggle.

Wire shape: `PlayerView.captured: Vec<Piece>` with `#[serde(default)]`.
This is the same back-compat pattern already used for `in_check` (v4),
`chain_lock` + `current_color` (v5). No protocol bump needed — v5 just
shipped this session and tolerates field additions on `PlayerView` via
the existing serde-default convention. (If the user wants a clean
v5→v6 bump for record-keeping, that's a one-line change to whichever
const tracks the protocol number — surfaced if/when we find it.)

### Engine helper: `GameState::captured_pieces()`

`crates/chess-core/src/state/mod.rs` — add:

```rust
/// All pieces captured so far, in chronological (history) order.
/// Used by `PlayerView` for sidebar rendering. Returns the captured
/// piece's full identity — safe under ADR-0004 because every captured
/// piece was revealed at capture time.
pub fn captured_pieces(&self) -> Vec<Piece> {
    let mut out = Vec::new();
    for record in &self.history {
        match &record.the_move {
            Move::Capture { captured, .. }
            | Move::CannonJump { captured, .. } => out.push(*captured),
            Move::ChainCapture { path, .. } => {
                out.extend(path.iter().map(|hop| hop.captured));
            }
            Move::DarkCapture { revealed, attacker, .. } => {
                let (Some(rev), Some(att)) = (*revealed, *attacker) else { continue };
                match dark_capture_outcome(att, rev, self.rules.house) {
                    DarkCaptureOutcome::Capture => out.push(rev),
                    DarkCaptureOutcome::Trade   => out.push(att),
                    DarkCaptureOutcome::Probe   => {}
                }
            }
            _ => {}
        }
    }
    out
}
```

`dark_capture_outcome` and `DarkCaptureOutcome` are already private to
`state/mod.rs` (added in the prior session — see `state/mod.rs:563`),
so the helper sits in the same file with direct access. No new public
API beyond `captured_pieces` itself.

### View projection

`crates/chess-core/src/view.rs` — add field:

```rust
/// Pieces captured so far (chronological). Always present; clients
/// sort/group as needed. Added in protocol v5.1; older clients see
/// the empty default via `serde(default)`.
#[serde(default)]
pub captured: Vec<Piece>,
```

Populate in `PlayerView::project()`:

```rust
captured: state.captured_pieces(),
```

### chess-tui rendering (`clients/chess-tui/src/ui.rs`)

Add a `Captured` block in both `draw_sidebar` (~L869–979) and
`draw_sidebar_net` (~L981–1145), inserted before the help / chat
region. Two-line content:

```
┌ Captured ────────────────┐
│ R 紅: 兵 兵 馬           │  (red text)
│ B 黑: 卒 卒              │  (dim/dark text)
└──────────────────────────┘
```

Glyphs come from existing `glyph::glyph(kind, side, style)` (TUI
glyph.rs:16). Style by side using `Style::default().fg(Color::Red)`
for red glyphs and the existing dim/black colour for black.
`Paragraph::wrap` handles overflow if a row gets long.

Sort toggle:
- New `AppState` field: `captured_sort: CapturedSort` (`{ Time, Rank }`).
- New keybinding: `g` (graveyard sort) — toggles `Time ↔ Rank`. `g`
  is currently unbound (existing keys: `hjkl`/arrows, Enter/Space,
  Esc, `u`, `f`, `n`, `r`, `?`, `q`, `:`, `m`, `t`, `w`).
- New CLI flag: `--captured-sort time|rank` (default `time`) added to
  the existing arg parser in `clients/chess-tui/src/cli.rs`.
- Preserve the toggle across screen transitions via the existing
  `AppState::replace_preserving_prefs` path (same pattern as
  `--no-confetti` / `--no-check-banner`).

### chess-web rendering

New component `clients/chess-web/src/components/captured.rs`:

```rust
#[component]
pub fn CapturedPanel(#[prop(into)] view: Signal<PlayerView>) -> impl IntoView { … }
```

Renders two rows (Red / Black) with one `<span class="captured-piece">`
per piece. Glyph from `crate::glyph::glyph(...)`, consistent with the
board.

Sort toggle:
- `Prefs` (`clients/chess-web/src/prefs.rs`) gains
  `captured_sort: RwSignal<CapturedSort>` persisted to localStorage
  key `chess.ui.capturedSort` (existing keys are `chess.fx.confetti`,
  `chess.fx.checkBanner`).
- A small button in the panel header swaps between "⏱ Time" and
  "📊 Rank" labels (text-only, no extra icon library).

Wire from `Sidebar` in `components/sidebar.rs` (slot in after the
`legal_count` line, before `sidebar-actions`) AND from
`OnlineSidebar` in `pages/play.rs` (~L255–354). Both consume the same
component because the panel only depends on `view`.

CSS additions to `clients/chess-web/style.css` (alongside existing
`.sidebar` block at L295–325):

```css
.captured-panel { display: grid; gap: 0.25rem; }
.captured-header { display: flex; justify-content: space-between; align-items: baseline; }
.captured-header h4 { margin: 0; font-size: 0.95rem; }
.captured-row { display: flex; flex-wrap: wrap; gap: 0.35rem; align-items: center; font-family: var(--cjk-font, inherit); }
.captured-row.red .captured-piece  { color: var(--red); }
.captured-row.black .captured-piece { color: var(--fg); }
.captured-piece { font-size: 1.05rem; font-weight: 700; }
.captured-toggle { font-size: 0.8rem; padding: 0.1rem 0.5rem; }
```

### Sort logic (shared semantics)

Both clients reuse the same ordering rules so behaviour is consistent:
- `Time`: as returned by `captured_pieces()`, grouped by side at
  render time but each side's row stays in capture order.
- `Rank`: stable-sort each side's slice by `PieceKind` rank
  (General > Advisor > Elephant > Chariot > Horse > Cannon > Soldier).
  Use `chess_core::piece::PieceKind`'s implicit ordering if present;
  otherwise add `pub fn rank_value(kind: PieceKind) -> u8` next to
  the existing banqi rank helper in `crates/chess-core/src/piece.rs`
  (check first — banqi already needs piece ranks for capture rules,
  so this likely already exists as private).

The sort lives in each client's view layer (TUI rendering fn / web
component). The engine just ships the chronological list.

## Files to modify

- `crates/chess-core/src/state/mod.rs` — add `captured_pieces()`.
- `crates/chess-core/src/view.rs` — add `captured` field + projection.
- `crates/chess-core/src/piece.rs` — add `rank_value()` if not already
  exposed (banqi capture rules likely already have it; reuse, don't
  duplicate).
- `crates/chess-core/tests/captured_pieces.rs` — new test file.
- `clients/chess-tui/src/cli.rs` — `--captured-sort time|rank` flag.
- `clients/chess-tui/src/app.rs` — `captured_sort` state + `g` key
  binding + preserve-across-screens.
- `clients/chess-tui/src/ui.rs` — render Captured block in both
  sidebar fns.
- `clients/chess-web/src/components/captured.rs` — new component.
- `clients/chess-web/src/components/mod.rs` — re-export.
- `clients/chess-web/src/components/sidebar.rs` — slot in
  `<CapturedPanel>`.
- `clients/chess-web/src/pages/play.rs` — same in `OnlineSidebar`.
- `clients/chess-web/src/prefs.rs` — `captured_sort` signal +
  localStorage hydration.
- `clients/chess-web/style.css` — `.captured-panel` etc.

## Tests (`crates/chess-core/tests/captured_pieces.rs`)

- Fresh state: `view.captured.is_empty()`.
- Single `Move::Capture`: 1 entry with the right piece.
- `Move::ChainCapture` with 3 hops → 3 entries in path order.
- `Move::CannonJump` → 1 entry (xiangqi).
- DarkCapture outcomes (use a hand-written `.pos` fixture with a known
  attacker/defender pairing so we can force each outcome):
  - Capture outcome → 1 entry (revealed defender).
  - Trade outcome → 1 entry (attacker).
  - Probe outcome → 0 entries.
- Mixed sequence: ordering matches make-move chronology.
- After `unmake_move` of a capture, `captured_pieces().len()` decreases
  by the right amount (1 for `Capture`, N for `ChainCapture` with N
  hops, 0/1 for the three DarkCapture branches).

## Verification

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p chess-core --test captured_pieces
cargo test --workspace
cargo build --target wasm32-unknown-unknown -p chess-core
```

Manual TUI:
```bash
cargo run -p chess-tui -- banqi --house chain,dark,rush,horse --seed 42
# Capture a few pieces. Confirm sidebar Captured panel updates.
# Press 'g' to toggle Time/Rank sort.
# Trigger a 連吃 chain via the chain_lock UX → all hops appear.
# Try a DarkCapture probe → no entry. Trade → attacker enters its row.
```

Manual web:
```bash
make play-web
# /local/banqi?house=chain,dark,rush,horse&seed=42
# Same checks; toggle button in sidebar swaps Time ↔ Rank.
# Refresh the page → toggle preference persists (localStorage).
```

Net + spectator (captured list is public, all observers see the same):
```bash
make play-spectator VARIANT=banqi
# Make captures on the player panes; spectator sees them too.
```
