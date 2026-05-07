# chess-web — Leptos + WASM frontend (PR-1 design)

This plan delivers `chess-web` end-to-end in a single PR with five reviewable
commits. Scope: all three variants (xiangqi / banqi / three-kingdom-banqi
stub), local pass-and-play AND online play via `chess-net`, SVG-only
rendering, no JS toolchain, Trunk dev server proxying WS to chess-net during
dev and chess-net optionally serving `dist/` in production.

## 1. Crate skeleton

All paths under `clients/chess-web/` unless noted.

```
Cargo.toml             # see §1a — add leptos, gloo-net, wasm-bindgen, console_error_panic_hook
Trunk.toml             # build target, dist dir, [[proxy]] entries → :7878
index.html             # <link data-trunk rel="rust"/>, <link rel="css">, root <main id="app">
style.css              # palette vars (red/black RGB matching chess-tui), responsive svg sizing
README.md              # one-pager: trunk serve / make play-web / wire env var
src/
  lib.rs               # mount_to_body(App), wires console_error_panic_hook
  app.rs               # <App>: top-level router, theme provider, error toast region
  routes.rs            # Route enum + parse/format helpers (kept hash-router-friendly)
  state.rs             # ClientGame enum (§4), reactive signals, dispatch handlers
  ws.rs                # WebSocket pump (§5) — open/send/recv/close, ServerMsg → signals
  config.rs            # build-time env: WS_BASE_URL (defaults to same-origin /ws)
  orient.rs            # COPY of clients/chess-tui/src/orient.rs (see §2)
  glyph.rs             # COPY of clients/chess-tui/src/glyph.rs   (see §2)
  pages/
    mod.rs
    picker.rs          # <Picker> — variant + house-rules form, mirrors TUI startup
    local.rs           # <LocalPage> — owns local GameState, renders <Board>
    lobby.rs           # <LobbyPage> — subscribes /lobby, lists rooms, "create" button
    play.rs            # <PlayPage> — owns ws.rs handle, renders <Board> from PlayerView
  components/
    mod.rs
    board.rs           # <Board view=PlayerView observer=Side> — top-level SVG container
    grid.rs            # <Grid> — files/ranks lines for current shape
    river.rs           # <River> — xiangqi only, "楚河 / 漢界" between rank 4 and 5
    palace.rs          # <PalaceDiagonals> — xiangqi only, X in both palaces
    cell.rs            # <Cell> — invisible <rect> for hit-test + cursor highlight
    piece.rs           # <PieceGlyph> — <text> with CJK or ASCII glyph + side colour
    move_dot.rs        # <MoveDot> — small filled circle on legal target squares
    cursor.rs          # <Cursor> — selected-square outline
    sidebar.rs         # <Sidebar> — side-to-move, status banner, resign / rematch / new game
    toast.rs           # <ErrorToast> — non-fatal ServerMsg::Error display, auto-dismiss
```

### 1a. Cargo.toml additions

```toml
[dependencies]
chess-core = { path = "../../crates/chess-core" }
chess-net  = { path = "../../crates/chess-net" }   # for protocol::{ClientMsg, ServerMsg, ...}
serde      = { workspace = true }
serde_json = { workspace = true }

# Leptos + Trunk world. Pin to current stable.
leptos              = { version = "0.6", features = ["csr"] }
leptos_router       = { version = "0.6", features = ["csr"] }
console_error_panic_hook = "0.1"
wasm-bindgen        = "0.2"
gloo-net            = { version = "0.6", features = ["websocket"] }
gloo-timers         = { version = "0.3", features = ["futures"] }
futures-util        = { workspace = true }
js-sys              = "0.3"
web-sys             = { version = "0.3", features = ["Window", "Location", "console"] }
```

`chess-net` already pulls axum/tokio; we depend on it solely for
`chess_net::protocol::*`. To keep the WASM build clean we will add a
`protocol-only` cargo feature on `chess-net` in commit 5 (gates the
axum/tokio deps behind `default = ["server"]`). That's the only chess-net
change required by the web client itself; the static-serve change is
additive (§7).

### 1b. Workspace plumbing

