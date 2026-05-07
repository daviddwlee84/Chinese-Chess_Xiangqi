# TUI menu UX — tree-style custom-rules picker + dynamic rules overlay

## Context

The chess-tui main menu currently exposes only fixed presets (`PickerEntry::ALL`, `clients/chess-tui/src/app.rs:27-77`):
xiangqi standard/strict, banqi purist/Taiwan/aggressive, online, quit. There is no way for a TUI user to mix individual house-rule flags or pick a deterministic seed without dropping out and re-running with CLI args (`--house chain,rush --preset taiwan --seed 42`, parsed at `main.rs:202-233`).

Worse, once in-game, the rules overlay (`r`) shows a static `RULES_LINES` constant (`ui.rs:1641-1668`) that mentions house rules in the abstract ("chain-capture (連吃), chariot-rush (車衝). Other house rules are P1 TODO.") but never tells you **which flags are actually on** for the current game, nor the banqi seed.

This change adds:

1. A **tree-style "Custom rules…" sub-screen** reachable from the picker (one entry per variant), with cursor-navigated radio-buttons (preset), checkbox rows (six house-rule flags, `Space` toggles), and a seed text input. UX modelled on Image #4 (skill-install screen). Banqi gets all six flags + seed; xiangqi gets a Standard/Casual radio.
2. A **rules-aware overlay**: `draw_rules_overlay` becomes dynamic — it takes the live `RuleSet` (and seed if known) and prepends a `[x] / [ ]` summary of the flags that are active for the running game, before the existing generic rules text.

The CLI `--house`/`--preset`/`--seed` path stays untouched (still useful for scripts and `make play-local`); the new screen is purely additive.

## Files to modify

| File | What changes |
|---|---|
| `clients/chess-tui/src/app.rs` | New `Screen::CustomRules(CustomRulesView)` variant; new struct `CustomRulesView`; new `PickerEntry::CustomXiangqi` + `CustomBanqi`; `dispatch_picker` routes the new entries; `dispatch_custom_rules` handles cursor / toggle / start; `input_mode()` returns `InputMode::CustomRules` (or `Text` when seed-edit active) |
| `clients/chess-tui/src/input.rs` | Add `InputMode::CustomRules` variant; `from_key` adds rows for it: `j`/`k`/arrows = `CursorMove`, `Space` = `Toggle`, `h`/`l` = `CycleLeft`/`CycleRight`, `Enter` = `Confirm`, `Esc`/`q` = `Back` |
| `clients/chess-tui/src/ui.rs` | New `draw_custom_rules(f, area, view, style)`; modify `draw_rules_overlay(f, area, rules, variant, seed, style)` signature to accept the live `RuleSet`/variant and build the `[x]/[ ]` summary block above `RULES_LINES`; call sites updated to pass `app.current_rules()` and `app.current_seed()` |
| `clients/chess-tui/src/main.rs` | (Optional) Extract `build_banqi_rules` so `CustomRulesView::to_rule_set` can reuse the same flag→`HouseRules` mapping; otherwise inline the conversion — both fine, this is small code |
| `clients/chess-tui/src/banner.rs` | No changes — `BannerKind::AppTitle` reused above the new sub-screen |

No `chess-core` changes. No protocol bump. No new ratatui widgets — re-uses the `Paragraph` + `Line` + `Span` pattern already in `draw_picker` (`ui.rs:49-85`).

## Design

### 1. Picker entry points

Append two entries to `PickerEntry::ALL` (`app.rs:27-77`), placed right after the last banqi preset:

```rust
PickerEntry::CustomXiangqi,   // label: "Xiangqi (象棋) — custom rules…"
PickerEntry::CustomBanqi,     // label: "Banqi (暗棋) — custom rules…"
```

`PickerEntry::rules()` is unchanged for the existing six entries; the two new ones don't return a `RuleSet` directly — instead `dispatch_picker` (`app.rs:814-845`) treats them specially and pushes `Screen::CustomRules(CustomRulesView::new_xiangqi())` / `::new_banqi(default_preset)`.

### 2. `CustomRulesView` state

