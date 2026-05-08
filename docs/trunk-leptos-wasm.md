# `chess-web` — Rust web stack (Leptos + Trunk + WASM)

Reference for the toolchain, project layout, and dev/prod workflows
that ship the browser frontend. For *why* (vs. JS frameworks, vs. Yew /
Dioxus), see [`architecture.md`](architecture.md). For *what shipped
in PR-1*, see the `chess-web` Done entry in [`../TODO.md`](../TODO.md).
For traps that have bitten us, see [`../pitfalls/`](../pitfalls/).

## Toolchain

```bash
rustup target add wasm32-unknown-unknown    # one-time, per machine
cargo install trunk                          # one-time, per machine
```

`trunk --version` should print **0.21.x or newer**. Older Trunks lack
`addresses` (replaces deprecated `address`) and `ws = true` proxy support.

If your rustup mirror lacks the `wasm32-unknown-unknown` component (e.g.
the tuna mirror occasionally drops it), prefix the target install with
`RUSTUP_DIST_SERVER=https://static.rust-lang.org`. See
[`../pitfalls/wasm-getrandom-unresolved-imp.md`](../pitfalls/wasm-getrandom-unresolved-imp.md)
for a sibling rustup pitfall.

`cargo install trunk` pulls ~500 deps and compiles for several minutes
on first run; the binary lands in `~/.cargo/bin/trunk`.

## Crate layout

```
clients/chess-web/
├── Cargo.toml          # split deps: native vs target_arch="wasm32"
├── Trunk.toml          # build target + dev-server proxies
├── index.html          # Trunk entry + pre-WASM loading shell
├── style.css           # CSS custom-property palette + boot loader
├── src/
│   ├── lib.rs          # mounts Leptos via #[wasm_bindgen(start)]
│   ├── app.rs          # <App>: leptos_router routes
│   ├── routes.rs       # variant slug parser  ── pure-logic, native
│   ├── orient.rs       # display orientation  ── pure-logic, native (copy of chess-tui)
│   ├── glyph.rs        # piece glyph tables   ── pure-logic, native (copy of chess-tui)
│   ├── state.rs        # ClientGame helpers   ── pure-logic, native
│   ├── config.rs       # ws_url() helpers     ── wasm32 only
│   ├── ws.rs           # gloo-net WS pump     ── wasm32 only
│   ├── components/     # SVG board + sidebar  ── wasm32 only
│   └── pages/          # Picker / Local / Lobby / Play  ── wasm32 only
└── dist/               # trunk build output (gitignored)
```

`crate-type = ["cdylib", "rlib"]` is required: `cdylib` for
`wasm-bindgen` to produce a `.wasm` module, `rlib` so unit tests in
the pure-logic modules compile against the host target.

## Cargo.toml conventions

The crate **must** stay buildable on the host target so `cargo check
--workspace` (the default CI gate) passes for all 8 workspace crates.
We achieve that by gating browser-only deps behind a target predicate:

```toml
# Native — only chess-core, chess-net protocol, serde.
[dependencies]
chess-core = { path = "../../crates/chess-core" }
serde      = { workspace = true }
serde_json = { workspace = true }
chess-net  = { path = "../../crates/chess-net", default-features = false }

# Browser-only — leptos, gloo-net, web-sys, wasm-bindgen.
[target.'cfg(target_arch = "wasm32")'.dependencies]
leptos        = { version = "0.6", features = ["csr"] }
leptos_router = { version = "0.6", features = ["csr"] }
gloo-net      = { version = "0.6", default-features = false, features = ["websocket"] }
web-sys       = { version = "0.3", features = ["Window", "Location"] }
wasm-bindgen  = "0.2"
wasm-bindgen-futures = "0.4"
js-sys        = "0.3"
console_error_panic_hook = "0.1.7"
futures-util  = { workspace = true }
futures       = "0.3"
```

Two non-obvious points:

1. **`chess-net` is pulled in with `default-features = false`.** That
   selects the `protocol-only` subset of the crate (just `ServerMsg` /
   `ClientMsg` / friends). With default features it would drag in
   axum + tokio, which do not compile to WASM.
2. **`gloo-net` itself is `default-features = false, features = ["websocket"]`.**
   The `http` feature pulls in `reqwest`-flavoured machinery the SPA
   doesn't need; trimming it shrinks the WASM bundle measurably.

`lib.rs` mirrors the gating:

