# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project at a glance

Rust + WASM Chinese chess engine supporting standard xiangqi (象棋), banqi (暗棋), and three-kingdoms banqi (三國暗棋). The foundational `chess-core` crate is shipped end-to-end, `chess-tui` is wired up for local play (xiangqi + banqi, vim cursor + mouse, CJK or ASCII glyphs), `chess-net` ships a multi-room websocket server (`chess-net-server`) with an in-TUI lobby browser, optional per-room password, and live `Rooms` push to lobby viewers, and `chess-web` (Leptos + Trunk + SVG-only rendering) is a single-page browser client that handles both local pass-and-play (pure WASM, no server) and online play (WS to chess-net). AI (`chess-ai`) is still a stub tracked in [`TODO.md`](TODO.md).

For the tech-selection rationale see [`docs/architecture.md`](docs/architecture.md); for the chess-web Rust+web stack (Leptos + Trunk + WASM) see [`docs/trunk-leptos-wasm.md`](docs/trunk-leptos-wasm.md); for locked-in design decisions see [`docs/adr/`](docs/adr/).

## Common commands

```bash
# Workspace sanity (all 8 crates compile)
cargo check --workspace

# Format + lint (CI requires both clean; clippy uses -D warnings)
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# Engine tests (~71 fast + 1 slow ignored). Default if you change rules.
cargo test -p chess-core

# Run a single test
cargo test -p chess-core --lib coord::tests::direction_deltas_consistent

# Slow xiangqi perft depth-3 — runs in release for speed
cargo test --release -p chess-core --test xiangqi_perft -- --ignored

# WASM cleanliness — proves chess-core has no platform deps
cargo build --target wasm32-unknown-unknown -p chess-core

# Run the REPL test harness (proves the engine end-to-end)
cargo run -p chess-cli
> xiangqi
> moves
> play h2e2
> banqi --preset taiwan --seed 42
> view 0
> quit

# Interactive TUI (default render: CJK glyphs + color)
cargo run -p chess-tui                                    # variant picker
cargo run -p chess-tui -- xiangqi                         # casual mode (default)
cargo run -p chess-tui -- xiangqi --strict                # standard self-check rules
cargo run -p chess-tui -- banqi --preset taiwan --seed 42
cargo run -p chess-tui -- --style ascii xiangqi           # letter glyphs
cargo run -p chess-tui -- --no-color xiangqi              # monochrome
cargo run -p chess-tui -- --as black xiangqi              # render as Black

# Networked play (multi-room — see ADR-0005; spectator + chat — see ADR-0006)
cargo run -p chess-net -- --port 7878 xiangqi             # server, all rooms = xiangqi
cargo run -p chess-net -- --port 7878 xiangqi --strict    # strict self-check
cargo run -p chess-net -- --port 7878 banqi --preset taiwan --seed 42
cargo run -p chess-net -- --port 7878 --max-spectators 32 xiangqi  # raise spectator cap (default 16)

# Client — entry points to the same server:
cargo run -p chess-tui -- --lobby   ws://127.0.0.1:7878            # browse rooms (Enter join / w watch / c create)
cargo run -p chess-tui -- --connect ws://127.0.0.1:7878             # default room "main" (back-compat)
cargo run -p chess-tui -- --connect ws://127.0.0.1:7878/ws/myroom   # named room
cargo run -p chess-tui -- --connect ws://127.0.0.1:7878/ws/locked --password secret  # locked room
cargo run -p chess-tui -- --connect 'ws://127.0.0.1:7878/ws/myroom?role=spectator'   # watch read-only

curl -s http://127.0.0.1:7878/rooms | jq                   # JSON room snapshot (incl. spectator counts)

# tmux harnesses
make play-local                                           # 1 server + 2 --connect clients (default room)
make play-lobby                                           # 1 server + 3 panes (2 lobby flow + 1 watcher)
make play-web                                             # 1 server + Trunk dev-server (SPA on :8080)
make play-spectator                                       # 1 server + 2 players + 1 spectator (chat / role demo)
make play-local VARIANT=banqi                             # banqi
make play-local PORT=9000 VARIANT=xiangqi                 # custom port
make stop-local / make stop-lobby / make stop-web / make stop-spectator   # tear down each session

# Web client (Leptos + Trunk + SVG)
cargo install trunk                                       # one-time
rustup target add wasm32-unknown-unknown                  # one-time
make play-web                                             # full dev loop (server + Trunk hot-reload)
make build-web                                            # trunk build --release → clients/chess-web/dist
cargo run -p chess-net -- --port 7878 \
    --static-dir clients/chess-web/dist xiangqi           # single-binary prod (chess-net serves dist/)
```