* `Cargo.toml` workspace members already includes `clients/chess-web`. No
  change needed.
* Add to workspace metadata: `[workspace.metadata.trunk] target = "clients/chess-web/index.html"` (optional convenience).

## 2. Sharing orient.rs / glyph.rs — pick (a), schedule (c)

**Decision: duplicate (a) for PR-1; record promotion to a `chess-client-shared`
crate (c) as a P2 TODO.** Justification:

* The two files together are ~230 LOC of pure data tables + arithmetic with
  `#[cfg(test)]` round-trip proofs. They have no internal churn pressure
  (last edited when banqi shipped; now stable).
* Promoting to chess-core (b) violates ADR-0001's engine/presentation split.
  CLAUDE.md explicitly notes "chess-tui orientation lives in
  clients/chess-tui/src/orient.rs, not chess-core" — we keep that invariant.
* Creating `clients/chess-client-shared` now adds a fourth member to the
  clients dir for two files. It earns its keep only when a third consumer
  (chess-ai's debug viewer? a future Tauri shell?) appears. Premature.
* Drift risk is mitigated by the existing test suites: TUI tests cover
  `xiangqi_red_round_trip`, `banqi_round_trip_and_dims`, the 180-degree
  rotation property, and the glyph-table totals. We mirror those tests in
  `chess-web` so any divergence trips CI on both crates simultaneously.

PR-1 includes a `TODO.md` entry with `--backlog` linking to a stub at
`backlog/promote-client-shared.md` describing the trigger ("third consumer
or first behavioural divergence between TUI and web").

## 3. Routing

Use `leptos_router` with **hash routing** (`#/local/xiangqi`,
`#/lobby`, `#/play/main`). Reasoning:

* Works under `file://` for offline `trunk build` testing without a server.
* No server-side rewrite rule needed when chess-net serves `dist/` (every
  path resolves to `index.html` automatically because the route is in the
  fragment).
* GitHub Pages friendly if we later host static-only.

Routes:

| Path                   | Component       | Notes |
|------------------------|-----------------|-------|
| `/`                    | `<Picker>`      | variant + house rules form, "Play locally" / "Browse online" buttons |
| `/local/:variant`      | `<LocalPage>`   | `:variant` ∈ `xiangqi`, `xiangqi-strict`, `banqi`, `three-kingdom`. Banqi accepts `?seed=<u64>&house=<csv>` |
| `/lobby`               | `<LobbyPage>`   | subscribes to `/lobby` ws; "create room" form |
| `/play/:room`          | `<PlayPage>`    | accepts `?password=` query (forwarded to ws URL); shows "spectator slot full" if server returns room-full |

`leptos_router::Router` with `<Route path="/local/:variant" view=LocalPage/>`
etc. The `:variant` param parses to `Variant` + `RuleSet` via the same
helper `routes::parse_variant_slug`.

## 4. State model — `ClientGame`

```rust
// state.rs
pub enum ClientGame {
    /// Local pass-and-play. We own the authoritative GameState and project
    /// it on the fly each render. `observer` flips after every move so the
    /// person who's about to play sees their own pieces at the bottom.
    Local { state: chess_core::state::GameState, observer: chess_core::piece::Side },
    /// Online: the server is authoritative. We hold the most recent
    /// PlayerView and the seat we were assigned.
    Online { view: chess_core::view::PlayerView, observer: chess_core::piece::Side, rules: chess_core::rules::RuleSet },
}

impl ClientGame {
    pub fn current_view(&self) -> std::borrow::Cow<'_, chess_core::view::PlayerView> { ... }
    pub fn observer(&self) -> chess_core::piece::Side { ... }
}
```

`<Board view=Signal<PlayerView> observer=Signal<Side>/>` is the only API the
component consumes — it does not know whether it's local or online. After
a local move:

```rust
state.make_move(&mv)?;
state.refresh_status();                        // CLAUDE.md gotcha
let observer_next = state.side_to_move;        // optional auto-flip
let view = PlayerView::project(&state, observer_next);
set_signal(view);
```

After an online move we just send `ClientMsg::Move{mv}` and wait for the
server's `Update{view}` to land via the ws pump. Local mode therefore
exercises the same projection code path the server runs — keeps "what
hidden cells look like" identical across both modes.

## 5. WebSocket layer

**Crate: `gloo-net::websocket::futures::WebSocket`.** Higher-level than
`web-sys::WebSocket`, less abstraction debt than `reqwasm`, and gives us
real `Stream`/`Sink` adapters that play with `futures-util` cleanly.

`ws.rs` exposes:

```rust
pub struct WsHandle {
    outbound: futures::channel::mpsc::UnboundedSender<chess_net::protocol::ClientMsg>,
    // signals: server_msg, status (Connecting/Open/Closed{reason})
}

pub fn connect(url: String) -> (WsHandle, ReadSignal<Option<ServerMsg>>, ReadSignal<ConnState>);
```

Internals: `wasm_bindgen_futures::spawn_local` two tasks:

1. **Read pump** — `while let Some(frame) = ws.next().await { ... }`
   `Message::Text(s)` → `serde_json::from_str::<ServerMsg>(&s)` → set
   `last_msg` signal. The `<PlayPage>` has an `Effect` that pattern-matches
   the signal and updates `ClientGame::Online { view }` for `Update`,
   replaces it for `Hello`, surfaces `Error` to the toast region.
2. **Write pump** — `outbound_rx.next().await` → serialize → `ws.send`.

Reconnect / retry: out of scope for PR-1. On close we set `ConnState::Closed`
and the `<PlayPage>` renders a banner "Disconnected — reload the page" with
a `<button on:click=reload>`. A `// TODO(reconnect)` comment marks the spot
plus a `TODO.md` entry "chess-web: ws auto-reconnect with exponential
backoff" (P2).

URL construction (`config.rs`):

```rust
pub fn ws_url(path: &str, password: Option<&str>) -> String {
    // 1. compile-time CHESS_WEB_WS_BASE env (set by Trunk if you want absolute URL in prod)
    // 2. fallback: same origin, swap http→ws / https→wss, append `path`
}
```

Default deployment ⇒ same-origin, served by chess-net itself, so no env
var needed. Dev mode goes through Trunk's WS proxy (§8), still same-origin
from the browser's perspective.

## 6. SVG board rendering

One viewBox, integer coordinates, dimensions in "board units" so we can
restyle by editing CSS only:

* **Unit cell**: 60 × 60. Xiangqi board: 9 files × 10 ranks ⇒ inner grid
  480 × 540, with 30-unit margins ⇒ viewBox `"0 0 540 600"`. Banqi (8 cols
  × 4 rows after transpose): `"0 0 510 270"`. Three-kingdom: render the
  4×8 bounding box with greyed-out unplayable cells and a "variant not yet
  shipped" overlay (gracefully matches CLAUDE.md gotcha).
* **Intersections, not boxes** — pieces sit on grid crossings. Component
  composition mirrors §1's `components/` listing. Render order:
  Grid → River → PalaceDiagonals → Cells (invisible hit-test) → MoveDots
  → PieceGlyphs → Cursor.
* **Hit-test = `<rect class="cell" on:click=...>`** sized 60×60 centred on
  each crossing. Browser handles geometry; no `hit_test` math needed.
  Touch input gets pointer-events for free; `touch-action: manipulation` in
  CSS to disable double-tap-zoom on mobile.
* **Piece glyph**: `<text x=cx y=cy text-anchor="middle" dominant-baseline="central"
  class={"piece-{side}"}>` + CJK or ASCII string from `glyph::glyph(...)`.
  Style tokens default to CJK; ASCII is opt-in via a header toggle stored
  in `localStorage` (deferred — PR-1 ships CJK only, header toggle is a
  P2 TODO line).
* **Palette** (CSS custom properties, settable per theme):
  `--red:  #b22222;`  `--black: #1a1a1a;`  `--board: #f4d2a3;`
  `--grid: #5a3b1c;`  `--river: #888;`  `--cursor: #2a7fff;`
  `--move-dot: rgba(34,127,255,0.45);`. Matches chess-tui's existing tone.
* **River** = `<text>` "楚 河 漢 界" sitting on a transparent band between
  ranks 4 and 5; vertical grid lines stop at rank 4 and resume at rank 5.
* **Palace diagonals** = two `<line>` per side from corner to corner of the
  3×3 palace.
* **Banqi hidden tile** = `<rect class="tile-back"/>` + `<text>暗</text>`
  (CJK) / `?` (ASCII), via `glyph::hidden(style)`.
* **Last-move highlight & cursor**: `<rect>` with stroked border; cursor is
  derived from a `(selected: Option<Square>, hover: Option<Square>)` signal
  pair on `<Board>`.

## 7. chess-net static-serve change

Minimal, additive, feature-gated. In `crates/chess-net/Cargo.toml`:

```toml
[features]
default      = ["server", "static-serve"]
server       = ["dep:axum", "dep:tokio", "dep:futures-util"]
static-serve = ["dep:tower-http"]
protocol-only = []  # for chess-web — disables server + static-serve

[dependencies]
tower-http = { version = "0.5", features = ["fs"], optional = true }
axum         = { workspace = true, optional = true }
tokio        = { workspace = true, optional = true }
futures-util = { workspace = true, optional = true }
```

In `crates/chess-net/src/server.rs` `serve()`:

```rust
let mut app = Router::new()
    .route("/ws", get(upgrade_default))
    .route("/ws/:room_id", get(upgrade_room))
    .route("/lobby", get(upgrade_lobby))
    .route("/rooms", get(rooms_snapshot_json));

#[cfg(feature = "static-serve")]
if let Some(dir) = static_dir {
    app = app.fallback_service(tower_http::services::ServeDir::new(dir).append_index_html_on_directories(true));
} else {
    app = app.route("/", get(upgrade_default));   // current behaviour
}
#[cfg(not(feature = "static-serve"))]
{
    app = app.route("/", get(upgrade_default));
}
```

`/` no longer unconditionally upgrades when serving statics; the SPA's
`index.html` lives there instead. The default route `main` becomes
`GET /ws` only (still v1-compatible — old `chess-tui --connect ws://host`
hits `/` and gets the SPA, but old clients always used `/ws` in practice
per `clients/chess-tui/src/url.rs`).

`bin/server.rs` gains `--static-dir <path>` (CLI flag, defaults to env
`CHESS_NET_STATIC_DIR`, falls back to None ⇒ no static serving). Dev loop
sets `--static-dir clients/chess-web/dist`. Compile-time embedding via
`include_dir!` is rejected for PR-1 (rebuild loop is too slow with embedded
WASM).

## 8. Trunk dev-server proxy

`clients/chess-web/Trunk.toml`:

```toml
[build]
target = "index.html"
dist   = "dist"

[serve]
address = "127.0.0.1"
port    = 8080
open    = false

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

Trunk supports `ws = true` since 0.18 (verified — it forwards the upgrade
header). If a corporate Trunk pin lacks WS proxy, fallback path: set
`CHESS_WEB_WS_BASE=ws://127.0.0.1:7878` at `trunk serve` time — the
browser hits chess-net directly. Document both in the README.

## 9. Makefile / dev loop

Add to `Makefile`:

```makefile
WEB_PORT ?= 8080

.PHONY: play-web build-web stop-web

play-web:
	@scripts/play-web.sh chess-web $(PORT) $(WEB_PORT) $(VARIANT)

build-web:
	cd clients/chess-web && trunk build --release
	@echo "dist/ ready — pass --static-dir clients/chess-web/dist to chess-net-server"

stop-web:
	-tmux kill-session -t chess-web 2>/dev/null
```

`scripts/play-web.sh` mirrors `play-local.sh`: window 0 = `cargo run -p chess-net -- --port $PORT $VARIANT --static-dir clients/chess-web/dist`, window 1 = `cd clients/chess-web && trunk serve --port $WEB_PORT`. Add `wasm` and `web` to the `check` target's prereqs eventually; for PR-1 leave `make check` alone (Trunk is a dev-only dependency, not a CI gate).

## 10. Phasing — five reviewable commits in one PR

1. **Scaffold + variant picker.** Writes `Cargo.toml`, `Trunk.toml`,
   `index.html`, `style.css`, `lib.rs`, `app.rs`, `routes.rs`,
   `pages/picker.rs`. No board rendering yet — just confirms `trunk serve`
   produces a clickable form that navigates to a stub `<LocalPage>`. CI:
   `cargo build --target wasm32-unknown-unknown -p chess-web`.
2. **SVG xiangqi board + local play.** Adds duplicated `orient.rs`,
   `glyph.rs`, the full `components/` tree, and `pages/local.rs` for
   xiangqi. Local pass-and-play works. Tests mirrored from chess-tui.
3. **Banqi + three-kingdom stub.** Banqi rendering (transposed 8×4,
   hidden tiles, `f` key flip via button), three-kingdom shows the empty
   4×8 with the "variant not yet shipped" overlay.
4. **WS + lobby + online play.** `ws.rs`, `pages/lobby.rs`,
   `pages/play.rs`, error toast. Works against `cargo run -p chess-net`.
5. **chess-net static-serve + Makefile + scripts.** The `tower-http`
   feature, `--static-dir` flag, `play-web.sh`, Makefile targets,
   `protocol-only` feature on chess-net so the web build doesn't drag
   axum/tokio into WASM. Update CLAUDE.md "where to put new work" + add
   `make play-web` to the commands block.

## 11. Testing strategy

* **Unit (native)**: orient.rs / glyph.rs round-trip and table-total tests
  cloned verbatim from chess-tui — they're pure logic, no
  `wasm-bindgen-test` needed.
* **Component logic (native)**: helpers in `state.rs` (`ClientGame::after_move`,
  `routes::parse_variant_slug`) get plain `#[test]`s.
* **WASM smoke**: a single `wasm-bindgen-test` that mounts `<App>` into a
  detached `body`, navigates the route, and asserts the picker renders
  three buttons. Anything beyond is deferred.
* **E2E**: out of scope for PR-1. Add a `backlog/web-playwright.md` stub
  with the recipe ("run `cargo run -p chess-net -- ... --static-dir
  clients/chess-web/dist` and `npx playwright test`") tagged P2.
* **Manual smoke checklist** in the PR description: open two tabs against
  `make play-web`, play a full xiangqi game, switch one tab to banqi,
  confirm hidden tiles render, refresh both ⇒ confirm Hello/Update flow.

## 12. Deferred — TODO.md entries to add in PR-1

All filed via `scripts/add-todo.sh`, P2 unless noted, tagged
`area:chess-web`:

* WS auto-reconnect with exponential backoff (P2)
* CJK / ASCII glyph toggle in header, persisted to `localStorage` (P3)
* Move animations (translate piece along path) (P3)
* History scrubber + undo via UI (mirror TUI `u` key) (P2)
* Mobile-optimized layout (portrait orientation, larger hit targets) (P3)
* PWA manifest + service-worker offline cache for local mode (P3)
* i18n strings table (Chinese vs English UI) (P3)
* Spectator mode — third+ connection joins as observer, no seat (P2,
  needs chess-net change too)
* WebRTC peer-to-peer fallback for local-network play without a server
  (P3, large)
* Promote `clients/chess-web/src/{orient,glyph}.rs` to
  `clients/chess-client-shared` once a third consumer arrives or the two
  copies diverge (P2, links to `backlog/promote-client-shared.md`)
* Playwright E2E harness against `--static-dir` builds (P2, links to
  `backlog/web-playwright.md`)

### Critical Files for Implementation

- /Volumes/Data/Program/tries/2026-05-06-chinese-chess/clients/chess-web/Cargo.toml
- /Volumes/Data/Program/tries/2026-05-06-chinese-chess/clients/chess-web/src/components/board.rs
- /Volumes/Data/Program/tries/2026-05-06-chinese-chess/clients/chess-web/src/ws.rs
- /Volumes/Data/Program/tries/2026-05-06-chinese-chess/clients/chess-web/src/state.rs
- /Volumes/Data/Program/tries/2026-05-06-chinese-chess/crates/chess-net/src/server.rs
