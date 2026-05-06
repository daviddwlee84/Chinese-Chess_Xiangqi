# chess-net: multi-room lobby + optional password

## Context

The chess-net MVP shipped 2026-05-06: one server boot = one hard-coded room = one game; the first two ws clients seat Red/Black, the third gets `Error{"room full"}`. Coordination is "Slack the IP, count seats by hand." This PR turns chess-net into a real multi-room server with an in-TUI lobby browser, plus an optional per-room password for the friend-list-lock case (not security).

The MVP retrospective surfaced three integration paths the user explicitly asked about: (A) extend `chess-tui` with a `Screen::Lobby`, (B) ship a separate `chess-tui-lobby` binary or subcommand, (C) replace/parallel the transport with `russh` for `ssh chess.you.dev`. Path A wins — see Recommendation. The SSH path is already filed as the separate `[?/L] Wish/SSH multiplayer alternative to chess-net` P? spike in `TODO.md` and is **not** absorbed here.

The hard structural constraints from the existing stack:
- `clients/chess-tui/src/net.rs` is sync `tungstenite` + `std::sync::mpsc` — no tokio in the TUI binary. The lobby connection must reuse this exact shape.
- `clients/chess-tui/src/main.rs::normalize_ws_url` (line 140) already preserves any URL containing `/ws`, so `ws://host/ws/<id>` works today with zero changes.
- `crates/chess-net/src/server.rs::RoomState` (line 26) is held in `Arc<Mutex<RoomState>>` and broadcasts via per-socket `mpsc::UnboundedSender<ServerMsg>` write tasks — that pattern survives untouched into multi-room.
- `NetEvent::Server(Box<ServerMsg>)` already boxes the payload (net.rs:28); growing `ServerMsg` doesn't trigger a `clippy::large_enum_variant` regression.

## Recommendation: Option A (in-process `Screen::Lobby`)

Extend chess-tui in-process with a new `Screen::Lobby` that subscribes to a server-side **lobby ws channel** (`ws://host/lobby`), which pushes a live `Rooms` snapshot whenever any room's state changes. Joining a room spawns a second `NetClient` against `ws://host/ws/<id>?password=…` — the existing connect path. This keeps users inside one terminal and gives live "room appeared / filled / finished" updates with no polling.

**Rejected — B (separate `chess-tui-lobby` binary)**: forces a two-process workflow and re-introduces the room-URL-copy-paste problem this PR exists to fix. Also doubles the deploy surface for one feature.

**Rejected — C (russh)**: ~1500 LOC of `russh` glue + PTY/SIGWINCH plumbing + SSH key management, locks the canonical transport to SSH, and conflicts with the chess-web roadmap (P2). Worth a spike later, not in this PR.

**Why ws push, not REST `GET /rooms` polling**: the existing `NetClient` is wire-shaped for streaming; a REST endpoint would force adding an HTTP client (`reqwest`) or hand-rolling HTTP over `TcpStream`. A `/rooms` JSON endpoint is added anyway as a small `axum::Json` handler for `curl` debugging, but the TUI uses the ws.

## Scope

**In:**
- Multi-room server: `HashMap<RoomId, Arc<Mutex<RoomState>>>`, room auto-created on first connect to `/ws/<id>`, GC'd when last seat leaves (except room `main`, which is permanent for backwards compat).
- Lobby ws channel `/lobby` with subscriber set; live `Rooms` push on every state change.
- Per-room optional password set by the first joiner, stored plain-text (friend-lock not security; documented).
- chess-tui `Screen::Lobby`, `Screen::HostPrompt`, `Screen::CreateRoom` (plus growing `Screen::Picker` by one entry).
- chess-tui CLI: new `--lobby <ws-url>` flag (skip picker → land in lobby), new `--password <pw>` flag (paired with `--connect ws://host/ws/<id>`).
- `/rooms` JSON snapshot endpoint for `curl`/debugging.
- Backwards compat: `ws://host` and `ws://host/ws` (no room id) land in room `main`. v1 clients keep working unchanged.
- `make play-lobby` Makefile target + `scripts/play-lobby.sh` (server + 3 panes: 2 lobby-flow joiners + 1 lobby watcher).