TUI input map: `hjkl` / arrows move cursor, `Enter` / `Space` select-or-commit,
`Esc` cancel, `u` undo, `f` flip (banqi), `n` new game (back to picker),
`r` toggle rules overlay, `?` toggle keymap help, `q` / `Ctrl-C` quit. `:`
opens an instant coord-input prompt (type ICCS like `h2e2` / `a3xb3xc3` /
`flip a0`, Enter commits, Esc cancels); `m` opens the same with live preview
— each keystroke updates the selected-square highlight, and a complete move
also previews the destination via the cursor. In Net mode, `t` opens chat
input (players only — Enter sends, Esc cancels); in the lobby, `w` watches
the highlighted room as a spectator. Mouse left-click selects or commits.
When the game ends, a banner appears in the sidebar and move attempts are
gated; `n` returns to the picker, `u` takes back the losing move.

`rustup target add wasm32-unknown-unknown` once per machine. If your rustup mirror lacks the target (e.g. tuna), prefix the command with `RUSTUP_DIST_SERVER=https://static.rust-lang.org` — see [`pitfalls/wasm-getrandom-unresolved-imp.md`](pitfalls/wasm-getrandom-unresolved-imp.md) for the related `js`-feature gotcha.

## Architecture quick reference

The engine lives entirely in `crates/chess-core`. `chess-tui` consumes it for local play and (via `--connect`) talks to `chess-net`'s axum-ws server, which holds the authoritative `GameState` and broadcasts per-side `PlayerView` after each move. `chess-engine`, `chess-ai`, `chess-web`, and `xtask` are still stubs. Five non-obvious decisions are locked in — full rationale in `docs/adr/`:

1. **`Square(u16)` linear index** (ADR-0002), not `(file, rank)` tuples. `Board` knows its `BoardShape` and converts. Scales to 19×19 + irregular topology via per-shape mask.
2. **`Move` is a flat enum** (ADR-0004). `Move::Reveal { at, revealed: Option<Piece> }` is the network ABI boundary: clients send `revealed: None`, the authoritative engine fills in `Some(piece)` post-flip. All variants serde clean.
3. **`RuleSet` is plain data + `bitflags`, not a trait** (ADR-0003). Move-gen is free functions in `rules/{xiangqi,banqi,three_kingdom}.rs` dispatching on `Variant` and consulting `HouseRules` flags. Trait-object rule layering was rejected — kills inlining, fights serde.
4. **`GameState` is one concrete struct.** `TurnOrder` holds `SmallVec<[Side; 3]>` so 3-player isn't a special case. `Side(u8)` (not a fixed enum) carries this.
5. **`PlayerView::project(&GameState, observer)` is the only externally-visible state.** Hidden pieces become `VisibleCell::Hidden` with no identity. `tests/view_projection.rs` proptest enforces no-leak in serialized JSON. The future network layer must ship `PlayerView`, never `GameState`.

Move generation pipeline (xiangqi): `pseudo_legal_moves` (geometry only) → clone-the-state-and-probe legality filter → emit. Cheap enough for 9×10; a future AI hot path should switch to make/unmake without cloning.

## Gotchas worth knowing