```rust
pub mod glyph;     // pure-logic, native + wasm
pub mod orient;    // pure-logic, native + wasm
pub mod routes;    // pure-logic, native + wasm
pub mod state;    // pure-logic, native + wasm

#[cfg(target_arch = "wasm32")] mod app;
#[cfg(target_arch = "wasm32")] mod components;
#[cfg(target_arch = "wasm32")] mod config;
#[cfg(target_arch = "wasm32")] mod pages;
#[cfg(target_arch = "wasm32")] mod ws;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    leptos::mount_to_body(app::App);
}
```

If you import a leptos type inside `state.rs` or any pure-logic module,
the workspace check breaks. Keep leptos/web-sys/gloo strictly inside
the wasm32-gated tree.

## Trunk.toml — dev server + proxies

```toml
[build]
target  = "index.html"
release = false

[serve]
addresses = ["127.0.0.1"]   # 0.21+; the singular `address` is deprecated
port      = 8080
open      = false

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

A single `/ws` proxy entry covers BOTH the bare `/ws` route and
`/ws/<room>` (Trunk nests proxies as prefix routes in axum). Adding a
separate `/ws/` entry alongside `/ws` panics on startup with `Invalid
route … conflict with previously registered route` — see
[`../pitfalls/trunk-proxy-route-conflict.md`](../pitfalls/trunk-proxy-route-conflict.md).

The proxies exist so the SPA can use **same-origin URLs** for both
the dev server and prod (where chess-net's `--static-dir` mounts the
SPA at the same origin as the WS endpoints). `config::ws_base()` reads
`window.location.host`; nothing is hardcoded.

## Dev loop

```bash
make play-web        # tmux: pane 0 = chess-net :7878, pane 1 = trunk serve :8080
make stop-web        # tear down the tmux session

# or run them yourself:
cargo run -p chess-net -- --port 7878 xiangqi    # in one terminal
(cd clients/chess-web && trunk serve)             # in another
```

`trunk serve` is **development only**. It builds a debug WASM bundle
and keeps Trunk's live-reload websocket open. Do not expose this path to
remote users unless you're intentionally debugging with them.

`trunk serve` watches `src/`, `index.html`, `style.css`, and `Cargo.toml`.
On any change it recompiles, regenerates the WASM bundle, and pushes a
reload over its own websocket to every connected browser tab. The first
build takes 2–3 minutes (leptos compilation); incremental builds
typically finish in a couple of seconds.

Open `http://127.0.0.1:8080/` once trunk reports `📡 server listening`.
Client-side navigation uses normal paths (`/local/xiangqi`, `/play/foo`,
…); Trunk's dev server serves `index.html` for the app routes while
proxying `/ws*`, `/lobby`, and `/rooms` to chess-net.

## Production build

```bash
make serve-web-prod ADDR=0.0.0.0:7878            # release SPA + WS, remote-ready

# Equivalent manual flow:
make build-web                                   # trunk build --release
cargo run -p chess-net -- --addr 0.0.0.0:7878 \
    --static-dir clients/chess-web/dist xiangqi  # one binary, SPA + WS
```

There are two production-style web builds:

- `make build-web` / `make build-web-server` — server-hosted, same-origin
  build under `clients/chess-web/dist`. Use with `chess-net --static-dir`.
- `make build-web-static WEB_BASE=/chinese-chess` — GitHub Pages build under
  `clients/chess-web/dist-static`. It sets Trunk's public URL to the Pages
  subpath and copies `index.html` to `404.html` for SPA hard-refresh fallback.

The static build cannot assume `/lobby` and `/ws/<room>` exist on GitHub
Pages. Its Online lobby asks for a websocket base URL (`wss://...`) and
preserves it as `?ws=...` across lobby/play routes; Local pass-and-play works
without a server.

`trunk build --release`:

- Sets `--release` on the cargo invocation (so `[profile.release]` from
  the workspace root applies — `lto = "thin"`, `codegen-units = 1`,
  `panic = "abort"`).
- Runs `wasm-opt` if available on `$PATH` for additional size shrinkage.
- Writes hashed asset filenames under `clients/chess-web/dist/`:
  `index.html`, `chess-web-<hash>.js`, `chess-web-<hash>_bg.wasm`,
  `style-<hash>.css`.

A debug build from `trunk serve` is around **5–7 MB** of `.wasm`; release
is around **800 KB–1.2 MB** depending on `wasm-opt` availability. On a
recent local build without `wasm-opt`, release was 856 KB raw and about
321 KB over gzip. The user still waits for download + browser
compile/instantiate on first load, so `index.html` includes a lightweight
loading shell that is removed after Trunk emits `TrunkApplicationStarted`.

