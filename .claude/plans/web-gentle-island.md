# chess-net — in-game chat + spectator mode

## Context

`chess-net` ships multi-room WS today (`PROTOCOL_VERSION = 2`), but a room is hard-capped at two seats: the third connection receives `ServerMsg::Error{"room full"}` and the socket closes. There's no chat — players can play and resign, that's it.

The user wants two additions for online play, without disturbing the chess UX:

1. A small chat window so the two players can talk during a game.
2. Allow third-and-beyond connections to join as spectators (read-only board, no moves).

Both clients ship together (chess-web **and** chess-tui). Per user direction:
- Players-only chat — spectators can read but not post.
- Last 50 messages persist for the room's lifetime so late joiners (especially spectators) see context.

This is a back-compat-aware protocol bump (v2 → v3). v2 clients keep working as players via the existing `Hello` path; spectator mode is opt-in via a query param so v2 clients never trip into an unknown welcome variant.

## Recommended approach

### Wire protocol (v2 → v3) — additive

`crates/chess-net/src/protocol.rs`:

```rust
pub const PROTOCOL_VERSION: u16 = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    Hello { protocol: u16, observer: Side, rules: RuleSet, view: PlayerView },     // unchanged
    Update { view: PlayerView },                                                    // unchanged
    Error { message: String },                                                      // unchanged
    Rooms { rooms: Vec<RoomSummary> },                                              // unchanged
    // NEW: spectator-side counterpart to Hello (no `observer`).
    Spectating { protocol: u16, rules: RuleSet, view: PlayerView },
    // NEW: pushed once right after Hello / Spectating with the room's chat
    // ring buffer (≤50 lines). Empty if the room is fresh.
    ChatHistory { lines: Vec<ChatLine> },
    // NEW: pushed live to every recipient (seats + spectators) on each chat
    // line. `from` is always a player's `Side` — system messages are P2.
    Chat { line: ChatLine },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    Move { mv: Move },             // unchanged
    Resign,                        // unchanged
    Rematch,                       // unchanged
    ListRooms,                     // unchanged
    // NEW: send a chat message. Server enforces players-only, length ≤256
    // chars after trim, drops control chars except `\n` (rendered as space
    // by clients), and stamps `ts_ms` server-side.
    Chat { text: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatLine {
    pub from: Side,
    pub text: String,
    pub ts_ms: u64,                // unix ms, set server-side
}

// RoomSummary gains a spectator count, defaulted for v2 wire compatibility.
pub struct RoomSummary {
    pub id: String,
    pub variant: String,
    pub seats: u8,
    #[serde(default)]
    pub spectators: u16,           // NEW
    pub has_password: bool,
    pub status: RoomStatus,
}
```