**Out (each gets a TODO.md entry, P2 unless noted):**
- Mixed-variant servers (one server, different variants per room).
- Spectator slots (3+ connections per room).
- Reconnect / resume on transient disconnect (already a TODO).
- Lobby filter (`/`) and sort.
- Lobby-push debouncing (≥250ms coalesce) — premature today.
- Server-side admin / kick / signed token.
- WSS / TLS / oauth (loopback-only posture preserved).
- Persistent room storage (rooms are in-memory; restart = wipe).

## Wire schema (`crates/chess-net/src/protocol.rs`)

`PROTOCOL_VERSION` bumps to **2**. Old messages serialize unchanged; we only add new variants.

```rust
pub const PROTOCOL_VERSION: u16 = 2;

#[serde(tag = "type")]
pub enum ServerMsg {
    Hello { protocol: u16, observer: Side, rules: RuleSet, view: PlayerView },
    Update { view: PlayerView },
    Error { message: String },
    // NEW: pushed on the lobby socket only — initial snapshot on connect,
    // then again whenever any room's state changes.
    Rooms { rooms: Vec<RoomSummary> },
}

#[serde(tag = "type")]
pub enum ClientMsg {
    Move { mv: Move },
    Resign,
    Rematch,
    // NEW: lobby-only. Forces a fresh Rooms push to the requester.
    ListRooms,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoomSummary {
    pub id: String,
    pub variant: String,        // "xiangqi" | "xiangqi-strict" | "banqi"
    pub seats: u8,              // 0|1|2
    pub has_password: bool,
    pub status: RoomStatus,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomStatus { Lobby, Playing, Finished }
```

Backwards compat:
- **New server + v1 client**: v1 client connects to `/ws` → default room `main` → Hello with `protocol: 2` (v1 ignores the field). Works.
- **v1 server + new client using `--lobby`**: v1 server returns 404 on `/lobby` upgrade → tungstenite errors → `NetEvent::Disconnected("connect failed: ...")`. Surface as `"server doesn't support lobby; try --connect ws://host/ws"` in the TUI.

## Server architecture (`crates/chess-net/src/server.rs`)

```rust
type RoomMap = Arc<Mutex<HashMap<String, Arc<Mutex<RoomState>>>>>;

struct AppState {
    rooms: RoomMap,
    /// Outbound senders for every connected lobby socket.
    /// Cleaned by lobby write tasks draining their mpsc and exiting.
    lobby_subs: Arc<Mutex<Vec<mpsc::UnboundedSender<ServerMsg>>>>,
    /// Server-wide variant for MVP (matches `chess-net-server <variant>`).
    /// Every auto-created room copies this.
    default_rules: RuleSet,
}

struct RoomState {
    state: GameState,
    seats: Vec<(Side, mpsc::UnboundedSender<ServerMsg>)>,
    rematch: Vec<Side>,
    password: Option<String>,   // NEW: set by first joiner, immutable thereafter
}
```

**Locking discipline (load-bearing)**: outer `RoomMap` lock is held only for HashMap insert / remove / iterate-to-snapshot — never across `await`, never across a per-room `tx.send`. Inner `Mutex<RoomState>` per-room is acquired separately. Lock order: outer → inner, never reverse. Avoids head-of-line block: a slow room's inner lock can't freeze new joins or lobby pushes. (`DashMap` would also work but doubles the dep tree for marginal gain.)

**Routes (axum)**:
```rust
Router::new()
    .route("/",            get(upgrade_default))   // back-compat → "main"
    .route("/ws",          get(upgrade_default))   //   "
    .route("/ws/:room_id", get(upgrade_room))      // join named room
    .route("/lobby",       get(upgrade_lobby))     // subscribe to room list
    .route("/rooms",       get(rooms_snapshot_json)) // curl/debug
    .with_state(app_state)
```

**Validation** (in `upgrade_room`, before `ws.on_upgrade`):
- Room id: regex `^[a-zA-Z0-9_-]{1,32}$`. Rejects path traversal, whitespace, NUL. Invalid → HTTP 400.
- `?password=` query: max 64 chars, any printable. Stored as-is.
- Wrong password: upgrade succeeds, server immediately sends `Error{"bad password"}` and closes. Visible in client as a normal `ServerMsg::Error` (not `tungstenite::Error::Http`).