If Trunk 0.21 errors with `invalid value '1' for '--no-color'`, your
environment has `NO_COLOR=1`. Use `NO_COLOR=false trunk build --release`
or unset `NO_COLOR`; the `make build-web` target already applies that
override.

When chess-net is started with `--static-dir`, it mounts
`tower_http::services::ServeDir` as the route fallback with a
`ServeFile::new(dir/"index.html")` backstop, so any path that doesn't
match `/ws*` / `/lobby` / `/rooms` falls through to `index.html` and
the SPA's client-side router takes over. **One trade-off**: enabling
`--static-dir` re-routes `GET /` to `index.html`, which means
`chess-tui --connect ws://host` (no path) breaks. v1 chess-tui
clients must switch to `--connect ws://host/ws` when talking to a
chess-net that's also serving the SPA. The `make play-local` /
`make play-lobby` flows do not enable `--static-dir`, so back-compat
is preserved unless you opt in.

Static production responses are optimized for remote users:

- `Accept-Encoding: br` / `gzip` is handled by `tower-http` runtime
  compression on the static fallback service.
- Hashed `.wasm`, `.js`, and `.css` assets get
  `Cache-Control: public, max-age=31536000, immutable`.
- `index.html` and SPA fallback responses get `Cache-Control: no-cache`
  so new deploys are discovered without forcing users to clear browser
  cache.

## Bundle size — what's in the 5 MB?

About 60% leptos + leptos_router (reactive runtime + router DSL),
20% chess-core (board/state/move generation), 10% gloo-net + futures
plumbing, 10% wasm-bindgen + serde glue. `wasm-opt -Oz` strips that
to ~800 KB. If sizes climb, a `cargo bloat --release --crates` run is
the right starting point.

## Common errors

- **`error: linking with 'cc' failed: undefined symbol: __wbindgen_*`** —
  The `wasm-bindgen` CLI version that Trunk auto-installs doesn't match
  the `wasm-bindgen` version in your `Cargo.lock`. Trunk normally fetches
  the matching CLI from the Trunk asset cache; on first run with a new
  `wasm-bindgen` version, this can take a minute. Symptoms include the
  startup hanging at `applying new distribution`. Wait it out, or
  `cargo install -f wasm-bindgen-cli --version <X.Y.Z>` matching your
  lock.
- **`Invalid route ... conflict with previously registered route`** —
  Two Trunk proxy entries register the same axum nest path. Documented
  in [`../pitfalls/trunk-proxy-route-conflict.md`](../pitfalls/trunk-proxy-route-conflict.md).
- **The SPA loads but `Disconnected` appears immediately** — chess-net
  isn't running or isn't reachable on `127.0.0.1:7878`. The browser's
  network tab will show a 502 from Trunk's proxy. Start chess-net
  (`make play-web` does both) or set `CHESS_WEB_WS_BASE` if pointing
  at a remote host.

## Why these tools

- **Leptos vs Yew**: Leptos is fine-grained reactive (no VDOM diff);
  smaller runtime and the macros feel more like SwiftUI/SolidJS than
  React. The cost is a less mature ecosystem; we hit no blockers in
  PR-1.
- **Trunk vs `wasm-pack` + `webpack` / `vite`**: Trunk is single-binary,
  watches/builds/serves with one config file, and has no node toolchain
  in the loop. The trade-off is JS-ecosystem things (PostCSS plugins,
  fancy bundling) aren't available — but a Rust-rendered SVG SPA
  doesn't need them.
- **`gloo-net` vs raw `web_sys::WebSocket`**: gloo-net wraps the browser
  WS in a real `Stream`/`Sink` so we can use `futures-util` combinators
  the same way a tokio app would. The cost is one extra crate; the
  alternative is wiring `JsValue` callbacks by hand.

## Related

- [`architecture.md`](architecture.md) — why Rust+WASM at all
- [`../CLAUDE.md`](../CLAUDE.md) "Gotchas worth knowing" — chess-web
  + chess-net interaction notes
- [`../backlog/promote-client-shared.md`](../backlog/promote-client-shared.md)
  — when to factor `orient.rs` / `glyph.rs` out of the duplicated
  client copies
- [`../backlog/web-ws-reconnect.md`](../backlog/web-ws-reconnect.md) —
  planned auto-reconnect work
- [`../backlog/web-playwright.md`](../backlog/web-playwright.md) — E2E
  smoke recipe
