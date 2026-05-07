# Confetti + Big Banner + Check Toggle (TUI & Web)

## Context

Right now both clients show end-of-game outcome as a few quiet lines in the sidebar (`game_over_banner` in chess-tui, `Sidebar` `<p class="status won">` in chess-web). The user wants wins/losses to feel obvious — a confetti effect plus a big "VICTORY / DEFEAT / DRAW" banner — and wants the existing "將軍 / CHECK" warning (already shown only in TUI Local for xiangqi) elevated and made toggleable across both clients. Also a small picker-page polish in the same PR. No new crate dependencies (banner ASCII art is hand-rolled; confetti uses ratatui spans / pure CSS).

Locked decisions from the clarifying round: hand-rolled ASCII art • check banner default ON • spectators get confetti with neutral "Red wins" copy (never "VICTORY") • menu polish in same PR but minimal.

## Approach

Three-phase rollout: chess-core wire change first, then chess-tui, then chess-web. Each phase is independently testable.

### Phase 1 — chess-core / chess-net (wire)

**1. Add `in_check` to `PlayerView`** — `crates/chess-core/src/view.rs`
- Add `pub in_check: bool` with `#[serde(default)]`. In `project()` (line 40), compute `state.is_in_check(observer)`. Returns `false` for Banqi naturally (existing `is_in_check` already handles missing-general).
- Tests: extend `view.rs::tests` with one xiangqi-not-in-check, one xiangqi-in-check, one banqi-always-false.
- ~15 LOC + 30 LOC tests.
- Wire-compatible with v3 clients via `serde(default)` — but bump anyway for clarity.

**2. Bump protocol** — `crates/chess-net/src/protocol.rs`
- `pub const PROTOCOL_VERSION: u32 = 3` → `4`. Update the doc-comment changelog block.
- Add a one-line ADR-0007 entry under `docs/adr/0007-spectator-check-flag.md` (3–5 sentences) noting the in_check field addition.

### Phase 2 — chess-tui

**3. Hand-rolled banner module** — new `clients/chess-tui/src/banner.rs`
- Five const arrays of 5–6 string rows: `VICTORY`, `DEFEAT`, `DRAW`, `CHECK_EN`, `CHECK_CN` (將軍). Block-style using `█▓░` (CJK style) or `# - +` (ASCII style).
- `pub fn art(kind: BannerKind, style: Style) -> &'static [&'static str]`.
- ~150 LOC (mostly data).

**4. Confetti animation** — new `clients/chess-tui/src/confetti.rs` + `app.rs`, `main.rs`
- `Particle { x: f32, y: f32, vx: f32, vy: f32, glyph: char, color: Color }`. `ConfettiAnim { particles: Vec<Particle>, started_at: Instant }`.
- `spawn(area_w, area_h)` — 30–50 particles, vy in `[-2.0, -0.8]`, vx in `[-0.6, 0.6]`, glyphs `*✦◆●+`, colors yellow/red/cyan/green.
- `step(&mut self) -> bool` (returns `done`): `vy += 0.4`, advance, drop offscreen, expire after 3s.
- AppState gets `pub confetti_anim: Option<ConfettiAnim>`, `pub show_confetti: bool`, `pub show_check_banner: bool`, `pub prev_status_local`/`prev_status_net` for transition detection.
- `main.rs:225` loop: when `confetti_anim.is_some()`, set `poll_ms = 50` (else 60/200 as today). Call `confetti_anim.step()` each iteration before `terminal.draw`.
- Trigger: in `app.dispatch` (Local) and `app.tick_net` (Net), detect `prev_status == Ongoing && new_status != Ongoing` → `confetti_anim = Some(spawn(...))` if `show_confetti`.

**5. Game-over overlay + extend Net check banner** — `clients/chess-tui/src/ui.rs`
- New `draw_game_over_overlay(frame, board_area, kind, role, style)` rendering ASCII art centered over the board (a `Paragraph` inside a sub-Rect). Lives ~3s — driven by the same `confetti_anim` lifetime so they appear together. After overlay collapses, existing sidebar `game_over_banner` (line 1173) keeps showing.
- Spectator branch picks neutral `BannerKind::Outcome { winner: Side }` rendering "RED WINS" instead of "VICTORY"/"DEFEAT".
- Render confetti particles **after** `draw_board` by writing into the frame's buffer (overwrite cells in board area). Skip cells outside `board_area`.
- Extend `draw_sidebar_net` (line 940): mirror the existing `is_in_check` line from `draw_sidebar` (line 884), reading from `view.in_check`. Gate both Local + Net renders behind `app.show_check_banner`.