**Back-compat**: v2 clients never request spectator mode (no UI for it), so they only ever receive `Hello` / `Update` / `Error` / `Rooms` — all unchanged. v2 lobby clients deserializing v3 `RoomSummary` ignore the new `spectators` field. v2 player clients connected to a v3 server can still play normally — they just don't see chat. `Chat` from a player to a v2 opponent gets sent on the wire and the v2 client's `serde` will fail to deserialize → the existing chess-tui error path treats it as a non-fatal log line. (Acceptable: v2 client isn't expected to participate in chat.)

### Spectator opt-in URL

Two ways a third client can ask for spectator role on `GET /ws/<room>`:

1. Query param: `?role=spectator` (or `?spectate=1`). Used by v3 clients clicking "Watch" in the lobby.
2. Auto-fallback: if a third+ connection arrives **without** `?role=spectator` and the room isn't full of spectators, the server upgrades them to spectator instead of replying "room full". This is the "zero-config" path for v3 clients that don't know the param.

v2 clients arrive without the param → with auto-fallback they'd be upgraded to spectator silently and then the spectator `Spectating` welcome would crash their parser. To keep v2 alive: **only auto-upgrade when the inbound `User-Agent` header reports `chess-tui/3+` or `chess-web/3+`** — otherwise fall through to "room full". Simpler alternative we'll actually adopt: **require `?role=spectator` explicitly**. v2 clients hit "room full" exactly as before; v3 clients put the param on Watch links.

**Spectator cap**: 16 per room, configurable via `--max-spectators` (default 16). 17th spectator gets `Error{"room watch capacity reached"}`. Avoids unbounded fan-out cost and keeps `notify_lobby` cheap.

### Server (`crates/chess-net/src/server.rs`)

```rust
struct RoomState {
    rules: RuleSet,
    state: GameState,
    seats: Vec<(Side, mpsc::UnboundedSender<ServerMsg>)>,         // unchanged
    spectators: Vec<mpsc::UnboundedSender<ServerMsg>>,            // NEW
    chat: VecDeque<ChatLine>,                                     // NEW, cap 50
    password: Option<String>,
    rematch_votes: HashSet<Side>,                                 // (existing field name)
}
```

Changes:

- `handle_room_socket`: after auth, branch on `query.role`:
  - `Some("spectator")` and `spectators.len() < MAX_SPECTATORS` → push channel into `spectators`, send `Spectating { protocol, rules, view }` then `ChatHistory { lines }`. No `Hello`.
  - `None` → existing seat-assignment path. After `Hello`, also send `ChatHistory { lines }`.
  - Otherwise → existing "room full" / "watch capacity reached" error.
- New `process_chat(g, sender_side, text)` — validates length/sanity, pushes into ring buffer (`chat.push_back`; truncate to 50 with `pop_front`), then `broadcast_chat(g, line)` to seats + spectators.
- New `broadcast_to_all(g, msg)` helper that fans `msg` over both `seats.iter().map(|(_, tx)| tx)` and `spectators.iter()`. Use it for `Chat` only — `Update` stays per-seat-projected (each seat needs its own `PlayerView`), but spectators receive the projection from `Side::RED`'s POV (or, simpler, the same `Update` payload cloned — fine because spectators are observers, not participants).

  Actually, since `PlayerView::project` returns a server-leaked-safe view, spectators should see the **observer-from-RED** projection (no hidden information leak). Add `broadcast_update_with_spectators(g)` which fans seat-specific `Update`s and one extra `Update { view: PlayerView::project(&g.state, Side::RED) }` to each spectator.
  
  *Banqi note*: spectator view from Red's POV reveals nothing extra — banqi hidden tiles are hidden in every projection until flipped. Three-kingdom would need a "neutral" projection later (P3, deferred).

- `process_client_msg`: route `ClientMsg::Chat` for spectators to `Error{"spectators cannot chat"}`. For players, call `process_chat`.
- GC (`drop_summary` site): only delete the room when **both** `seats` and `spectators` are empty (and it's not `main`).
- `notify_lobby` / `RoomSummary` build: include `spectators: g.spectators.len() as u16`.
- New CLI flag in `bin/server.rs`: `--max-spectators N` (default 16). Reads `CHESS_NET_MAX_SPECTATORS` env as fallback to mirror existing `--static-dir` pattern.

### chess-web (`clients/chess-web/`)

**Layout** (`pages/play.rs` + `style.css`):

The existing 2-column grid (`minmax(0, 1fr) 280px`) becomes a 2-column grid where the right column stacks `<OnlineSidebar>` on top and `<ChatPanel>` below — **no new column**, no new responsive break needed. Chat panel ~280×280 with internal scroll. On mobile (<720px) the whole right column collapses below the board as today; chat sits below sidebar.

```
.game-page (grid 1fr / 280px)
├── .board-pane
└── .right-column (NEW)
    ├── <OnlineSidebar/>
    └── <ChatPanel/>           ← NEW
```

**New component** `clients/chess-web/src/components/chat_panel.rs`:

```rust
#[component]
pub fn ChatPanel(
    role: Signal<ClientRole>,                  // Player(Side) | Spectator
    log: Signal<Vec<ChatLine>>,                // last-50 ring buffer
    on_send: Callback<String>,                 // disabled when role==Spectator
) -> impl IntoView { ... }
```

Renders a scrollable log (auto-scroll to bottom on new line) + a bottom input row. Spectator: input disabled with placeholder `"Spectators can read but not chat"`. Each line shows `[hh:mm] Red: text` colored by `from` side (use existing `--red`/`--fg` palette).

**State** (`clients/chess-web/src/state.rs`):

Extend the existing `ClientGame::Online` arm with:

```rust
pub enum ClientRole { Player(Side), Spectator }

ClientGame::Online {
    role: ClientRole,                    // was just `observer: Side`
    view: PlayerView,
    rules: RuleSet,
    chat: VecDeque<ChatLine>,            // NEW, capped at 50 client-side too
}
```

`<PlayPage>` reactive effect on incoming `ServerMsg`:
- `Spectating{..}` → set role=Spectator, view=…, leave chat empty.
- `ChatHistory{lines}` → replace `chat`.
- `Chat{line}` → push, truncate to 50.
- All move/resign/rematch UI gated on `matches!(role, ClientRole::Player(_))`.

**Lobby** (`pages/lobby.rs`):

Each room row gains a "Watch" affordance whenever the row is shown:
- Existing "Join" link: unchanged when `seats < 2`, hidden when `seats == 2`.
- New "Watch" link: visible always, points at `/play/<id>?role=spectator&password=<…>`.
- Seat label updates to `"{seats}/2 · {spectators} 👁"` when spectators > 0.

Add a tiny test in `routes.rs` for a `?role=spectator` parser if we surface it as a typed param (otherwise just URL-build inline).

**WS** (`clients/chess-web/src/ws.rs`):

No API change — `ReadSignal<Option<ServerMsg>>` already carries the new variants once protocol.rs adds them. Add a `connect_spectator(url)` convenience or just append `?role=spectator` at the call site in `<PlayPage>`.

**Style** (`style.css`):

```css
.right-column { display: grid; gap: 1rem; align-content: start; }
.chat-panel {
    background: rgba(244, 210, 163, 0.04);
    border: 1px solid var(--grid);
    border-radius: 0.5rem;
    display: grid; grid-template-rows: 1fr auto;
    height: 320px;
}
.chat-log { overflow-y: auto; padding: 0.5rem 0.75rem; font-size: 0.88rem; }
.chat-line { margin: 0.15rem 0; }
.chat-line .from-red { color: var(--red); font-weight: 600; }
.chat-line .from-black { color: var(--fg); font-weight: 600; }
.chat-line .ts { color: var(--muted); font-size: 0.78rem; margin-right: 0.4rem; }
.chat-input { display: flex; gap: 0.4rem; padding: 0.4rem 0.5rem; border-top: 1px solid var(--grid); }
.chat-input input { flex: 1; }
.chat-input input:disabled { opacity: 0.5; cursor: not-allowed; }
.spectator-badge { color: var(--accent); font-size: 0.85rem; }
```

### chess-tui (`clients/chess-tui/`)

**State** (`app.rs`):

Extend `NetView`:
```rust
pub struct NetView {
    pub role: Option<NetRole>,                     // None until welcome
    pub view: Option<PlayerView>,
    pub rules: Option<RuleSet>,
    pub chat: VecDeque<ChatLine>,                  // capped 50
    pub chat_input: Option<String>,                // Some when in chat-input mode
    pub last_msg: Option<String>,
}
pub enum NetRole { Player(Side), Spectator }
```

**Input map** (`input.rs`):
- `t` → enter chat-input mode (only when `role == Some(Player(_))` and game is online; in local mode `t` stays unbound). Sets `chat_input = Some(String::new())`.
- While `chat_input.is_some()`:
  - Printable chars append (cap at 256 chars).
  - `Backspace` deletes last char.
  - `Enter` sends `ClientMsg::Chat{text}` if non-empty after trim, then exits the mode.
  - `Esc` exits without sending.
  - All other game keys (hjkl/Enter-as-commit/u/n/r/?/q) ignored while typing, except Ctrl-C which always quits.

**Rendering** (`ui.rs`):

The 36-col sidebar gains a chat region at the bottom:
```
┌── sidebar ────────────────────┐
│ Variant: xiangqi               │
│ Side: Red                      │
│ Status: Ongoing                │
│ Legal moves: 44                │
│                                │
│ ── chat ──                     │
│ [12:03] Red: nice opening      │
│ [12:04] Black: thanks          │
│ [12:05] Red: g'l                │
│                                │
│ > _                            │  ← only when in input mode
└────────────────────────────────┘
```

Implementation: split the sidebar `Rect` with `Layout::vertical([Length(meta_rows), Min(0), Length(input_rows)])` — meta keeps current content, middle gets the chat log scrolled to bottom, bottom shows the input row when active (or `[t] chat` hint when role==Player and not active, or `(spectator)` when role==Spectator).

**Spectator gating**:
- `app.rs::apply_action` for select/commit/flip checks `role`. If `Spectator`, set `last_msg = "spectators cannot move"` and return.
- Resign/rematch keys (currently no `r`-for-resign? — verify via input.rs) similarly gated.
- Sidebar variant label appends `(spectator)` in `--accent` color.

**Net** (`net.rs`):

`NetEvent` already wraps `ServerMsg`; new variants flow through unchanged. Add no-op match arms — they'll be handled in `apply_net_event` in `app.rs`.

**Lobby** (`app.rs` Screen::Lobby + `bin/lobby_render`):

- New action: `w` on a highlighted room → connect with `?role=spectator`. Remains available when room is full (existing `Enter` to join is disabled at seats==2).
- Room row format: `"{id:20} {variant:12} {seats}/2  {spec}👁  {status}"` — pad spectator column with empty space when 0.

### TUI ↔ Web ↔ chess-net testing notes

`make play-local` becomes a 3-pane tmux: server, two TUI players. Spectator/chat smoke test recipe:

1. `make play-local` — server + Red + Black panes.
2. Spawn a third client manually: `cargo run -p chess-tui -- --connect ws://127.0.0.1:7878/ws/main?role=spectator`. Should render a read-only board and chat history.
3. In Red pane: press `t`, type "hi", Enter. Black + spectator should see the line.
4. In spectator pane: press `t`. Should display "(spectator)" hint and ignore the keypress.
5. Open `http://localhost:8080/lobby` via `make play-web`; the `main` row should show `1/2 · 1 👁`.
6. Click Watch on any row → web `/play/<id>?role=spectator` opens with disabled chat input + read-only board.

A new tmux helper `scripts/play-spectator.sh` (called by `make play-spectator`) starts the existing 2-player session plus a third TUI spectator pane for one-shot demo.

### Test plan

`crates/chess-net/tests/spectator.rs` (new):
- `spectator_join_succeeds_with_role_param` — third connection with `?role=spectator` gets `Spectating` + `ChatHistory`, not `Error`.
- `third_player_without_role_param_still_room_full` — back-compat assertion.
- `spectator_chat_rejected` — `ClientMsg::Chat` from spectator returns `Error{"spectators cannot chat"}`.
- `chat_ring_buffer_caps_at_50` — push 60 lines, assert `ChatHistory` to a late spectator carries exactly 50.
- `spectator_capacity_enforced` — 17th spectator gets `Error{"room watch capacity reached"}`.
- `chat_broadcasts_to_seats_and_spectators` — Red sends, both Black and 1 spectator receive `Chat{line}` with matching `from`/`text`.

`crates/chess-net/tests/protocol_roundtrip.rs` (extend or new):
- v2 `RoomSummary` JSON (without `spectators`) deserializes into v3 struct with `spectators=0`.
- Each new ServerMsg/ClientMsg variant round-trips through `serde_json`.

`clients/chess-tui/src/input.rs` test (extend if a key-map test exists; else new):
- `t` produces `Action::EnterChatInput` only in `Screen::Net`.
- Chat-input mode delegates printable chars, Enter, Backspace, Esc correctly.

`clients/chess-web/src/components/chat_panel.rs` — pure-logic helpers tested where extractable (`format_ts(ts_ms) -> String`, `truncate_ring(&mut VecDeque, max)`); leptos UI itself relies on the manual smoke test.

## Critical files to modify or create

**Protocol**:
- `crates/chess-net/src/protocol.rs` — bump version, add `ChatLine`, `Spectating`/`ChatHistory`/`Chat` ServerMsg, `Chat` ClientMsg, `RoomSummary.spectators`.

**Server**:
- `crates/chess-net/src/server.rs` — `RoomState.{spectators, chat}`, spectator branch in `handle_room_socket`, `process_chat`, `broadcast_to_all`, GC tweak, `notify_lobby` includes spectator count, `MAX_SPECTATORS` constant + flag wiring.
- `crates/chess-net/src/bin/server.rs` — `--max-spectators` CLI flag + env fallback.
- `crates/chess-net/tests/spectator.rs` (new), `crates/chess-net/tests/protocol_roundtrip.rs` (new or extend).

**chess-web**:
- `clients/chess-web/src/state.rs` — `ClientRole` enum, extend `ClientGame::Online`.
- `clients/chess-web/src/pages/play.rs` — role-aware effects, `<ChatPanel>` wiring, OnlineSidebar spectator badge, gate move/resign/rematch on role.
- `clients/chess-web/src/pages/lobby.rs` — Watch link + spectator count column.
- `clients/chess-web/src/components/chat_panel.rs` (new).
- `clients/chess-web/src/ws.rs` — URL helper for `?role=spectator` (optional).
- `clients/chess-web/style.css` — `.right-column`, `.chat-panel`, `.chat-log`, `.chat-input`, `.spectator-badge`.

**chess-tui**:
- `clients/chess-tui/src/app.rs` — `NetView.{role, chat, chat_input}`, dispatch new ServerMsg variants in `apply_net_event`, gate input on role.
- `clients/chess-tui/src/input.rs` — `t` mapping + chat-input mode key handling.
- `clients/chess-tui/src/ui.rs` — sidebar layout split, chat log render, input row, spectator badge.
- `clients/chess-tui/src/net.rs` — verify NetEvent enum exposes new variants (likely just enum-passthrough).

**Tooling**:
- `scripts/play-spectator.sh` (new) + `Makefile` `play-spectator` / `stop-spectator` targets.

**Docs / planning**:
- `CLAUDE.md` — chess-net gotcha block: protocol v3 spectator/chat additions, `?role=spectator` opt-in for back-compat, MAX_SPECTATORS default, chat ring buffer of 50.
- `TODO.md` — promote "chess-net chat + spectators" via `scripts/promote-todo.sh`; new follow-ups: chat moderation, system messages (player joined/left), emoji/unicode tests, spectator chat permission setting per room.
- `backlog/chess-net-chat-moderation.md` (new) — rate limiting, mute, server-side filtering recipe.
- `backlog/chess-net-system-messages.md` (new) — `ChatLine.from` becomes an enum to carry `System("Red joined")` etc.
- `docs/adr/0006-chess-net-spectators-chat.md` (new) — locks in: opt-in `?role=spectator`, players-only chat, 50-msg ring buffer, spectator cap default 16, spectator view from Red POV.

## Existing functions/utilities to reuse

- `chess_net::server::send_close_with` (server.rs error-then-close path) — reuse for spectator capacity error.
- `chess_net::server::notify_lobby` + `refresh_summary` — extend, don't replace.
- `chess_core::view::PlayerView::project(&state, Side::RED)` — spectator board view.
- `chess_core::piece::Side` — already in `ChatLine.from`.
- `clients/chess-tui/src/app.rs::Screen` enum + existing `Screen::Net` render path — chat sits inside it.
- `clients/chess-web/src/components/sidebar.rs` styling tokens (`--red`, `--fg`, `--muted`, `--accent`) — chat lines reuse them, no new palette tokens.
- `leptos_router::A` for the new lobby Watch link.

## Verification

CI gates (must pass before push):

1. `cargo fmt --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` — protocol round-trip, spectator suite, chat ring buffer, input-mode tests.
4. `cargo build --target wasm32-unknown-unknown -p chess-core` — engine WASM still clean.
5. `cargo build --target wasm32-unknown-unknown -p chess-web` — web WASM compiles with new component.

Manual end-to-end smoke (in PR description checklist):

1. `make play-local` — Red + Black; press `t` in Red, type a line, confirm Black sees it. Backspace + Esc behave. `t` while typing inserts `t` (not re-enter mode).
2. From a third terminal, `cargo run -p chess-tui -- --connect ws://127.0.0.1:7878/ws/main?role=spectator` — board renders read-only, chat log shows the prior line, `t` shows "(spectator)" badge instead of input.
3. Spectator presses `hjkl/Enter` on a piece → `last_msg` shows "spectators cannot move", no state change.
4. `curl -s http://127.0.0.1:7878/rooms | jq` — main room shows `"spectators": 1`.
5. `make play-web` then open `http://localhost:8080/lobby` in two tabs — spectator counter on `main` updates live.
6. Click Watch on `main` → spectator page loads with disabled chat input + read-only board.
7. v2 back-compat: `git stash` the protocol changes locally → run an older chess-tui as third joiner → still gets `room full` error (i.e. v2 path unchanged when v3 client doesn't ask for spectator).
8. Capacity: spawn 17 spectators via `for i in {1..17}; do cargo run -p chess-tui -- --connect ws://127.0.0.1:7878/ws/main?role=spectator & done` — 17th gets the "room watch capacity reached" error and exits.
9. Chat history: send 60 chat lines, then connect a fresh spectator — `ChatHistory` carries 50 lines; oldest 10 dropped.

## Phasing — 6 commits, 1 PR

1. **protocol-v3**: protocol.rs bump + new variants/types + round-trip tests + CHANGELOG-style note in module doc.
2. **server-spectator**: spectator branch in `handle_room_socket`, GC tweak, `notify_lobby` spectator count, capacity flag + tests.
3. **server-chat**: ring buffer, `process_chat`, broadcast helper, tests.
4. **chess-web**: `<ChatPanel>` + role-aware play page + lobby Watch links + styles.
5. **chess-tui**: chat input mode (`t`), sidebar split, spectator gating, lobby `w` action.
6. **docs**: ADR-0006, CLAUDE.md gotcha update, TODO.md promote + new backlog entries, `play-spectator.sh` + Makefile target.

## Deferred (P2/P3 follow-ups added to TODO.md in this PR)

- **System messages** (`ChatLine.from` becomes an enum) — `[backlog/chess-net-system-messages.md]`.
- **Chat moderation**: rate limiting (≤1 msg/sec/player), mute button, server-side word filter — `[backlog/chess-net-chat-moderation.md]`.
- **Spectator chat permission**: per-room toggle so a streamer can let viewers chat — small protocol addition.
- **Reconnect-as-spectator**: rejoining a room you previously sat in should auto-restore your seat (token cookie). Currently disconnect frees the seat.
- **Web chat UI polish**: emoji picker, markdown-lite (bold/italic/code), unread-message badge when chat panel scrolled up.
- **Chess-tui chat scrollback**: PgUp/PgDn to scroll the chat log when more than visible rows.
- **Three-kingdom spectator view**: needs a "neutral" `PlayerView::project_neutral` variant — out of scope until three-kingdom engine ships.
