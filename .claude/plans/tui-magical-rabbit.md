# TUI coordinate-based move input

## Context

Today the chess-tui board can only be driven with `hjkl`/arrows + `Enter` (or
mouse). Power users — and anyone reading move logs in ICCS form — want to
type the move directly. Adopt the same input idiom used everywhere else in
the codebase: `chess_core::notation::iccs::decode_move` (already powers
`chess-cli`'s `> play h2e2`, and `> play 'flip a0'`, `> play 'a3xb3xc3'`).

User-decided shape:

- **`:`** opens an instant prompt — type ICCS, `Enter` commits, `Esc` cancels.
  No board feedback while typing; pure text buffer.
- **`m`** opens a *live preview* prompt — every keystroke re-parses; a 2-char
  prefix (e.g. `h2`) sets `selected = Some(square)` so the existing
  selected-square highlight kicks in; a complete 4-char move (e.g. `h2e2`)
  also moves the board cursor to the destination so the cursor highlight
  previews the target. `Esc` restores the `(cursor, selected)` snapshot
  taken on entry.
- Available in **Local Game and Net** (Net spectators see the same
  `"Spectators cannot move."` message used by the existing flip/move flow).
- Prompt rendered at the **bottom of the sidebar in yellow**, mirroring the
  chat-input idiom (template at `clients/chess-tui/src/ui.rs:1109-1123`).

Scope: keymap, two new dispatch handlers, one shared parser refactor in
chess-core, sidebar rendering, help-line text. No new files.

## Critical files to modify (in order)

1. `crates/chess-core/src/notation/iccs.rs` — add `decode_move_from_view(&PlayerView, &str)` next to the existing `decode_move`. Refactor the legal-move-resolution body into a private `decode_move_inner` that takes board dims + a legal-move slice; both public entries delegate. Add 2 round-trip tests.
2. `clients/chess-tui/src/input.rs` — add `pub enum CoordKind { Instant, Live }`; add `Action::CoordStart(CoordKind)`; map `KeyCode::Char(':')` → `CoordStart(Instant)` and `KeyCode::Char('m')` → `CoordStart(Live)` in the `InputMode::Game` arm at `input.rs:107-122`.
3. `clients/chess-tui/src/orient.rs` — drop `#[allow(dead_code)]` on `project_cell` (line 25). The new live-preview path becomes its first non-test caller.
4. `clients/chess-tui/src/app.rs` — see §State / §Dispatch / §Tests below.
5. `clients/chess-tui/src/ui.rs` — sidebar bottom-hint replacement and HELP_LINES updates.
6. `CLAUDE.md` — extend the input-map paragraph; add a coord-input gotcha after the chat-input one.

## State additions (`app.rs`)

```rust
const COORD_INPUT_MAX: usize = 16;  // longest realistic ICCS ~ "a3xb3xc3xd3"

pub struct CoordInputState {
    pub kind: CoordKind,                                // re-exported from input.rs
    pub buf: String,
    /// Live mode only: (cursor, selected) at entry, restored on Esc.
    pub snapshot: Option<((u8, u8), Option<Square>)>,
}
```

Add a field to **both** views (parallel to `chat_input`):

- `GameView` (`app.rs:80-85`): `pub coord_input: Option<CoordInputState>`
- `NetView`  (`app.rs:115-133`): `pub coord_input: Option<CoordInputState>`

Initialize to `None` in `AppState::new_game` (line 244+) and `new_net` (line 267+).

## `input_mode()` extension (`app.rs:355-371`)

```rust
Screen::Net(n)  if n.chat_input.is_some() || n.coord_input.is_some() => InputMode::Text,
Screen::Game(g) if g.coord_input.is_some()                            => InputMode::Text,
Screen::Game(_) | Screen::Net(_)                                      => InputMode::Game,
```

## Dispatch routing (`AppState::dispatch`, `app.rs:373-454`)

Add:

```rust
Action::CoordStart(kind) => self.dispatch_coord_start(kind),
```

The existing `TextInput | TextBackspace | … | Submit => self.dispatch_text(action)` arm continues to handle the buffer edits — `dispatch_text` is extended below.

## `dispatch_coord_start` (new)

Mirrors `dispatch_chat_start` (line 486-502) but spans both screens.

- **Game**: open unconditionally; engine rejects illegal moves at submit time. On `Live`, snapshot `(cursor, selected)` and clear `selected` so the highlight starts clean. Set `last_msg = "Coord (instant|live): type ICCS, Enter commits, Esc cancels."`.
- **Net**: gate on (a) `chat_input.is_some()` → `last_msg = "Finish or cancel chat (Esc) before move-input."` and bail; (b) `role` (Spectator → "Spectators cannot move."; None → "Not connected yet.") matching the chat-start pattern at lines 495-501; then snapshot+clear identically to Game.

Mutual-exclusion in the other direction: extend `dispatch_chat_start` (Net side) with a symmetric early-return when `n.coord_input.is_some()`.

## `dispatch_text` extension (`app.rs:634-753`)

For **`Screen::Net(n)`** (lines 731-750): branch on which buffer is open:

```rust
Screen::Net(n) => {
    if n.coord_input.is_some()      { Self::dispatch_coord_text_net(n, action); }
    else if n.chat_input.is_some()  { /* existing chat branch verbatim */ }
}
```

For **`Screen::Game(g)`** (no current branch): add

```rust
Screen::Game(g) if g.coord_input.is_some() => {
    let observer = self.observer;
    Self::dispatch_coord_text_game(g, action, observer);
}
```

### `dispatch_coord_text_game` (new method on `AppState`)

```rust
fn dispatch_coord_text_game(g: &mut GameView, action: Action, observer: Side) {
    let Some(ci) = g.coord_input.as_mut() else { return; };
    match action {
        Action::TextInput(c)   => { text_input::push_char(&mut ci.buf, c, COORD_INPUT_MAX);
                                    if matches!(ci.kind, CoordKind::Live) { Self::live_preview_game(g, observer); } }
        Action::TextBackspace  => { text_input::backspace(&mut ci.buf);
                                    if matches!(ci.kind, CoordKind::Live) { Self::live_preview_game(g, observer); } }
        Action::Submit         => {
            let buf = std::mem::take(&mut ci.buf);
            match chess_core::notation::iccs::decode_move(&g.state, buf.trim()) {
                Ok(mv) => { g.coord_input = None; Self::apply_move(g, mv); }
                Err(e) => { if let Some(ci) = g.coord_input.as_mut() { ci.buf = buf; }
                            g.last_msg = Some(format!("Bad move: {e}")); }
            }
        }
        _ => {}
    }
}
```

Submit semantics: success closes the prompt and replays through the existing
`apply_move` (line 875) which already calls `make_move` + `refresh_status` +
sets `last_msg`. Error keeps the prompt open with the buffer restored so the
user can backspace-and-retry — matches typical REPL ergonomics.

### `dispatch_coord_text_net` (new free fn)

Same shape but with the Net pre-flight gates (turn / status / spectator)
copied from `compute_select_outcome` (line 986-1023):

```rust
fn dispatch_coord_text_net(n: &mut NetView, action: Action) {
    let Some(ci) = n.coord_input.as_mut() else { return; };
    match action {
        Action::TextInput(c)  => { text_input::push_char(&mut ci.buf, c, COORD_INPUT_MAX);
                                   if matches!(ci.kind, CoordKind::Live) { live_preview_net(n); } }
        Action::TextBackspace => { text_input::backspace(&mut ci.buf);
                                   if matches!(ci.kind, CoordKind::Live) { live_preview_net(n); } }
        Action::Submit        => {
            let Some(view) = n.last_view.as_ref() else { n.last_msg = Some("Not connected yet.".into()); return; };
            let role = n.role.unwrap_or(NetRole::Player(Side::RED));
            if role.is_spectator()                    { n.last_msg = Some("Spectators cannot move.".into()); n.coord_input = None; return; }
            if !matches!(view.status, GameStatus::Ongoing) { n.last_msg = Some("Game over.".into()); n.coord_input = None; return; }
            if view.side_to_move != role.observer()    { n.last_msg = Some("Not your turn.".into()); return; }  // keep buf
            let buf = std::mem::take(&mut ci.buf);
            match chess_core::notation::iccs::decode_move_from_view(view, buf.trim()) {
                Ok(mv) => { n.coord_input = None; n.selected = None;
                            let _ = n.client.cmd_tx.send(ClientMsg::Move { mv });
                            n.last_msg = Some("Sent.".into()); }
                Err(e) => { if let Some(ci) = n.coord_input.as_mut() { ci.buf = buf; }
                            n.last_msg = Some(format!("Bad move: {e}")); }
            }
        }
        _ => {}
    }
}
```

### Live-preview helper (shared, private fn)

Re-parses `buf` after every keystroke and updates highlight state:

- 0–1 chars or first char invalid → `selected = None`, cursor unchanged.
- 2–3 chars with valid origin square → `selected = Some(origin)`, cursor unchanged.
- 4 chars with valid `from+to` → `selected = origin`, `cursor = orient::project_cell(to, observer, shape)`.
- `from x to` (single hop, e.g. `h2xh9`) → same destination jump as 4-char form.
- `flip <sq>` or chain (5+) → no further preview updates beyond last valid origin.

```rust
// One copy, two thin wrappers (one for &g.state.board, one for &PlayerView).
fn apply_live_preview<F: FnMut(&str) -> Option<Square>>(
    buf: &str, mut parse_sq: F, shape: BoardShape, observer: Side,
    cursor: &mut (u8, u8), selected: &mut Option<Square>,
) { /* logic as above */ }
```

## `dispatch_back` extension (`app.rs:456-484`)

Insert coord-cancel arms **before** the chat-cancel arm:

```rust
Screen::Game(g) if g.coord_input.is_some() => {
    let ci = g.coord_input.take().unwrap();
    if let Some((cur, sel)) = ci.snapshot { g.cursor = cur; g.selected = sel; }
    g.last_msg = None;
}
Screen::Net(n) if n.coord_input.is_some() => {
    let ci = n.coord_input.take().unwrap();
    if let Some((cur, sel)) = ci.snapshot { n.cursor = cur; n.selected = sel; }
    n.last_msg = None;
}
// existing chat-cancel arm follows
```

For Instant mode `snapshot` is `None`, so nothing is restored — cursor and
`selected` were never touched, by design.

## `chess-core` parser refactor (`crates/chess-core/src/notation/iccs.rs`)

Refactor so the legal-move resolution doesn't need a `&GameState`:

```rust
pub fn decode_move(state: &GameState, input: &str) -> Result<Move, CoreError> {
    decode_move_dims(&state.board, &state.legal_moves(), input)
}

pub fn decode_move_from_view(view: &PlayerView, input: &str) -> Result<Move, CoreError> {
    // PlayerView already exposes .shape / .width / .height / .legal_moves (verified).
    // Either reconstruct a Board::empty(view.shape) for parsing, or add a private
    // parse_square_dims(width, height, shape, &str). The cheapest path is:
    let board = Board::empty(view.shape);
    decode_move_dims(&board, &view.legal_moves, input)
}

fn decode_move_dims(board: &Board, legal: &[Move], input: &str) -> Result<Move, CoreError> {
    /* body of current decode_move, but reading legal from the slice instead of
       calling state.legal_moves() */
}
```

`Board::empty(shape)` is already public (used elsewhere); confirm by `grep -n
'fn empty' crates/chess-core/src/board.rs` while implementing.

Tests (next to existing `parse_h2e2_in_xiangqi`):

```rust
#[test] fn decode_move_from_view_round_trips_step() {
    let state = GameState::new(RuleSet::xiangqi());
    let view  = PlayerView::project(&state, state.side_to_move);
    assert_eq!(decode_move_from_view(&view, "h2e2").unwrap(),
               decode_move(&state, "h2e2").unwrap());
}
#[test] fn decode_move_from_view_flip_in_banqi() {
    let state = GameState::new(RuleSet::banqi_with_seed(HouseRules::empty(), 3));
    let view  = PlayerView::project(&state, state.side_to_move);
    assert!(matches!(decode_move_from_view(&view, "flip a0").unwrap(),
                     Move::Reveal { revealed: None, .. }));
}
```

## UI changes (`clients/chess-tui/src/ui.rs`)

### Sidebar bottom hint — Game (lines 921-927)

```rust
} else {
    lines.push(Line::from(""));
    if let Some(ci) = &g.coord_input {
        lines.push(Line::from(vec![
            Span::styled("> ",                   TuiStyle::default().fg(Color::Yellow)),
            Span::styled(format!("{}_", ci.buf), TuiStyle::default().fg(Color::Yellow)),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "?=help, r=rules, : / m=coord, n=new, q=quit",
            TuiStyle::default().fg(Color::DarkGray),
        )));
    }
}
```

### Sidebar bottom hint — Net (around line 1058-1064)

Same pattern using `n.coord_input`. Default-hint text becomes
`"?=help, r=rules, t=chat, : / m=coord, q=quit"`. **Do not** put the coord
buffer in the chat pane — keep the chat pane semantics intact.

### `HELP_LINES` and `HELP_LINES_NET` (lines 1299-1310 and 1143-1155)

Insert after the `Esc` line in each:

```
":               coord input (instant): type ICCS (h2e2 / flip a0), Enter commits",
"m               coord input (live preview): same, with selected/cursor preview",
```

## CLAUDE.md updates

- Input-map paragraph (around line 84-91): append a sentence describing `:` and `m`.
- Gotchas section (after the chat-input gotcha, around line 139): add a "chess-tui coord-input mode hijacks the keymap" entry — note (1) Game + Net both, (2) `:` vs `m` snapshot behaviour, (3) mutual-exclusion with chat-input in Net, (4) `decode_move` (Local) vs `decode_move_from_view` (Net) parsing routes, (5) error keeps prompt open, success closes it, (6) Live mode finally exercises the long-allow-dead-code `orient::project_cell`.

## Tests

### chess-core (`iccs.rs`)
Two view-flavored decoder tests above.

### chess-tui (`app.rs`, new `mod tests`)

There is no existing test module in `app.rs` today — add one. All tests use
`AppState::new_game(RuleSet::xiangqi(), Style::default(), false, Side::RED)`
and exercise pure dispatch state machine (no terminal, no NetClient required).

1. `coord_instant_commits_on_enter` — `:` then `h2e2` then `Submit`; assert `state.history.len() == 1` and `coord_input.is_none()`.
2. `coord_live_sets_selected_at_two_chars` — `m` then `h`/`2`; assert `g.selected == Some(Square(25))` (h2 = file 7, rank 2 in 9-wide → 2*9+7=25).
3. `coord_live_jumps_cursor_at_four_chars` — `m` then `h2e2`; assert `g.cursor == (7, 4)` (e2 projected for Red observer) and not the original (5, 4).
4. `coord_live_esc_restores_snapshot` — manually set `g.cursor = (5, 3)`, then `m`, type `h2e2`, then `Action::Back`; assert cursor and selected restored, `coord_input.is_none()`.
5. `coord_bad_notation_keeps_prompt_open` — `:` then `z9z9` then `Submit`; assert `state.history.len() == 0`, `coord_input.is_some()`, `last_msg.starts_with("Bad move:")`.

Net-side tests defer (NetClient mocking is expensive; the shared helpers are
exercised by Local tests).

## Verification

```bash
# Engine + parser refactor
cargo test -p chess-core --lib notation::iccs::tests

# TUI dispatch state machine
cargo test -p chess-tui

# Workspace + clippy + fmt + WASM
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --target wasm32-unknown-unknown -p chess-core

# End-to-end smoke (manual)
cargo run -p chess-tui -- xiangqi
#   press ':'  → '>' prompt; type h2e2 + Enter → cannon plays
#   press 'm'  → type h2 → e2 highlights as selected; type 'e2' → cursor jumps to e2; Enter commits
#   press 'm'  → type 'zz' → no highlight; Esc → cursor + selected restored

cargo run -p chess-tui -- banqi --preset taiwan --seed 42
#   press ':'  → flip a0  → reveal accepted
#   press 'm'  → 'a0a1' → both squares preview before commit

# Net mode (two terminals or `make play-local`)
cargo run -p chess-net -- --port 7878 xiangqi  &
cargo run -p chess-tui -- --connect ws://127.0.0.1:7878
#   on your turn: ':' → h2e2 + Enter → server accepts, broadcast back
#   on opponent's turn: ':' → 'h2e2' + Enter → "Not your turn." (buf preserved)
#   while typing chat ('t'): pressing ':' does nothing (chat hijacks keys)
#   while typing coord (':'): pressing 't' does nothing (coord hijacks keys)

# Spectator gate
cargo run -p chess-tui -- --connect 'ws://127.0.0.1:7878/ws/main?role=spectator'
#   pressing ':' or 'm' → "Spectators cannot move." last_msg, prompt never opens
```
