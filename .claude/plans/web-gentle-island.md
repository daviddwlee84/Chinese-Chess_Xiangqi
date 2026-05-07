# chess-web — Leptos + WASM frontend

## Context

`crates/chess-core` has been WASM-clean from day one (verified in CI via `cargo build --target wasm32-unknown-unknown -p chess-core`), and `crates/chess-net` already speaks a clean JSON protocol with multi-room lobby support. The `clients/chess-web/` crate has been a 2-line stub waiting for this PR. `TODO.md` lists "chess-web: Leptos + WASM frontend" at P2 with the line `Same chess-core compiled to wasm32. Consumes PlayerView from chess-net. Mouse-first; vim-style keys for parity with TUI.`

This PR ships chess-web as a single-page SVG-rendered web client supporting **both local pass-and-play (pure WASM, no server)** and **online play (WebSocket to chess-net)**. SVG-only — no art assets — so the first cut depends on no image pipeline. The user's worry about "websocket being redundant for solo play" is addressed by the dual-mode design: WS only opens when the user enters the lobby.

## Recommended approach

### Crate skeleton

Under `clients/chess-web/`:

```
Cargo.toml                # +leptos 0.6, leptos_router, gloo-net, console_error_panic_hook,
                          #  web-sys, futures-util, chess-net (default-features=false, features=["protocol-only"])
Trunk.toml                # build target + [[proxy]] entries → :7878
index.html                # <link data-trunk rel="rust"/> + <main id="app"></main>
style.css                 # CSS custom-property palette, responsive svg sizing
src/lib.rs                # #[wasm_bindgen(start)] mount_to_body(App), wires panic hook
src/app.rs                # <App>: Router + theme + ErrorToast region
src/routes.rs             # Route enum + parse_variant_slug
src/state.rs              # ClientGame enum, reactive signals
src/ws.rs                 # WebSocket pump (gloo-net::websocket::futures::WebSocket)
src/config.rs             # ws_url() — same-origin default, CHESS_WEB_WS_BASE override
src/orient.rs             # COPY of clients/chess-tui/src/orient.rs (incl. tests)
src/glyph.rs              # COPY of clients/chess-tui/src/glyph.rs (incl. tests)
src/pages/{picker,local,lobby,play}.rs
src/components/{board,grid,river,palace,cell,piece,move_dot,cursor,sidebar,toast}.rs
```

### Sharing orient.rs / glyph.rs across TUI and web → **duplicate**

Both files are stable, ~230 LOC of pure data + arithmetic with round-trip tests. Promoting into `chess-core` would violate ADR-0001 (CLAUDE.md gotcha: "chess-tui orientation lives in `clients/chess-tui/src/orient.rs`, not chess-core. The engine stays presentation-free"). A shared crate is premature for two consumers. Mitigation: copy the tests too, so any drift trips both crates' CI in the same run. Add `backlog/promote-client-shared.md` recording the trigger.

### Routing — hash routing via `leptos_router`

`#/local/xiangqi`, `#/lobby`, `#/play/main` — works under `file://`, no server-side rewrite needed when chess-net serves `dist/`, GitHub-Pages friendly.

| Path | Component |
|---|---|
| `/` | `<Picker>` (variant + house rules) |
| `/local/:variant` | `<LocalPage>` — `?seed=&house=` for banqi |
| `/lobby` | `<LobbyPage>` — subscribes to `/lobby` ws |
| `/play/:room` | `<PlayPage>` — `?password=` |

### State model — `ClientGame`

```rust
pub enum ClientGame {
    Local { state: GameState, observer: Side },
    Online { view: PlayerView, observer: Side, rules: RuleSet },
}
```

`<Board view=Signal<PlayerView> observer=Signal<Side>/>` is the only API the component knows. Local mode: after every move call `state.make_move(&mv)?; state.refresh_status(); PlayerView::project(&state, side_to_move)` — same projection the server runs, so hidden-cell behaviour matches across modes (see CLAUDE.md gotcha: "make_move does NOT auto-refresh status"). Online mode just sends `ClientMsg::Move{mv}` and renders the next `Update{view}`.

### WebSocket layer — `gloo-net`

`gloo-net::websocket::futures::WebSocket` (real Stream/Sink, smaller than reqwasm). `ws.rs` exposes `connect(url) -> (WsHandle, ReadSignal<Option<ServerMsg>>, ReadSignal<ConnState>)` with two `spawn_local` tasks — read pump deserializes `ServerMsg` into a signal, write pump drains an `mpsc::UnboundedSender<ClientMsg>`. PR-1 reconnect = "Disconnected — reload" banner + `backlog/web-ws-reconnect.md` for backoff retry.

