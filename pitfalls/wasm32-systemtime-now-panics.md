---
status: known-bug
first-hit: 2026-05-13
last-hit: 2026-05-13
---

# `std::time::SystemTime::now()` panics on wasm32 — silently kills any in-browser code path that touches it

## Symptom (verbatim)

LAN multiplayer chat: typing in the chat input + clicking Send
appears to do nothing. Chat log stays on `No messages yet.` on
BOTH host and joiner pages, despite:

* Pairing succeeding end-to-end (board renders, "You play Red",
  "Connected", host can move pieces and see updates).
* Move sync working both directions.
* Chat input being enabled (not in spectator mode).
* No JS exception bubbling up to the user.

Browser console reveals the actual cause:

```
panicked at library/std/src/sys/pal/wasm/../unsupported/time.rs:31:9:
time not implemented on this platform

at chess_net::room::now_ms()
at chess_net::room::Room::process_chat()
at chess_net::room::Room::apply()
```

The Rust panic is converted to a `RuntimeError: unreachable` inside
the WASM module. The closure that fired the panic (the click handler
chain → `handle.send(ClientMsg::Chat)` → `host.handle_self_send` →
`room.apply` → `process_chat` → `now_ms`) aborts mid-execution. No
fanout happens — neither host's local sink nor the joiner's
DataChannel ever receives the `ServerMsg::Chat`. The user sees
"chat just doesn't work".

## Root cause

`std::time::SystemTime::now()` is unimplemented on `wasm32-unknown-
unknown`. The stdlib provides a stub that immediately panics with
"time not implemented on this platform" rather than returning an
error or `Duration::ZERO`. The panic crashes whatever async/sync
context called it.

`chess_net::room::now_ms()` is called inside `process_chat` to
stamp `ChatLine::ts_ms`. Originally written for the native
chess-net server (where `SystemTime` works), the function was
inherited unchanged when `host_room.rs` started running `Room`
in-browser for LAN multiplayer (Phase 4 of `webrtc-lan-pairing`).
The native server tests never tripped the panic; the wasm32 path
trips it on the first chat message.

The error mode is silent at the UI layer because:

1. The panic occurs deep in the call stack inside a click handler.
2. `console_error_panic_hook` writes to `console.error` but the
   page UI shows nothing visible.
3. Higher-level Rust code (transport, room routing) doesn't catch
   panics — they unwind through the wasm boundary and show as
   `RuntimeError: unreachable` traces.

## Workaround

cfg-gate the time source: native uses `SystemTime`, wasm uses
`js_sys::Date::now()`:

```rust
fn now_ms() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
}
```

Cargo.toml gets a wasm32-only `js-sys` dependency:

```toml
[target.'cfg(target_arch = "wasm32")'.dependencies]
js-sys = "0.3"
```

This keeps native chess-net free of any web deps while letting
the same `Room` code path run in-browser via `host_room.rs`.

## Prevention

Any crate that compiles for both wasm32 and a native target
needs to audit `std::time::*` usage:

* `SystemTime::now()` — panics on wasm32. Use `js_sys::Date::now()`
  on wasm32, gated by `cfg(target_arch = "wasm32")`.
* `Instant::now()` — also panics on `wasm32-unknown-unknown` (in
  some toolchain versions). Safer to use `web_sys::window()
  .performance().now()` for monotonic time on wasm32.
* `Duration` arithmetic itself is fine.

The smell test: if your crate's `Cargo.toml` lists wasm32 as a
supported target (or another crate consumes it for wasm32) AND
the crate uses `std::time::SystemTime` or `Instant`, you have a
latent panic waiting on the first call site.

When debugging "an in-browser action silently does nothing":

1. **Check the browser console first**. WASM panics surface
   as `RuntimeError: unreachable` with a Rust stack trace if
   `console_error_panic_hook` is registered (which `chess-web`
   does in `lib.rs`).
2. Look for any `panicked at library/std/...` messages — those
   are stdlib bugs you've inherited, not application logic
   errors.
3. The Rust function name in the trace points directly at the
   call site.

## See also

* `crates/chess-net/src/room.rs::now_ms` — the cfg-gated fix.
* `crates/chess-net/Cargo.toml` — the wasm32-only `js-sys` dep.
* `clients/chess-web/src/host_room.rs` — the consumer that brought
  the wasm32 code path online.
* `pitfalls/leptos-rwsignal-queue-self-clear-race.md`,
  `pitfalls/leptos-create-effect-inside-spawn-local-silent-gc.md`,
  `pitfalls/webrtc-set-remote-description-resolves-before-dc-open.md`
  — the three other LAN-pairing pitfalls discovered in the same
  testing session.
* Rust issue: https://github.com/rust-lang/rust/issues/48564
  (long-standing tracker for `std::time` on wasm32).
