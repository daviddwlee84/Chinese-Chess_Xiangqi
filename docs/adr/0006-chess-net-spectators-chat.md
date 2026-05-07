# ADR 0006 — chess-net spectators + in-room chat (protocol v3)

Status: accepted (shipped 2026-05-07)
Supersedes: nothing
Related: ADR-0004 (PlayerView projection), ADR-0005 (multi-room lobby)

## Context

After ADR-0005 the chess-net server runs multiple rooms but each one is a
hard 2-seat box: the third connection gets `Error{"room full"}` and is
dropped. There's no in-room chat either — once seated, the only client →
server messages are `Move`, `Resign`, `Rematch`, `ListRooms`. Two natural
follow-ups landed in the same PR:

1. Let third-and-beyond connections join as **read-only spectators** that
   see the board + chat but cannot move, resign, or chat.
2. Add a small **chat channel** between the seated players (and replayed to
   spectators) so opponents can talk during a game.

Three integration shapes were considered for spectator joins:

A. **Auto-fallback**: if the room is full and the client connects without a
   role flag, upgrade them to spectator silently.
B. **Explicit opt-in** via `?role=spectator`.
C. **Sniff the User-Agent** for `chess-tui/3+` and only auto-fallback then.

For chat permissions:

D. Players-only — spectators read but can't post.
E. Everyone in the room can chat (incl. spectators).
F. Players-only and spectators don't see the chat panel at all.

## Decision

**Spectator opt-in**: pick **B**. v2 clients never set `?role=spectator`,
so the existing "room full" path is preserved byte-identically. v3 clients
explicitly opt in (lobby Watch button → URL with the param). The
`User-Agent` sniff (Option C) was rejected as a magic side-channel that's
hard to reason about.

**Chat permissions**: pick **D**. Spectators see the room's chat history
(replayed via `ChatHistory` after `Spectating`) and live `Chat` pushes,
but `ClientMsg::Chat` from a spectator returns `Error{"spectators cannot
chat"}`. Streamer-style "let viewers chat too" is filed at
`backlog/chess-net-chat-moderation.md` as a P2 with a per-room toggle
sketch.

**Chat history**: `VecDeque<ChatLine>` per room, capped at 50. Late
joiners get the buffer in their welcome `ChatHistory` payload so the
conversation has context. Messages are server-stamped (`ts_ms`), trimmed,
and stripped of control chars (newline / tab → space).

**Spectator capacity**: 16 per room by default, configurable via
`--max-spectators` on `chess-net-server` (or `CHESS_NET_MAX_SPECTATORS`
env). Past the cap the joiner gets `Error{"room watch capacity reached"}`.
Bounded fan-out keeps `broadcast_to_all` cheap for chat.

**Spectator board view**: rendered from `Side::RED`'s perspective via the
existing `PlayerView::project(&state, Side::RED)`. That projection is the
only externally-visible state per ADR-0004 — proptest already enforces
no-leak for hidden cells, so banqi spectators stay correctly opaque.
Three-kingdom would need a neutral third-side projection later (out of
scope until the engine ships).

## Wire-protocol additions (v2 → v3)

`PROTOCOL_VERSION = 3`. New variants are additive — v2 clients keep
working as players because they never request a spectator role and the
new server messages only flow when triggered by v3 client actions.

```rust
ServerMsg::Spectating { protocol, rules, view }
ServerMsg::ChatHistory { lines: Vec<ChatLine> }
ServerMsg::Chat { line: ChatLine }
ClientMsg::Chat { text }

pub struct ChatLine { pub from: Side, pub text: String, pub ts_ms: u64 }
```

`RoomSummary` gains `spectators: u16` with `#[serde(default)]` so v2
lobby snapshots decode cleanly into v3 (field defaults to 0).

## Consequences

**Single ws upgrade path**: spectators and players share
`handle_room_socket` and `process_client_msg`; the role gate sits in
those two functions plus a small `Connection::{Player(Side), Spectator}`
enum. No new route, no new auth, no new dependencies.

**Lobby push includes spectator count**: `notify_lobby` now broadcasts
`spectators` in every `Rooms` snapshot, so the chess-tui lobby (`w` to
watch) and chess-web lobby (Watch button) can show the live count.

**Garbage collection respects spectators**: a non-`main` room is only GC'd
when both seats AND spectators are empty. Watching keeps a room alive
without holding a seat — important for streamer scenarios where the
players leave but the audience lingers briefly.

**Friend-lock still applies**: a `?password=`-locked room rejects the
wrong password regardless of `?role`. Spectators of locked rooms must
present the same secret as players.

**Plain-text chat**: no encryption, no moderation, no rate limiting in
v3. The room is a friend-only space by construction (locked or
shared-out-of-band link). Moderation/rate-limit primitives are sketched in
`backlog/chess-net-chat-moderation.md`; system messages
("Black has joined") in `backlog/chess-net-system-messages.md`.

## Validation

`crates/chess-net/tests/spectator_chat.rs` adds 8 server-smoke tests for:

- Spectator join with `?role=spectator` (welcome shape + ChatHistory).
- v2 back-compat: third joiner without the role param still gets `room full`.
- Players-only chat gate (`Error{"spectators cannot chat"}`).
- 50-line ring buffer cap + replay to a late spectator.
- 16-spectator default cap (overridable via `ServeOpts::with_max_spectators`).
- Lobby summary tracks spectator count.
- Spectator receives live `Update`s after a player's move.

`crates/chess-net/tests/protocol_roundtrip.rs` adds round-trip coverage
for the new ServerMsg / ClientMsg variants and the v2 → v3 RoomSummary
default path.

`make play-spectator` boots a server + 2 player panes + 1 spectator pane
for hands-on smoke testing of the chat-from-`t` UX and the read-only
spectator board.
