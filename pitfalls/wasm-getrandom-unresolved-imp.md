# `chess-core` fails to build for `wasm32-unknown-unknown`: getrandom `unresolved module imp`

**Symptom**

```
$ cargo build --target wasm32-unknown-unknown -p chess-core
...
error[E0433]: failed to resolve: use of unresolved module or unlinked crate `imp`
   --> .../getrandom-0.2.17/src/lib.rs:402:9
    |
402 |         imp::getrandom_inner(dest)?;
    |         ^^^ use of unresolved module or unlinked crate `imp`
error: could not compile `getrandom`
```

Triggered when adding `rand` (transitively pulling in `getrandom 0.2`) to a workspace that builds for `wasm32-unknown-unknown` (the bare WASM target with no host backend, used by browser frameworks like Leptos / Yew).

## Root cause

`getrandom 0.2` ships several backends (`linux`, `darwin`, `windows`, `wasi`, …). For `wasm32-unknown-unknown` it has no default backend — that target is "unknown" by definition. The user must opt into a backend by enabling a feature on `getrandom` directly:

- `js` — bridges to Web Crypto API via `wasm-bindgen` (browser only)
- `custom` — caller provides a `register_custom_getrandom!` shim (manual)

Since `getrandom` is a transitive dep, you can't enable the feature on it from the using crate's `[dependencies]` block — Cargo features merge across the dep graph but only when the feature is named directly. You must declare `getrandom` as a direct dep with the desired feature.

## Fix

In `crates/chess-core/Cargo.toml`, add a target-specific dep:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
getrandom = { version = "0.2", features = ["js"] }
```

This is target-conditional, so native builds aren't affected. The browser WASM build now compiles cleanly.

## Why it surfaced late

Pure `cargo test` runs everything for the host target, where `getrandom` works fine. The WASM cleanliness check (`cargo build --target wasm32-unknown-unknown -p chess-core`) was the only step that exercised this path — added to the CI workflow for exactly this reason.

## Caveat

The `js` feature pulls in `wasm-bindgen` and `js-sys`, which means `chess-core` is no longer "completely WASM-clean" in the WASI/no-host sense. It's clean for the **browser** WASM target, which is what we actually want for `chess-web`. If a future contributor wants `chess-core` runnable in pure WASI / Cloudflare Workers, this needs revisiting (probably by pushing the randomness boundary out of `chess-core` and into the caller).

## Related

- TODO.md `[M] Threefold repetition draw detection` — when adding Zobrist, prefer a deterministic seed over `OsRng` so we don't deepen the WASM-host coupling.
- `docs/architecture.md` "WASM cleanliness" goal.
