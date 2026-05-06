# ADR 0005 — Multi-room lobby + optional password

Status: accepted (shipped 2026-05-06)
Supersedes: nothing
Related: ADR-0004 (PlayerView projection — still the only visible state)

## Context

The chess-net MVP shipped a single hard-coded room: two ws clients connect
to `ws://host:7878`, get seated Red/Black, and play. To find each other,
players had to coordinate the host:port out of band ("Slack the IP, count
seats by hand"). Three integration paths surfaced when designing the next
step:

A. Extend `chess-tui` with an in-process lobby browser (`Screen::Lobby`).
B. Ship a separate `chess-tui-lobby` binary or subcommand.
C. Replace/parallel the transport with `russh` so users do
   `ssh chess.you.dev`.

## Decision

Pick **A**. Multi-room is implemented inside the existing chess-net axum
server with a new `/lobby` ws subscription channel, and `chess-tui` grows
three new screens (`HostPrompt`, `Lobby`, `CreateRoom`) plus `--lobby` /
`--password` flags. The first joiner of a `/ws/<id>?password=<pw>` URL
locks the room with that password; later joiners must present the same
secret or get `Error{"bad password"}` and are dropped pre-Hello.

## Consequences

**Reused stack**: no new dependencies in chess-tui (no `reqwest`, no
`tui-input`, no `russh`). The lobby connection is a second instance of
the existing sync `tungstenite` worker thread (`net.rs`). Server-side it's
two more axum routes plus a `Mutex<HashMap<String, Arc<Mutex<RoomState>>>>`
table.

**Live updates**: the `Rooms` push goes out on every seat insertion / seat
removal / status flip / explicit `ListRooms` request. Lobby viewers see
new rooms appear and fill in real time without polling.

**Backwards compat preserved**: `ws://host` and `ws://host/ws` still land
in a default room called `main`, which is permanent (never GC'd) so v1
clients always have a stable target. The new wire variants
(`ServerMsg::Rooms`, `ClientMsg::ListRooms`) are additive — old payload
shapes are byte-identical.

**Friend-lock not security**: `?password=` is plain-text, stored in
memory, transmitted over plaintext ws. This is *not* an auth boundary;
it's the equivalent of a numeric room PIN to keep strangers out of your
home game. Real auth + WSS belongs in a separate PR.

**Mixed-variant servers deferred**: a server boots with one `--variant`
that's used for every auto-created room. Per-room `RuleSet` is doable
(`RoomState` already owns its `GameState`) but adds UX questions about
filter / sort in the lobby that we punted for now.

## Alternatives weighed

**REST `GET /rooms` polling instead of `/lobby` push**: simpler protocol,
but the chess-tui worker is shaped for streaming `ServerMsg`/`ClientMsg`
JSON. Adding HTTP would have meant either pulling in `reqwest` or
hand-rolling HTTP/1.1 over `TcpStream`. The push approach reuses the
existing transport with two new enum variants. The `/rooms` endpoint
exists anyway as a small `axum::Json` handler for `curl`-driven debugging.

**Separate `chess-tui-lobby` binary (Option B)**: rejected. The whole
point of the lobby is to remove the URL-copy-paste step. A second binary
re-introduces the gap (browse rooms in one process, copy id, type
`--connect ws://host/ws/<id>` in another).

**`russh` SSH server (Option C)**: rejected. ~1500 LOC of glue, locks the
canonical transport to SSH, conflicts with the chess-web roadmap (P2 in
`TODO.md`). Filed as a P? spike for later (`Wish/SSH multiplayer
alternative to chess-net`); not scope for this PR.

**`DashMap` instead of nested `tokio::sync::Mutex<HashMap<...>>`**: same
runtime cost in practice; the nested Mutex pattern is one fewer dep and
the lock ordering (outer → inner) is trivial to reason about. Outer lock
is held only for HashMap insert/remove/iterate-to-snapshot, never across
`await` or `tx.send`.

## Validation

Eight new server-smoke tests cover the lobby push semantics, multi-room
isolation, password accept/reject, and the `main`-room back-compat path.
Two new protocol round-trip tests cover the `Rooms` / `ListRooms` /
`RoomSummary` shapes. Manual smoke is `make play-lobby` (one server +
three TUI panes: two go through the picker → host-prompt → lobby flow,
one watches via `--lobby`).
