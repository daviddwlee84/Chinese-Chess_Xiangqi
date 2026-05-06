# chess-net: server-authoritative websocket protocol

**Status**: shipped (2026-05-06, MVP — single room, no lobby/reconnect/time controls)
**Effort**: M (matched estimate)
**Related**: `TODO.md` · `crates/chess-net/src/{protocol,server}.rs` · `crates/chess-net/src/bin/server.rs` · `clients/chess-tui/src/net.rs` · `Makefile` + `scripts/play-local.sh`

## What shipped

- `axum 0.7` + `tokio` server on `127.0.0.1:<port>`. Single room; first 2 ws clients seat as Red / Black, third gets `Error{"room full"}`. JSON over text frames, tagged enums (`{"type": ...}`).
- `ServerMsg::{Hello{protocol, observer, rules, view}, Update{view}, Error{message}}` and `ClientMsg::{Move{mv}, Resign}`. `Move::Reveal { revealed: None }` stays None on the wire — server fills `Some(piece)` only inside its local `GameState`.
- `chess-tui --connect ws://host:port` runs a sync `tungstenite` worker on a background OS thread; talks to the TUI via `std::sync::mpsc`. No tokio in the TUI binary. `--as` flag is overridden by the server's seat assignment.
- `make play-local` / `scripts/play-local.sh` boots one server + two clients in a single tmux session for fast manual smoke.
- 8 new tests: 6 protocol roundtrip (covers `Hello`/`Update`/`Error`/`Move(Step)`/`Move(Reveal None)`/`Resign`) + 2 end-to-end smoke (two clients exchange a real move; third connect gets "room full"). All pass under `cargo test -p chess-net`.

## Out of scope (separate TODO items)

- Lobby / matchmaking / multiple concurrent games.
- Reconnect (transient disconnect today drops the player; opponent gets `Error{"opponent disconnected"}`).
- Time controls (TODO entry retained; `WinReason::Timeout` already exists).
- Takeback / 悔棋 (TODO entry retained; needs opponent-approval protocol).
- TLS / wss / auth.
- chess-web frontend (separate TODO).

## Context

Why this surfaced. Trigger (conversation, bug, new tool, recurring annoyance).
Date helps — "2026-04, came up while reviewing X".

## Investigation

What's already been tried / read / measured. **This is the section that saves
future-you time.** Be specific:

- Commands run + relevant output
- Docs/issue/SO links read
- Benchmark numbers
- Error messages copy-pasted in full (not paraphrased)

## Options considered

| Option | Pros | Cons |
|---|---|---|
| A. … | … | … |
| B. … | … | … |

## Current blocker / open questions

Why this is still on the backlog. One of:

- Waiting on upstream X (link)
- Need host/data Y to verify
- Trade-offs unclear, need user preference
- Effort estimate exceeds current budget

## Decision (if any)

`2026-04 deferred — waiting for X release` or
`2026-04 chose option B because …`

Dates matter. A 6-month-old "decided to use mise" needs re-validation.

## References

Issues, PRs, SO links, related discussions.