**6. CLI flags** — `clients/chess-tui/src/main.rs:40` (`Cli`) + `app.rs:212`
- `#[arg(long)] no_confetti: bool`, `#[arg(long)] no_check_banner: bool`. Wire `app.show_confetti = !cli.no_confetti`, `app.show_check_banner = !cli.no_check_banner`. Default both ON.

**7. TUI picker polish** — `clients/chess-tui/src/ui.rs:46 draw_picker`
- 5-row ASCII title block above current entries (use `banner::art(BannerKind::AppTitle, style)`). Adds `BannerKind::AppTitle` variant. ASCII fallback uses `+/-` chars.

### Phase 3 — chess-web

**8. Prefs module** — new `clients/chess-web/src/prefs.rs`; init in `app.rs`
- `RwSignal<bool>` × 2: `fx_confetti`, `fx_check_banner`. Hydrate from `localStorage` keys `chess.fx.confetti` / `chess.fx.checkBanner` (default `"1"`). Persist on change via `create_effect`. Provide via `provide_context(Prefs { ... })`. Wrap all `web_sys::window()` calls so native-stub crate keeps building.

**9. End-game overlay** — new `clients/chess-web/src/components/end_overlay.rs`; mount in `pages/local.rs` + `pages/play.rs`
- Reuse WIP-overlay CSS (`style.css:201-228`) — copy `.endgame-overlay` / `.endgame-banner` from it. Big `<h1>` text "VICTORY"/"DEFEAT"/"DRAW" with `@keyframes pulse-banner`. Below: winner side + reason in smaller text. Auto-fades after 3s via `set_timeout` (kept simple; no dismiss button).
- Spectator (`role.is_spectator()`) renders neutral "Red wins" / "Black wins" — never "VICTORY"/"DEFEAT".
- Watches `view.status` via `create_effect` with a `prev_status` `RwSignal` owned by the page (survives remounts).

**10. CSS confetti + pulse** — `clients/chess-web/style.css`
- `@keyframes confetti-fall` (translateY 0→110vh + rotate 0→720deg over 3s), `@keyframes pulse-banner` (scale 1↔1.05).
- `.confetti-container { position: fixed; inset: 0; pointer-events: none; z-index: 50; }`, `.confetti-particle` consumes `--x`, `--delay`, `--color`, `--rotate-end` CSS vars.
- ~60 LOC CSS.

**11. Confetti component** — new `clients/chess-web/src/components/confetti.rs`
- Renders 40 `<div class="confetti-particle" style="--x: 23%; --delay: 0.2s; --color: #d43">`. Random params come from `js_sys::Math::random()` (already in WASM env). Mounted by `<EndOverlay>` only when `prefs.fx_confetti` is true. Auto-removes via the same 3s timer.

**12. Web check badge** — `clients/chess-web/src/components/sidebar.rs:43-64`
- Add `<div class="check-badge">將軍 / CHECK</div>` above turn-row, shown when `prefs.fx_check_banner && view.in_check`. CSS uses the same `pulse-banner` @keyframes at lower amplitude.
- Local mode: feed `view` from `PlayerView::project(&state, side)` after each move (already happening per CLAUDE.md). The new `in_check` field flows through automatically.

**13. Web settings + picker hero** — sidebar (or new `settings_menu.rs`) + `pages/picker.rs:8`
- Two `<label><input type="checkbox">` toggles in sidebar footer bound to `prefs.fx_confetti` / `prefs.fx_check_banner`.
- Picker: add `<header class="hero">` above `.picker-grid` with inline SVG of a 帥 glyph + `<h1>Chinese Chess 中國象棋</h1>` + tagline. ~50 LOC including CSS.

## Critical files to modify

