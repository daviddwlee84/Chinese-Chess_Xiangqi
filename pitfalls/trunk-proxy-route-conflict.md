# `trunk serve` panics: Invalid route "/ws/{*__private__axum_nest_tail_param}" — conflict with previously registered route

**Symptoms** (grep this section): `trunk-rt-worker` panic, `Invalid route`,
`__private__axum_nest_tail_param`, `conflict with previously registered route`,
silent tmux pane close after `make play-web`.

**First seen**: 2026-05
**Affects**: `trunk 0.21.x`, any `Trunk.toml` that registers two `[[proxy]]`
entries whose `rewrite` paths collide as axum nest prefixes.
**Status**: workaround documented (consolidate to a single `/ws` proxy)

## Symptom

`make play-web` opens a tmux session with two windows: `server` (chess-net)
and `web` (trunk serve). The `web` window flashes briefly then closes — the
pane was running `trunk serve`, which panicked, returned non-zero, and tmux
auto-killed the dead pane. No log makes it to the tmux scrollback.

Running `trunk serve` directly from `clients/chess-web/` reveals the panic:

```
2026-05-07T04:07:41.398252Z  INFO 📡 serving static assets at -> /
2026-05-07T04:07:41.398268Z  INFO 📡 proxying websocket /ws/ -> ws://127.0.0.1:7878/ws/
2026-05-07T04:07:41.398284Z  INFO 📡 proxying websocket /ws -> ws://127.0.0.1:7878/ws

thread 'tokio-rt-worker' panicked at .../trunk-0.21.14/src/proxy.rs:269:16:
Invalid route "/ws/{*__private__axum_nest_tail_param}":
Insertion failed due to conflict with previously registered route:
/ws/{*__private__axum_nest_tail_param}
```

The `static asset` and first two `proxying` lines print successfully — the
panic happens when axum's router tries to commit the second `/ws*` proxy
entry.

## Root cause

Trunk 0.21 wraps each `[[proxy]]` entry as an axum nested router via
`Router::nest()`. Internally, axum represents a nested route as a path
matcher of the form `<prefix>/{*__private__axum_nest_tail_param}`. Both
of the following entries collapse to the same matcher:

```toml
[[proxy]]
backend = "ws://127.0.0.1:7878/ws/"
ws      = true
rewrite = "/ws/"

[[proxy]]
backend = "ws://127.0.0.1:7878/ws"
ws      = true
rewrite = "/ws"
```

axum requires distinct nest prefixes; registering two that compile to
the same wildcard route triggers an `InsertError::Conflict` panic at
`Router::nest()`.

The original intent was to forward both `/ws` (the chess-net default-room
upgrade for v1 clients) and `/ws/<room>` (named room) — splitting them
into two entries seemed natural but is unnecessary because Trunk's
proxy already matches as a prefix.

## Workaround

Use a **single** proxy entry rooted at `/ws`. It catches both the bare
path and any sub-path:

```toml
# Trunk.toml
[[proxy]]
backend = "ws://127.0.0.1:7878/ws"
ws      = true
rewrite = "/ws"
```

This forwards `/ws` to `ws://127.0.0.1:7878/ws` and any `/ws/<room>` to
`ws://127.0.0.1:7878/ws/<room>` (Trunk preserves the trailing path
verbatim).

## Prevention

- One proxy entry per nest prefix. If you need two distinct backends
  for `/ws` vs. `/ws/`, route them through different prefixes
  (e.g. `/ws-default` and `/ws-room`) or proxy at a higher level
  (`/` → upstream, no Trunk-side splitting).
- When `make play-web` exits silently, run `trunk serve` directly from
  `clients/chess-web/` to see the actual error. The tmux pane in
  `scripts/play-web.sh` runs the command without a `--remain-on-exit`
  shim, so panics get swallowed by the auto-kill.

## Related

- [`../docs/trunk-leptos-wasm.md`](../docs/trunk-leptos-wasm.md) — Trunk
  config narrative and the working `[[proxy]]` block
- [`../scripts/play-web.sh`](../scripts/play-web.sh) — could be hardened
  with `tmux set-option remain-on-exit on` so panics stay visible
  (deferred — the doc + this pitfall is enough breadcrumb for now)
- Upstream: trunk-rs/trunk does not currently surface a clearer error
  for this case; consider filing if it recurs across users