**Lobby push triggers** (`notify_lobby(state: &AppState)` — snapshot under outer lock, drop, then iterate `lobby_subs` outside any lock):
1. Seat insertion (room appearing or filling).
2. Seat removal (room emptying / GC'd).
3. `process_client_msg` move that flips `GameStatus::Ongoing` → `Won`/`Drawn`.
4. Explicit `ClientMsg::ListRooms` (sends only to the requester).

Triggers 1–3 are infrequent per game; no debounce needed at MVP scale (filed as P2 if churn observed).

**Empty-room GC**: after `g.seats.retain(|(s, _)| *s != seat)` in the disconnect path, if `g.seats.is_empty()` and `room_id != "main"`, drop the inner lock, re-acquire the outer `RoomMap`, and remove the entry. Stops memory leaks from spectator-style room churn.

## Client architecture

**`clients/chess-tui/src/net.rs`** — unchanged. The lobby spawns a second `NetClient::spawn(format!("ws://{host}/lobby"))`. Each `NetClient` is its own OS thread + mpsc pair; they don't interact.

**`clients/chess-tui/src/app.rs`** — three new `Screen` variants:

```rust
pub enum Screen {
    Picker(PickerView),       // existing — gets one new entry
    Game(Box<GameView>),
    Net(Box<NetView>),
    HostPrompt(HostPromptView),  // NEW: "Server URL: ws://___"
    Lobby(Box<LobbyView>),       // NEW: room browser
    CreateRoom(CreateRoomView),  // NEW: room id + optional password
}

pub struct HostPromptView { pub buf: String, pub error: Option<String> }

pub struct LobbyView {
    pub client: NetClient,
    pub host: String,                  // "ws://host:port"
    pub rooms: Vec<RoomSummary>,
    pub cursor: usize,
    pub last_msg: Option<String>,
    pub connected: bool,
    pub password_buf: Option<String>,  // mid-prompt buffer when joining locked room
    pub style: Style,                  // forwarded to Screen::Net on join
    pub use_color: bool,
}

pub struct CreateRoomView {
    pub host: String,
    pub id_buf: String,
    pub password_buf: String,
    pub focus: CreateRoomField,        // Id | Password | Submit
    pub error: Option<String>,
    pub style: Style,
    pub use_color: bool,
}
```

`PickerEntry::ALL` gets a new variant `ConnectToServer` inserted before `Quit`. `dispatch_picker` adds one match arm: transition to `Screen::HostPrompt`.

**Esc semantics** (real change vs MVP, which had no re-entry): Esc from Lobby → HostPrompt; Esc from HostPrompt → Picker; Esc from CreateRoom → Lobby. Implement by storing previous-screen identity inline in each new view struct only where needed — don't add a generic back-stack.

**Keybinds (`clients/chess-tui/src/input.rs`)**:
- Lobby: `j/k` or `↓/↑` move cursor (reuse `PickerUp`/`PickerDown`); Enter/Space joins; `c` = `LobbyCreate`; `r` = `LobbyRefresh` (sends `ClientMsg::ListRooms`); Esc = `Back`; `/` reserved for filter (parsed but `Action::None` for now).
- HostPrompt / CreateRoom: text input. **Don't add `tui-input` crate** — write a 30-line helper for printable chars + backspace + `Tab`/`Shift-Tab` for field focus.
- Collision: `r` is `RulesToggle` globally today. Resolve by gating: in `Screen::Lobby`, `r` is `LobbyRefresh`; everywhere else, unchanged. Add `in_lobby: bool` next to the existing `in_picker: bool` plumbed into `from_key`.
- New `Action` variants: `Back`, `LobbyCreate`, `LobbyRefresh`, `TextInput(char)`, `TextBackspace`, `FocusNext`, `FocusPrev`.

**`clients/chess-tui/src/main.rs`** CLI additions:
```rust
#[arg(long)] connect: Option<String>,   // existing
#[arg(long)] lobby: Option<String>,     // NEW
#[arg(long)] password: Option<String>,  // NEW: pairs with --connect
```

Branch in `main()` (in this priority order):
1. `--lobby <url>` set → `AppState::new_lobby(url)`.
2. `--connect <url>` set → `AppState::new_net(url, password)` (forwards `?password=` if set).
3. Variant subcommand → existing local game.
4. Bare → existing picker.

`normalize_ws_url` already covers `/ws/<id>` (it preserves anything containing `/ws`). Add `normalize_lobby_url` that auto-appends `/lobby` if absent.

## UX flow

```
chess-tui                                  → Picker
  └ Connect to server…                     → HostPrompt
      └ enter "ws://host:7878"             → Lobby (ws://host/lobby)
          ├ ↑/↓ on a row, Enter            → Net (ws://host/ws/<id>?pw=…)
          │   └ if has_password: prompt    →   "
          ├ c                              → CreateRoom
          │   └ submit                     → Net (server auto-creates)
          ├ r                              → manual ListRooms
          └ Esc                            → HostPrompt
chess-tui --connect ws://host/ws/foo --password bar  → Net directly
chess-tui --connect ws://host                        → Net default room (unchanged MVP)
chess-tui --lobby ws://host                          → Lobby directly
```

Pre-Hello rendering inside `Screen::Net` already covered by `draw_connecting_placeholder` (`ui.rs:131`); for `Screen::Lobby`, mirror that with `draw_lobby_connecting` until first `Rooms` push lands.

## Critical files

### New
- `crates/chess-net/tests/lobby_smoke.rs` (or extend existing `server_smoke.rs`).
- `clients/chess-tui/src/text_input.rs` — 30-line ASCII text-input helper (printable chars, backspace, Tab focus).
- `docs/adr/0005-multi-room-lobby.md` — ADR continuing the 0001–0004 sequence.
- `scripts/play-lobby.sh` + `make play-lobby` target.

### Modified
- `crates/chess-net/src/protocol.rs` — `PROTOCOL_VERSION` → 2; add `ServerMsg::Rooms`, `ClientMsg::ListRooms`, `RoomSummary`, `RoomStatus`.
- `crates/chess-net/src/server.rs` — `AppState`, `RoomMap`, `notify_lobby`, route handlers `upgrade_room`/`upgrade_lobby`/`rooms_snapshot_json`, room id validation regex, GC on last-seat-leave, password field on `RoomState`.
- `crates/chess-net/src/lib.rs` — re-export new protocol types.
- `crates/chess-net/src/bin/server.rs` — no functional change; help text mentions multi-room.
- `clients/chess-tui/src/app.rs` — three new `Screen` variants, `dispatch_lobby`/`dispatch_host_prompt`/`dispatch_create_room`, `tick_net` extended to also drain lobby `NetClient` events, `apply_lobby_event`.
- `clients/chess-tui/src/input.rs` — new `Action` variants, `in_lobby` gate, text-input action wiring.
- `clients/chess-tui/src/ui.rs` — `draw_host_prompt`, `draw_lobby`, `draw_create_room`, `HELP_LINES_LOBBY`.
- `clients/chess-tui/src/main.rs` — `--lobby`, `--password`, `normalize_lobby_url`, branch order in `main()`.
- `Makefile` — `play-lobby` target.
- `CLAUDE.md` — Common commands: `--lobby`, `--password`, `make play-lobby`. Architecture quick-reference: chess-net is now multi-room. Gotchas: lobby push semantics, password is plain-text friend-lock not security, default room `main` is permanent.
- `TODO.md` — promote `chess-net: multi-room lobby + optional password` to Done; add five P2 follow-ups (mixed-variant, spectators, lobby filter, debouncing, admin/kick).

## Tests

`crates/chess-net/tests/server_smoke.rs` (extend; current has 4 tests):
- `lobby_lists_empty_initially` — connect to `/lobby`, expect `Rooms{rooms: []}` snapshot.
- `lobby_sees_room_after_join` — `/lobby` socket open; client joins `/ws/foo`; `/lobby` reads a second `Rooms` push containing `foo`.
- `lobby_sees_seat_fill_and_finish` — 2 clients join `foo`, lobby sees `seats: 2, status: Playing`; one resigns, lobby sees `Finished`.
- `password_rejected_on_wrong` — first joiner sets `?password=alpha`; second joiner uses `?password=beta`; gets `Error{"bad password"}`, no Hello, socket closed.
- `password_accepted_on_right` — second joiner with correct password gets seated as Black + Hello.
- `two_rooms_isolated_no_crosstalk` — `/ws/foo` clients play a move; `/ws/bar` clients receive no Update.
- `room_gc_after_last_seat_leaves` — lone joiner to `/ws/temp` disconnects; `/lobby` push omits `temp`.
- `default_room_main_persists_after_empty` — assert `main` is **not** GC'd.
- `default_room_back_compat` — connect to `/ws` (no id) → seated in `main`.

`crates/chess-net/tests/protocol_roundtrip.rs` (extend):
- `server_rooms_roundtrips` — `ServerMsg::Rooms{...}` JSON encode/decode, including a `RoomSummary` with and without `has_password`.
- `client_list_rooms_roundtrips` — `ClientMsg::ListRooms`.

chess-tui app-state tests are deferred (no harness exists today; manual `make play-lobby` covers UI flow).

## Verification

Pre-push gates (unchanged):
```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --target wasm32-unknown-unknown -p chess-core
```

Manual smoke (`make play-lobby`):
1. Server boots on `:7878`. tmux window 0 has 3 panes: pane A goes through Picker → HostPrompt → Lobby; pane B same; pane C lands in lobby via `--lobby`.
2. Pane A presses `c`, types `demo`, leaves password empty, submits → enters `demo` as Red.
3. Pane B in Lobby sees `demo (xiangqi, seats: 1, lobby)` row appear within ~50ms (server push, no polling). Picks it with Enter → joined as Black.
4. Pane C (still in Lobby) sees row update to `seats: 2, playing`.
5. Pane A makes a move; Pane B board updates; Pane C lobby unchanged (move doesn't flip status).
6. Pane A resigns; Pane C lobby row flips to `finished`.
7. Both A and B exit via `q`; Pane C lobby row disappears (room GC'd).

Backwards-compat smoke:
- Build new server, run `chess-net-server xiangqi`.
- Run **previous** `chess-tui` binary (rebuild after `git stash` of the client changes) with `--connect ws://127.0.0.1:7878` → seated in `main`, plays normally. Verifies no v1-client regression.

Password smoke:
- Pane A creates room `locked` with password `secret`. Pane B picks `locked` from lobby → prompted for password. Wrong password → `Error{"bad password"}` shown in `last_msg`, stays in lobby. Right password → seated.

## Out-of-scope follow-ups (TODO.md additions)

Run `scripts/add-todo.sh` for each in the implementing commit. The current P2 entry **chess-net: multi-room lobby + optional password** is promoted to Done by `scripts/promote-todo.sh --title "multi-room lobby" --summary "Multi-room ws server (/ws/<id>), optional ?password= lock, in-TUI Screen::Lobby with live ws push, /rooms JSON snapshot, default room 'main' preserved for v1 client back-compat."`

New P2 entries:
- `[M] chess-net: mixed-variant rooms` — per-room `RuleSet` instead of server-wide. Architecture supports it; blocked on UX for "what variant filter applies in lobby".
- `[M] chess-net: spectator slots` — third+ connection joins read-only; gets `Update`, `Move` errors back. Replaces today's "room full" dead-end.
- `[S] chess-net: lobby filter + sort` — `/` filter inside lobby; sort by recency / seat count / variant. Defer until ≥10 concurrent rooms is a real workflow.
- `[S] chess-net: lobby push debouncing` — ≥250ms coalesce on rapid status flips. Premature today; revisit if churn observable.
- `[S] chess-net: server-side admin / kick / signed token` — close stuck rooms or force-disconnect a misbehaving client. Wants admin-only ws path or signed token.

The pre-existing `[?/L] Wish/SSH multiplayer alternative to chess-net` P? entry is **not** touched — remains a separate spike for after chess-web ships.