| File | What changes |
|---|---|
| `crates/chess-core/src/view.rs` | Add `in_check: bool` to `PlayerView` + tests |
| `crates/chess-net/src/protocol.rs` | Bump `PROTOCOL_VERSION` to 4 |
| `clients/chess-tui/src/banner.rs` (new) | ASCII art constants + lookup fn |
| `clients/chess-tui/src/confetti.rs` (new) | Particle physics |
| `clients/chess-tui/src/app.rs:212` | New AppState fields, transition detection |
| `clients/chess-tui/src/main.rs:40,225` | CLI flags, dynamic poll interval |
| `clients/chess-tui/src/ui.rs:46,834,884,940,1173` | Picker title, overlay draw, Net check line |
| `clients/chess-web/src/prefs.rs` (new) | localStorage-backed signals |
| `clients/chess-web/src/components/end_overlay.rs` (new) | Overlay component |
| `clients/chess-web/src/components/confetti.rs` (new) | Particle DOM |
| `clients/chess-web/src/components/sidebar.rs:43-64` | Check badge + settings toggles |
| `clients/chess-web/src/pages/picker.rs:8-24` | Hero header |
| `clients/chess-web/style.css` | `@keyframes`, `.confetti-*`, `.endgame-*`, `.check-badge`, `.hero` |
| `docs/adr/0007-spectator-check-flag.md` (new) | Brief ADR for the wire change |

## Reusable primitives (don't reinvent)

- `GameState::is_in_check(side)` — `crates/chess-core/src/state/mod.rs:232`. Works in strict + casual; returns `false` for banqi.
- `game_over_banner()` — `clients/chess-tui/src/ui.rs:1173`. Keep as the post-overlay sidebar text.
- WIP-overlay CSS — `clients/chess-web/style.css:201-228` and Leptos `<Show when=... >` pattern at `clients/chess-web/src/pages/local.rs:136`. Copy the structure for `<EndOverlay>`.
- `glyph::side_name(side, style)` — both clients. For neutral spectator banner copy.
- CLI plumbing pattern — `--no-color` / `--style` in `main.rs:40` → `AppState` fields → renderer reads. Follow the same path for `--no-confetti` / `--no-check-banner`.

## Verification

**After Phase 1:**
```bash
cargo test -p chess-core view::tests
cargo check --workspace
cargo build --target wasm32-unknown-unknown -p chess-core
```

**After Phase 2 (chess-tui):**
```bash
cargo build -p chess-tui
cargo clippy -p chess-tui -- -D warnings

# Visual smoke tests:
make play-local                     # play to checkmate → expect confetti + VICTORY banner ~3s
make play-local VARIANT=banqi       # confirm no check banner (banqi has no general)
make play-spectator                 # third pane = spectator → expect neutral "RED WINS" + confetti
cargo run -p chess-tui -- --no-confetti --no-check-banner xiangqi   # gates work
cargo run -p chess-tui -- --style ascii xiangqi                      # ASCII art fallback renders

# Resign + stalemate paths:
# - In game: type ":" then a self-checkmating move (casual) → GeneralCaptured
# - In Net mode: send Resign from sidebar → Defeat banner on resigner, Victory on opponent
```

**After Phase 3 (chess-web):**
```bash
cd clients/chess-web && trunk serve
# Browser checks:
# - /local/xiangqi → play to checkmate, confirm overlay + confetti + check badge during play
# - Toggle both checkboxes in sidebar; reload → localStorage persists
# - /lobby → /play/<room> as spectator → expect neutral banner, no "VICTORY"
# - / (picker) → hero block visible

cargo build --target wasm32-unknown-unknown -p chess-web
trunk build --release    # ensure prod bundle
```

**Pre-push (all four green):**
```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --target wasm32-unknown-unknown -p chess-core
```

## Out of scope / deferred

- `figlet-rs` dep (rejected — hand-rolled is enough).
- Sound effects (no audio pipeline anywhere yet).
- In-game rules-edit modal (already tracked in `backlog/web-ingame-rules-modal.md`).
- Bigger picker redesign (style preview, rules panel) — file as P2 in `TODO.md` if wanted.
- Threefold-repetition draw banner — depends on the existing TODO for repetition detection.
- Persisted post-game card with "share result" / replay link — nice-to-have for later.

## Risks / callouts

- **Confetti in narrow terminals**: skip overlay if `board_area.width < art_max_width + 4`; just play confetti without big banner.
- **Net status-transition detection**: don't fire confetti on every server push — gate on `prev_status == Ongoing && new != Ongoing`. Reset `prev_status` on rematch.
- **Spectator UX**: `ClientRole::Spectator` already exists in `clients/chess-web/src/state.rs` (per CLAUDE.md); make sure overlay reads it from page state, not from a stale signal.
- **Wire compat**: `serde(default)` on `in_check` lets v3 clients deserialize v4 messages cleanly. The protocol bump is for clarity, not enforcement — the handshake already accepts version mismatches by reading what it understands.
- **localStorage in SSR / native**: gate every `web_sys::window()` access — prefs module must compile on `cargo check --workspace` (native stub).