- **`make_move` does NOT auto-refresh `status`.** The xiangqi legality filter calls `make_move` on a clone to test self-check; auto-refresh would recurse via `legal_moves`. Callers (CLI / TUI / future server) invoke `state.refresh_status()` after each move when they want to know if the game ended. `refresh_status` covers no-progress draws + no-legal-moves; threefold repetition is a TODO.

- **Three deferred house rules accept the flag but no-op**: `DARK_CHAIN`, `HORSE_DIAGONAL`, `CANNON_FAST_MOVE`. Only `CHAIN_CAPTURE` and `CHARIOT_RUSH` are wired end-to-end. Don't assume code that consumes `HouseRules` handles every flag — grep `rules/banqi.rs::generate` to confirm. The deferred ones are P1 in `TODO.md`.

- **`Variant::ThreeKingdomBanqi` exists but produces an empty 4×8 board.** The types are ready (3-seat `TurnOrder`, `Side(2)`, `BoardShape::ThreeKingdom`), but the actual mask + capture rules ship in PR 2. The setup builder is `setup.rs::build_three_kingdom_stub`. See `backlog/three-kingdoms-banqi.md` for what the implementation needs to settle.

- **WASM build needs `getrandom = { features = ["js"] }`** for `wasm32-unknown-unknown` browser builds. `chess-core/Cargo.toml` adds this as a target-specific dep. Symptom and root cause documented in `pitfalls/wasm-getrandom-unresolved-imp.md`.

- **Move list is `SmallVec<[Move; 32]>`.** Positions exceeding 32 legal moves spill to heap once — fine for correctness but watch when doing AI work.

- **Banqi shuffle determinism.** `RuleSet::banqi_with_seed(house, seed)` uses `ChaCha8Rng` (deterministic). `RuleSet::banqi(house)` falls back to `rand::thread_rng()` for the seed — fine on native; in browser WASM works because of the `js`-feature dep above.

- **Perft is the canary** for move-gen regressions. `tests/xiangqi_perft.rs` locks depth 1 = 44, depth 2 = 1920, depth 3 = 79666 (matches published values). If any of those change, audit the rule edit before assuming the test is wrong.

- **Test fixtures use `.pos` DSL.** Hand-written positions live in `tests/fixtures/<variant>/*.pos` and load via `GameState::from_pos_text(&str)`. `tests/end_conditions.rs` shows the pattern. Format spec: [`docs/snapshot-format.md`](docs/snapshot-format.md). Don't put new test positions inline as Rust code if a fixture file would do — fixtures are editable, diff-friendly, and double as endgame-puzzle source files.

- **Replay = `(initial, moves[])` not `Vec<MoveRecord>`.** `Replay::from_game(state, meta)` walks `state.history` back to the start via `unmake_move` and records the moves. `Replay::play_to(step)` is the single primitive behind animation playback, multi-ply takeback, fork-from-midpoint, and endgame puzzle "start at this position" — don't reinvent any of those.

- **chess-tui orientation lives in `clients/chess-tui/src/orient.rs`, not chess-core.** The engine stays presentation-free; the renderer transposes banqi (4×8 model → 8×4 display) and flips xiangqi (rank 0 at the bottom for Red observer, top for Black) entirely client-side. When `chess-net` lands, the same `project_cell` / `square_at_display` pair handles per-player perspective without any engine change.

- **chess-tui board uses an intersection layout, not boxed cells.** Pieces sit on grid crossings (rendered as `┼` for empty intersections, `╳` at palace centers, or the piece glyph). Rank rows are interleaved with between-rows containing `│` verticals plus `╲ ╱` palace diagonals. The river replaces the between-row at index 4 with a stylised text band — no vertical lines pass through it. Each terminal "cell" spans 4 cols × 2 rows; mouse hit-test in `app.rs::hit_test` divides by these constants. ASCII fallback (`--style ascii`) maps the same layout onto `+ - | \ / X` chars.

