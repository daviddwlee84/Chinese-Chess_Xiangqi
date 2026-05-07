# chess-web WS auto-reconnect

## Why this is in backlog

PR-1 ships a single-shot WS pump (`clients/chess-web/src/ws.rs::connect`).
On disconnect or read-pump error it sets `ConnState::Closed` / `Error` and
the page surfaces a "refresh to reconnect" toast. That's deliberate: it
keeps the protocol surface and the failure modes obvious for review. But
in practice users will close laptops, switch networks, and watch their
session evaporate.

## What good looks like

A reconnect helper that:

1. Holds onto the original `(url, password)` tuple so it can rebuild the URL.
2. Uses exponential backoff (250ms, 500ms, 1s, … capped at ~10s).
3. Resets the `Hello` flow on reconnect — the client expects a fresh
   `Hello { observer, rules, view }` from the server, then `Update`s.
4. Gives up after N consecutive failures (e.g. 8) and surfaces the toast
   with a manual "retry" button.

## Notes

- chess-net's room state survives a brief disconnect because the room is
  GC'd only when the *last* seat leaves. For a single client reconnect,
  the server should just reassign the seat. **Verify** — the server may
  currently treat reconnect as a new joiner getting `room full`. If so
  this depends on a server-side reconnect-by-token handshake too (TODO.md
  has a P2 entry for chess-net spectator slots which is adjacent — both
  want some form of session token).
- `gloo-net::websocket::futures::WebSocket` doesn't expose connection
  state directly; we infer from the read-pump's `Result<Message>`. Switch
  to raw `web_sys::WebSocket` if we need `onopen` / `onclose` callbacks.
- The reconnect logic should live entirely in `ws.rs` — pages should keep
  consuming `(handle, incoming, conn)` unchanged.

## Test plan

- Two browser tabs joined to the same room. Pause the chess-net process
  for 3s, resume. Both tabs should resume without a manual reload.
- Same flow but kill chess-net entirely. Both tabs should retry with
  visible backoff, then surface the manual-retry toast.