```rust
pub struct CustomRulesView {
    variant: VariantKind,            // Xiangqi | Banqi (local enum, not chess_core::Variant — we don't need ThreeKingdom here)
    cursor: usize,                   // index into rows()
    // Xiangqi:
    xiangqi_strict: bool,            // false = casual (default, matches current Xiangqi entry)
    // Banqi:
    banqi_preset: BanqiPreset,       // Purist | Taiwan | Aggressive | Custom
    banqi_flags: HouseRules,         // syncs with banqi_preset; switches to Custom on manual toggle
    banqi_seed: String,              // user-typed seed; empty = thread_rng()
    seed_editing: bool,              // true when cursor on Seed row AND user pressed Enter to start editing
}
```

`rows()` returns a `Vec<CustomRulesRow>` so cursor navigation and rendering share the layout:

```rust
enum CustomRulesRow {
    Header(&'static str),     // not selectable
    Preset(usize),            // index in preset list
    FlagToggle(HouseRules),   // single-bit flag
    SeedInput,
    StartButton,
}
```

`Header` rows are skipped during cursor moves (`dispatch_custom_rules` advances past them).

### 3. RuleSet construction on Enter

`CustomRulesView::to_rule_set(self) -> RuleSet`:
- Xiangqi: `RuleSet::xiangqi()` if `xiangqi_strict`, else `RuleSet::xiangqi_casual()`.
- Banqi: parse `banqi_seed.trim()` — empty → `RuleSet::banqi(banqi_flags)`; non-empty digit string → `RuleSet::banqi_with_seed(banqi_flags, parsed_seed)`; non-numeric → fall through to a `last_msg` error in the view (don't start the game).

Re-uses existing factories in `crates/chess-core/src/rules/mod.rs:56-80`. Same `HouseRules` flags as `house.rs:12-26`. No new public API in `chess-core`.

### 4. Picker → screen transition

`dispatch_picker` (`app.rs:814-845`) extends:

```rust
PickerEntry::CustomXiangqi => self.screen = Screen::CustomRules(CustomRulesView::new_xiangqi()),
PickerEntry::CustomBanqi   => self.screen = Screen::CustomRules(CustomRulesView::new_banqi()),
```

`Esc` from `Screen::CustomRules` returns to `Screen::Picker(PickerView { cursor: <last picker cursor> })` — store the picker cursor on `CustomRulesView` so we restore it. Same pattern as how `HostPrompt` returns to `Picker` today.

### 5. Rendering

`draw_custom_rules(f, area, view, style)` in `ui.rs`. Layout (top to bottom inside one centred `Block::bordered` like the rules overlay):

```
┌ Custom Banqi rules ─────────────────────────────┐
│                                                 │
│   Preset                                        │
│   ▶ ( ) Purist                                  │
│     (•) Taiwan                                  │
│     ( ) Aggressive                              │
│     ( ) Custom                                  │
│                                                 │
│   House rules (Space to toggle)                 │
│     [x] chain      連吃                         │
│     [ ] dark       暗吃                         │
│     [x] rush       車衝                         │
│     [ ] horse      馬斜                         │
│     [ ] cannon     砲快                         │
│     [ ] dark-trade                              │
│                                                 │
│   Seed: [ 42         ]   (空白=隨機)            │
│                                                 │
│   [ Start ]                                     │
│                                                 │
│ [Enter] Start  [Space] Toggle  [Esc] Back       │
└─────────────────────────────────────────────────┘
```

Uses the existing `Paragraph` + bordered `Block` pattern from `draw_picker` (`ui.rs:49-85`) and `draw_rules_overlay` (`ui.rs:1565-1591`). Cursor row gets `▶` prefix + `style::Style::accent`. Disabled (header) rows just plain.

### 6. Dynamic rules overlay

Change signature: `fn draw_rules_overlay(f, area, rules: &RuleSet, seed: Option<u64>, style: &StyleSet)` (was `_style`-only).

Build a prelude `Vec<Line>`:

```
Banqi 暗棋 — current rules
  [x] chain-capture     連吃
  [ ] dark-capture      暗吃
  [x] chariot-rush      車衝
  [ ] horse-diagonal    馬斜
  [ ] cannon-fast       砲快
  [ ] dark-trade
  Seed: 42 (deterministic)

```

For xiangqi, replace the 6-row block with a single row: `Standard self-check` or `Casual (allow self-check)` based on `rules.xiangqi_allow_self_check`. The seed row is omitted for xiangqi.

Then append `RULES_LINES` (the existing static body) so generic xiangqi/banqi explanations are still there.

`GameView` exposes `state.rules: Option<RuleSet>` (`app.rs:136`). For Net, `NetView` already holds `rules: RuleSet` from the server `Welcome` payload (per CLAUDE.md "v3 spectator + chat" gotcha). Add a tiny `app.current_rules() -> Option<&RuleSet>` accessor so `draw_rules_overlay` callers (Game and Net) share one path. Seed is **not** known on the client for Net mode (server doesn't ship it) — pass `None` and the seed row is omitted; for Local mode pass the seed the user typed (store it on `GameView` alongside `rules`, or re-derive from `RuleSet` if a seed accessor is added later — the simplest is to remember it on construction in `new_game`).

### 7. Web parity reference (do **not** modify)

The web client's `parse_house_csv` (`clients/chess-web/src/routes.rs:114-149`) already enumerates the same six tokens (`chain | dark | rush | horse | cannon | dark-trade`). Use the same labels and the same Chinese subtitles in the TUI rows so the two UIs stay coherent. Per CLAUDE.md gotcha (`backlog/promote-client-shared.md`), do **not** promote a shared crate yet — duplicate the few label strings.

## Implementation order

1. **Add `Custom*` picker entries + `Screen::CustomRules` plumbing** (no rendering yet) — verify `dispatch_picker` routes correctly with a `dbg!` and `Esc` returns to picker.
2. **Add `InputMode::CustomRules` + keymap rows in `input.rs`** — verify `j`/`k`/`Space`/`Enter`/`Esc` reach the dispatcher.
3. **Implement `dispatch_custom_rules`**: cursor move, preset cycle, flag toggle (which flips preset to `Custom`), seed digit append/backspace, `Confirm` on `StartButton` builds `RuleSet` and calls existing `new_game` (`app.rs:373`).
4. **Render `draw_custom_rules`** — start with plain text rows, add cursor + checkbox glyphs.
5. **Make rules overlay dynamic** — change signature, build the per-variant `[x]/[ ]` block, prepend to `RULES_LINES`, update both Game and Net call sites.
6. **Persist seed on `GameView`** for the Local case so the overlay can show it (Net stays `None`).

## Verification

```bash
# Build + lint
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p chess-tui

# Picker → Custom Banqi → toggle chain + rush, set seed=42, Enter
cargo run -p chess-tui
# In picker: arrow down to "Banqi (暗棋) — custom rules…", Enter
# In custom screen: Space on chain, Space on rush, k to Seed, type "42", k to Start, Enter
# In game: press `r` — overlay should show:
#   [x] chain-capture     連吃
#   [ ] dark-capture      暗吃
#   [x] chariot-rush      車衝
#   ...
#   Seed: 42 (deterministic)

# Custom Xiangqi → Casual → Enter; press `r` should show "Casual (allow self-check)"
# Custom Xiangqi → Standard → Enter; should show "Standard self-check"

# Esc from custom screen returns to picker with cursor where it was
# Determinism: re-run with seed=42, banqi initial flip layout should match

# CLI args path still works (regression check)
cargo run -p chess-tui -- banqi --preset taiwan --seed 42
cargo run -p chess-tui -- xiangqi --strict

# Net mode: rules overlay still works (no seed shown)
make play-local VARIANT=banqi
# In one of the client panes press `r` — flag list reflects what server picked

# Workspace
cargo test --workspace
cargo build --target wasm32-unknown-unknown -p chess-core
```

No new perft / engine tests needed — this is pure client UX. If we want regression coverage, add a tiny `app.rs` unit test asserting `CustomRulesView::to_rule_set` produces the expected `RuleSet` for each preset + flag combination (mirrors web's `routes.rs` tests).
