# Leptos `<Router base=…>` renders blank `<main>` at the base URL (trailing-slash sensitive)

**Symptoms** (grep this section):

- `cd dist-static && python3 -m http.server` (or GitHub Pages) serves
  `index.html` at the project root URL but the SPA mounts an *empty*
  `<main class="app-shell"><!----><!----></main>` — no picker, no error,
  no panic in the JS console.
- Playwright `wait-for-selector` times out: `TimeoutError: Timeout 60000ms
  exceeded. waiting for locator('text=Chinese Chess 中國象棋') to be visible`.
- Direct deep links like `/Chinese-Chess_Xiangqi/local/xiangqi` *do* render
  the right page (when the host serves the SPA shell as a 404 fallback) but
  the bare `/Chinese-Chess_Xiangqi/` lands on a white screen.
- `cargo test -p chess-web` is green; the bug is wasm-only.

**First seen**: 2026-05
**Affects**: `leptos = "0.6"`, `leptos_router = "0.6"` (verified on 0.6.15),
GitHub Pages-style project subpath hosting (`/<repo>/`)
**Status**: workaround documented (low-priority `<Route path="/*any" view=Picker/>`
fallback in `Routes`)

## Symptom

Build the Web client for a non-root `--public-url`:

```sh
make build-web-static WEB_BASE=/Chinese-Chess_Xiangqi
# stages dist-static/{index.html,404.html,…} with <base href="/Chinese-Chess_Xiangqi/">
```

Serve it under that subpath (mimicking GitHub Pages) and visit the root:

```sh
python3 -m http.server --directory /tmp/pages-root 8899
# http://127.0.0.1:8899/Chinese-Chess_Xiangqi/
```

The HTML loads, wasm boots, but the rendered DOM is:

```html
<main class="app-shell"><!----><!----><!----></main>
```

No JS error. `location.pathname` is `/Chinese-Chess_Xiangqi/`. The router
context exists; it just doesn't match any route.

## Root cause

`leptos_router` 0.6's `Matcher::test` in
[`matching/matcher.rs`](https://github.com/leptos-rs/leptos/blob/v0.6.15/router/src/matching/matcher.rs)
uses `get_segments(...)` which is **trailing-slash sensitive** for non-root
paths:

```rust
// non-root paths with trailing slashes get extra empty segment at the end.
// This makes sure that segment matching is trailing-slash sensitive.
if !segments.is_empty() && pattern.ends_with('/') {
    segments.push("".into());
}
```

When `<Router base="/Chinese-Chess_Xiangqi">` is set, the router strips the
base from `location.pathname` before matching. Pages serves the directory
URL as `/Chinese-Chess_Xiangqi/` (with trailing slash), so the *post-strip*
path passed to the matcher is `/` — but Leptos 0.6 stores it after
`base_path.unwrap_or_default()` joining as the **empty string**, which has
zero segments and matches `<Route path="">` only. `<Route path="/" …>` has
zero segments too, but the matcher's "extra trailing-slash segment" rule
makes the *location's* trailing slash non-zero in some browser histories
and the comparison fails. Net effect: at the project root URL, no route
matches and `<Routes>` renders nothing.

This is *not* the same trap as the one fixed in trailing-slash handling
PRs against `leptos_router` 0.7 — 0.6 has no `TrailingSlash::Redirect`
escape hatch wired through `<Router>`'s public API.

## Workaround

Add a low-priority wildcard fallback to `<Routes>` that targets the same
view as `/`:

```rust
// clients/chess-web/src/app.rs
view! {
    <main class="app-shell">
        <Routes base=base_path().to_string()>
            <Route path="/" view=Picker/>
            <Route path="/local/:variant" view=LocalPage/>
            <Route path="/lobby" view=LobbyPage/>
            <Route path="/play/:room" view=PlayPage/>
            <Route path="/*any" view=Picker/>   // ← catches the empty-segment case
        </Routes>
    </main>
}
```

`/*any` is a splat route; the matcher's "more-specific routes win" pass
keeps `/local/:variant`, `/lobby`, `/play/:room`, and `/` taking priority,
so the only URLs that hit `Picker` via the splat are exactly the ones
`Routes` would otherwise render blank — including the post-base-strip empty
path at the project root.

Verify with:

```sh
make build-web-static WEB_BASE=/Chinese-Chess_Xiangqi
# pages-mimicking server: serve 404.html as the in-tree SPA shell so
# deep links like /local/xiangqi also boot the SPA (status 404, but the
# wasm router takes over client-side).
```

## Prevention

- When you add a new top-level route, **don't remove `/*any`** — it's
  load-bearing for the project-subpath GitHub Pages deploy, not just a
  cosmetic "not found" handler.
- If you ever upgrade to `leptos_router` 0.7+, re-test the bare project
  root URL (`/<repo>/`) before deleting the splat fallback. The 0.7
  trailing-slash story is different and the workaround may no longer be
  needed (or may need a different shape).
- Native `cargo test -p chess-web` will *not* catch this — the router is
  wasm32-gated. Always smoke-test the static build with a real browser
  against the **bare base URL** before tagging a release.

## Related

- `clients/chess-web/src/app.rs` — the `<Routes>` definition
- `clients/chess-web/src/routes.rs` — `base_path()` helper that wires the
  `CHESS_WEB_BASE_PATH` env var into the router and `<a href>` builder
- `Makefile` — `build-web-static` target
- `.github/workflows/pages.yml` — deploy pipeline
- Upstream matcher source:
  [`leptos_router-0.6.15/src/matching/matcher.rs`](https://github.com/leptos-rs/leptos/blob/v0.6.15/router/src/matching/matcher.rs)
- Sibling pitfall:
  [`trunk-proxy-route-conflict.md`](trunk-proxy-route-conflict.md) — the
  `/ws` vs `/ws/` axum route-conflict pitfall, also rooted in
  trailing-slash handling but in a totally different layer.