- **Casual xiangqi (`RuleSet::xiangqi_casual()` / `xiangqi_allow_self_check: true`)** disables the standard self-check legality filter. Moves that leave your general capturable are accepted; the game ends with `WinReason::GeneralCaptured` when the general is physically taken. `refresh_status` detects the missing general unconditionally — keep the existing checkmate-by-zero-legal-moves path intact (it's still reachable in standard mode). When adding a new RuleSet field, mark it `#[serde(default)]` so older snapshots still deserialize. The TUI defaults to casual; the engine `RuleSet::xiangqi()` factory is still strict (so existing engine tests / snapshots stay correct) — only the chess-tui picker / `Cmd::Xiangqi` selection picks `xiangqi_casual()` by default.

- **`chess-net` is multi-room (ADR-0005) with v3 spectator + chat (ADR-0006).** `crates/chess-net/src/protocol.rs` defines `ServerMsg`/`ClientMsg` (JSON over text frames, `#[serde(tag = "type")]`). Routes: `GET /` and `GET /ws` upgrade into the default room `main` (backwards compat with v1 clients); `GET /ws/<room-id>` upgrades into a named room (auto-created on first arrival, GC'd when the last seat **and** spectator leaves — except `main`, which is permanent); `GET /lobby` is a non-seated subscription that receives `Rooms` pushes on every state change; `GET /rooms` returns the same snapshot as JSON for `curl`/debugging. Optional `?password=<secret>` locks the room (applies to seats and spectators); wrong password → `ServerMsg::Error{"bad password"}` before any welcome. **Password is plain-text friend-lock, not security** — there's no WSS or auth in the loop. Server holds `Arc<Mutex<HashMap<String, Arc<Mutex<RoomState>>>>>` + parallel `summaries` cache so `notify_lobby` builds `Rooms` snapshots without holding inner locks. Within a room, first seat = Red, second = Black; third+ joiner without `?role=spectator` still gets `room full` (v2 back-compat). `chess-tui` joins via `--connect` (direct URL) or `--lobby <host>` (room browser); the lobby spawns a second sync `tungstenite` worker — no tokio in the TUI binary. `Move::Reveal` stays `revealed: None` on the wire end-to-end (the server fills `Some(...)` only inside its local state). Reconnect / time controls / takeback / mixed-variant rooms / TLS are deferred (see `TODO.md`).

- **chess-net v3 spectator opt-in is `?role=spectator` (explicit only).** Joining a room with the param routes to the spectator branch in `server.rs::handle_room_socket`: server sends `ServerMsg::Spectating { protocol, rules, view }` (`PlayerView::project(&state, Side::RED)`), then `ChatHistory { lines }` from the room's ring buffer. Spectators receive `Update` from RED's perspective on every move — banqi hidden tiles stay opaque per ADR-0004. Move / Resign / Rematch / Chat from a spectator returns the appropriate `Error{"spectators cannot ..."}`. **Cap defaults to 16 per room**; override with `--max-spectators N` / `CHESS_NET_MAX_SPECTATORS` env. The (cap+1)th spectator gets `Error{"room watch capacity reached"}`. Spectators of a `?password=`-locked room must present the same password as players (validated before role branching). `RoomSummary.spectators: u16` is `#[serde(default)]` for v2 lobby back-compat. Auto-fallback (silently upgrading a third joiner to spectator) was rejected in ADR-0006 because v2 chess-tui clients would crash on the `Spectating` JSON tag — explicit opt-in keeps the v2 path byte-identical.

- **chess-net chat is players-only with a 50-line per-room ring buffer.** `ClientMsg::Chat { text }` from a seat: trimmed, control chars (`\n` / `\t`) collapsed to space, capped at 256 chars, server-stamps `ts_ms`, pushes to `RoomState.chat: VecDeque<ChatLine>` (oldest dropped at cap), then `broadcast_to_all` fans `ServerMsg::Chat { line }` to seats + spectators with the same payload (chat is a single broadcast, unlike `Update` which projects per-seat). Same `text` from a spectator → `Error{"spectators cannot chat"}`. Late joiners get the buffer in their welcome `ChatHistory { lines }`. **No rate limit, no moderation, no encryption** — this is a friend-only channel; production-grade primitives are sketched at `backlog/chess-net-chat-moderation.md`. System messages (player joined / left) are `ChatLine.from`'s next axis — see `backlog/chess-net-system-messages.md` (will bump to v4 when shipped).

- **chess-tui chat-input mode hijacks the keymap.** Pressing `t` in Net mode sets `NetView.chat_input = Some(String::new())`; `app.input_mode()` then returns `InputMode::Text` so the dispatcher treats printable keys as edits. `Enter` sends `ClientMsg::Chat`; `Esc` (Action::Back) cancels. The chat region in `draw_sidebar_net` reserves the bottom rows of the sidebar (`MIN_CHAT_ROWS = 6`); the log auto-scrolls by always rendering the last `log_h` lines from the `VecDeque`. Spectators see "(spectator — read-only)" instead of the input row and pressing `t` shows a hint. Lobby key `w` triggers `Action::LobbyWatch` → connects with `?role=spectator` (locked rooms still prompt for the password via the existing `PendingJoin` flow, now extended with an `as_spectator: bool` field).

- **chess-tui coord-input mode also hijacks the keymap, in both Game and Net.** Pressing `:` opens an *instant* prompt (no live preview); `m` opens a *live preview* prompt that re-parses on every keystroke and snapshots `(cursor, selected)` on entry so `Esc` can restore them. Both set `coord_input = Some(CoordInputState { … })` on the active view; `app.input_mode()` returns `InputMode::Text` so printable keys append. **Coord-input and chat-input are mutually exclusive in Net** — opening one with the other active is rejected via `last_msg`. Decoding goes through `chess_core::notation::iccs::decode_move` (Local) or `decode_move_from_view` (Net) — the latter operates on `&PlayerView` and shares its inner with the former via `decode_move_with(&Board, &[Move], &str)`. Live-preview's destination-cursor jump uses `orient::project_cell` (the long-`#[allow(dead_code)]` helper finally pulls its weight). Submit on bad notation keeps the prompt open with `last_msg = "Bad move: …"`; success closes the prompt and either calls `apply_move` (Local) or sends `ClientMsg::Move` (Net). Net spectator / not-your-turn / game-over gates are checked at submit time; `m` snapshot is `None` for `Instant`, so the Esc-restore arms in `dispatch_back` no-op there.

- **chess-web chat lives below the sidebar in a `right-column` grid.** `clients/chess-web/src/components/chat_panel.rs` renders the log + an input row (uncontrolled `<input>` read on submit) and auto-scrolls via a `create_effect` that sets `el.set_scroll_top(el.scroll_height())` on every `log.get()`. `ClientRole` enum (`state.rs`) replaces the previous `Option<Side>` observer in `<PlayPage>`; move / resign / rematch / chat-input are gated on `role.is_player()`. Spectator entry: lobby Watch button builds `/play/<id>?role=spectator` (and adds `&password=` when set); the play page reads `?role` and passes it to `room_url(...)`. Chat from a spectator hits the disabled-input path so `ClientMsg::Chat` never goes out from a spectator client (the server gate is the authoritative check).

- **chess-net has two opt-in features.** Default is `["server", "static-serve"]`. `default-features = false` exposes only the `protocol` module — that's how `chess-web` consumes the crate so axum/tokio don't end up in the WASM build. `static-serve` adds `tower-http` and the `--static-dir <path>` CLI flag (also reads `CHESS_NET_STATIC_DIR`); when set, `tower_http::services::ServeDir` is mounted as the route fallback so a single binary serves `clients/chess-web/dist/` + WS endpoints. **Enabling `--static-dir` re-routes `GET /` to serve `index.html`**, which means v1 chess-tui clients pointing at `ws://host` (no path) break — they must switch to `--connect ws://host/ws`. The default `make play-local` / `make play-lobby` flows do not enable `--static-dir`, so v1 back-compat is preserved unless you opt in.

- **chess-web compiles natively as a tiny stub.** `clients/chess-web/Cargo.toml` puts leptos + gloo-net + web-sys in `[target.'cfg(target_arch = "wasm32")'.dependencies]`. The crate's `lib.rs` `#[cfg(target_arch = "wasm32")]`-gates `app`, `components`, `pages`, `ws`, `config`. Native (`cargo check --workspace`) sees only `orient`, `glyph`, `routes`, `state` — pure-logic modules with their own tests (15 of them). The leptos UI compiles only via `cargo build --target wasm32-unknown-unknown -p chess-web` or `trunk serve`. Don't put leptos imports inside the pure modules or the workspace native check breaks.

- **chess-web duplicates `orient.rs` and `glyph.rs` from chess-tui verbatim.** This is intentional — promoting them into chess-core would violate ADR-0001 (presentation lives client-side). A shared crate (`clients/chess-client-shared`) is premature for two consumers; the duplicated tests catch drift in CI. See `backlog/promote-client-shared.md` for the trigger and recipe. If you edit one copy, edit the other.

- **chess-web routing is path-based with axum SPA fallback.** Routes: `/` (picker), `/local/:variant` (xiangqi/banqi/three-kingdom — three-kingdom renders the empty 4×8 stub board with a "WIP" overlay since the engine isn't shipped), `/lobby`, `/play/:room`. Trunk dev-server serves all paths to `index.html`; chess-net's `ServeDir` does the same via its `.fallback(ServeFile::new(dir/"index.html"))`. The same `<Board>` component renders local `GameState`-projected `PlayerView` and remote server-pushed `PlayerView` — local mode just runs `PlayerView::project(&state, state.side_to_move)` after each move (the same projection the server does), so hidden-cell behaviour matches across modes.

- **Rules are configured via URL query string on `/local/:variant`.** The picker page (`pages/picker.rs`) builds these from a reactive form; manually-typed URLs are also valid. Encoding lives in `routes.rs::{parse_local_rules, build_local_query, build_rule_set}` (pure-logic, native-tested): `?strict=1` for strict xiangqi (default = casual), `?house=chain,rush&seed=42` for banqi house rules + deterministic shuffle, `?preset=taiwan|aggressive|purist` as an alt to `house=`. Tokens: `chain`/`dark`/`rush`/`horse`/`cannon` map 1:1 to `HouseRules` flags (only `chain` + `rush` are wired in move-gen — see banqi gotcha above). Unknown tokens are dropped silently. Three-kingdom ignores all params. In-game rule editing (gear icon) is a P2 follow-up — see `backlog/web-ingame-rules-modal.md`.

## Where to put new work

| Kind | Where |
|---|---|
| Deferred features / "maybe later" | `TODO.md` via `scripts/add-todo.sh` (script lives in the [`project-knowledge-harness`](https://github.com/daviddwlee84/agent-skills) skill) |
| Research/design notes for a TODO item | `backlog/<slug>.md` (use `--backlog` on `add-todo.sh`) |
| Past traps you encountered | `pitfalls/<slug>.md` — **title by symptom** (verbatim error), not by root cause |
| Locked-in design decisions | `docs/adr/000N-<slug>.md` |
| Game rules reference | `docs/rules/<variant>.md` |
| Architectural overview / tech analysis | `docs/architecture.md` |

[`AGENTS.md`](AGENTS.md) describes the full backlog/pitfalls workflow with examples. Do **not** create new `ROADMAP.md` / `IDEAS.md` / `BACKLOG.md` files — `TODO.md` is the single index, validated by `scripts/todo-kanban.sh`.

When implementing a `TODO.md` item, in the same commit run:

```bash
scripts/promote-todo.sh --title "<substring>" --summary "<what shipped>"
```

## Pre-push checklist

CI runs all four. Run them locally before pushing:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --target wasm32-unknown-unknown -p chess-core
```

The depth-3 perft (`cargo test --release -p chess-core --test xiangqi_perft -- --ignored`) also runs in CI.