`Move::Reveal` always sent as `revealed: None` per the protocol contract (`crates/chess-net/src/protocol.rs:46`).

### SVG board

Single `<svg>` with integer "board units": cell = 60×60. Xiangqi viewBox `0 0 540 600`, banqi `0 0 510 270`. Render order: Grid → River → PalaceDiagonals → Cells (invisible `<rect>` for hit-test) → MoveDots → PieceGlyphs → Cursor. Hit-test = browser-native `on:click` on each cell rect (no manual math like the TUI's 4-col × 2-row trick). Pieces are `<text text-anchor="middle" dominant-baseline="central">` with `glyph::glyph(...)`. River = `<text>"楚 河 漢 界"</text>` with grid lines breaking between rank 4 and rank 5. Palace = two `<line>` per side. Banqi hidden tile = `<rect class="tile-back"/>` + `<text>暗</text>` from `glyph::hidden(style)`. Three-kingdom renders the empty 4×8 with a "variant not yet shipped" overlay (per CLAUDE.md gotcha — engine stub is empty 4×8). Palette via CSS custom properties so theming is a stylesheet edit.

### chess-net static-serve change

Minimal additions to `crates/chess-net/Cargo.toml` and `crates/chess-net/src/server.rs`:

```toml
# Cargo.toml
[features]
default        = ["server", "static-serve"]
server         = []                                    # axum + tokio (existing)
static-serve   = ["dep:tower-http"]
protocol-only  = []                                    # for chess-web WASM consumer

[dependencies]
tower-http = { version = "0.5", features = ["fs"], optional = true }
```

In `server.rs::serve`, when `--static-dir <path>` (or `CHESS_NET_STATIC_DIR` env) is set, install `tower_http::services::ServeDir` via `app.fallback_service(...)`. Otherwise keep the current `route("/", get(upgrade_default))`. `chess-web` consumes `chess-net` with `default-features = false, features = ["protocol-only"]` so axum/tokio do not leak into the WASM build.

### Trunk dev-server proxy

`Trunk.toml`:

```toml
[serve]
address = "127.0.0.1"
port    = 8080

[[proxy]]
backend = "ws://127.0.0.1:7878/ws"
ws      = true
rewrite = "/ws"

[[proxy]]
backend = "ws://127.0.0.1:7878/lobby"
ws      = true
rewrite = "/lobby"

[[proxy]]
backend = "http://127.0.0.1:7878/rooms"
rewrite = "/rooms"
```

Trunk has supported `ws = true` since 0.18. Fallback (older Trunks): `CHESS_WEB_WS_BASE=ws://127.0.0.1:7878` env var consumed by `config::ws_url`; browser hits chess-net directly.

### Makefile

Add to root `Makefile` (mirroring existing `play-local` / `play-lobby` style):

- `make play-web` — tmux session: pane 0 = `cargo run -p chess-net -- --port 7878 xiangqi`, pane 1 = `trunk serve` from `clients/chess-web/`. Script at `scripts/play-web.sh`.
- `make build-web` — `(cd clients/chess-web && trunk build --release)`.
- `make stop-web` — kill tmux session.

Leave `make check` untouched — Trunk is dev-only, not a CI gate.

### Phasing — 5 commits, 1 PR

1. **Scaffold**: `Cargo.toml`, `Trunk.toml`, `index.html`, `style.css`, `<App>` + `<Picker>` + routes (no board yet).
2. **Xiangqi local**: SVG board + `<Grid>`/`<River>`/`<Palace>`/`<Cell>`/`<Piece>` + duplicated `orient.rs`/`glyph.rs` + cloned tests + `<LocalPage>` for xiangqi.
3. **Banqi + three-kingdom**: transposed display, hidden tiles, `Move::Reveal` UX in local mode, three-kingdom stub overlay.
4. **Online**: `ws.rs` + `<LobbyPage>` + `<PlayPage>` + `<Toast>` + rematch/resign buttons.
5. **Static-serve + Make**: chess-net `tower-http` static-serve, `--static-dir` flag, `protocol-only` feature gate, `Makefile` targets, `scripts/play-web.sh`, CLAUDE.md update, TODO.md promote-todo + new follow-up entries.

## Critical files to modify or create

- `clients/chess-web/Cargo.toml` (rewrite)
- `clients/chess-web/Trunk.toml` (new)
- `clients/chess-web/index.html` (new)
- `clients/chess-web/style.css` (new)
- `clients/chess-web/src/lib.rs` (rewrite)
- `clients/chess-web/src/{app,routes,state,ws,config,orient,glyph}.rs` (new)
- `clients/chess-web/src/pages/{picker,local,lobby,play}.rs` (new)
- `clients/chess-web/src/components/{board,grid,river,palace,cell,piece,move_dot,cursor,sidebar,toast}.rs` (new)
- `crates/chess-net/Cargo.toml` (add `static-serve` + `protocol-only` features, optional `tower-http`)
- `crates/chess-net/src/server.rs` (add `--static-dir` fallback service)
- `crates/chess-net/src/bin/server.rs` (add `--static-dir` CLI flag / env var)
- `Makefile` (add `play-web`, `build-web`, `stop-web` targets)
- `scripts/play-web.sh` (new — mirror `scripts/play-local.sh`)
- `CLAUDE.md` (add chess-web command block, gotchas, dev-loop notes)
- `TODO.md` — promote chess-web entry via `scripts/promote-todo.sh`; add follow-ups
- `backlog/promote-client-shared.md`, `backlog/web-ws-reconnect.md`, `backlog/web-playwright.md` (new)

## Existing functions/utilities to reuse (verbatim or via dependency)

- `chess_core::state::GameState::new(rules)` + `state.make_move(&mv)` + `state.refresh_status()` — local engine
- `chess_core::view::PlayerView::project(&state, observer)` — same projection the server uses
- `chess_core::rules::{RuleSet, HouseRules, Variant}` — already serde-clean
- `chess_core::moves::Move` — directly serializable into `ClientMsg::Move{mv}`
- `chess_net::protocol::{ServerMsg, ClientMsg, RoomSummary, RoomStatus, PROTOCOL_VERSION}` — pulled in via `protocol-only` feature
- `clients/chess-tui/src/orient.rs::{display_dims, project_cell, square_at_display}` — copied verbatim
- `clients/chess-tui/src/glyph.rs::{glyph, hidden, side_name, Style}` — copied verbatim

## Verification

End-to-end smoke (manual, captured in PR description checklist):

1. `cargo check --workspace` — chess-web compiles with default workspace target.
2. `cargo build --target wasm32-unknown-unknown -p chess-web` — WASM build clean (chess-net pulled with `protocol-only` feature so no axum/tokio leak).
3. `cargo test --workspace` — duplicated orient/glyph tests pass; new `routes::parse_variant_slug` and `state::ClientGame::after_move` unit tests pass.
4. `cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean.
5. `make play-web` — tmux opens; `http://localhost:8080/#/local/xiangqi` plays a full xiangqi game (cursor hover shows legal-move dots, click-click commits, sidebar reflects turn + status).
6. Same URL with `#/local/banqi?seed=42&house=taiwan` — banqi shows transposed 8×4 layout, all tiles hidden, click flips, Taiwan chain-capture works.
7. `#/local/three-kingdom` — empty 4×8 with "variant not yet shipped" overlay (no crash).
8. Two browser tabs to `#/lobby`: create room "test" in tab A, see it in tab B's lobby list (live `Rooms` push), join from tab B → game starts. Make a move in each tab; confirm the opposite tab updates. Resign in tab A → tab B sees `Won{Resignation}`. Both click Rematch → fresh game.
9. With wrong password: `#/play/test?password=wrong` → toast "bad password", connection closes (no crash).
10. `make build-web && cargo run -p chess-net -- --port 7878 --static-dir clients/chess-web/dist xiangqi` then `http://localhost:7878/` — same SPA served by axum, same online flow.
11. One `wasm-bindgen-test`: mount `<App>`, assert picker renders three variant buttons.

## Deferred (P2/P3 entries to add to TODO.md in this PR)

WS auto-reconnect (backoff); CJK/ASCII glyph toggle in UI; move animations (CSS transitions); history scrubber + undo via UI; mobile-optimized layout; PWA / offline cache; i18n strings table; spectator mode (also needs chess-net change); WebRTC peer-to-peer fallback; promote-to-shared-crate (links to `backlog/promote-client-shared.md`); Playwright E2E (links to `backlog/web-playwright.md`).
