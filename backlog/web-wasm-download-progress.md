# chess-web: real WASM download progress meter

**Status**: deferred
**Effort**: M
**Related**: `clients/chess-web/index.html` · `docs/trunk-leptos-wasm.md`

## Context

On 2026-05-08, remote Web loading felt slow because the app had been
served through `trunk serve`, which produces a much larger debug WASM
bundle. The current shipped fix is intentionally simpler: use release
`dist/`, enable runtime compression/cache headers in `chess-net`, and
show an indeterminate pre-WASM loading shell.

This backlog item is only for a real byte-level progress meter.

## Investigation

- Trunk injects a module script equivalent to `await init({ module_or_path:
  "..._bg.wasm" })`, then dispatches `TrunkApplicationStarted`.
- That generated bootstrap does not expose per-byte progress to app code.
- A local release build measured `chess-web-*_bg.wasm` at 856 KB raw and
  about 321 KB with gzip; debug output from `trunk serve` was about 6.3 MB
  raw.
- The browser still has to download, compile, instantiate, and run
  `#[wasm_bindgen(start)]` before Leptos can mount.

## Options considered

| Option | Pros | Cons |
|---|---|---|
| Keep indeterminate loader | Low-risk, works with Trunk defaults | No real percentage |
| Custom bootstrap fetches WASM via `ReadableStream` | True download byte progress when `Content-Length` is present | More code, must preserve wasm-bindgen init contract |
| Service worker / preload observer | Could centralize asset loading later | More moving parts than this app needs today |

## Current blocker / open questions

Need to verify wasm-bindgen's generated `init` entry accepts the fetched
module/bytes cleanly across Trunk upgrades, and whether compressed transfer
responses expose useful `Content-Length` in target deployment environments.

## Decision

2026-05-08 deferred. The current release bundle plus gzip/br compression is
small enough that an indeterminate loader is the right default. Revisit only
if field measurements show first-load latency remains confusing after using
`make serve-web-prod`.

## References

- `docs/trunk-leptos-wasm.md`
- `clients/chess-web/index.html`
