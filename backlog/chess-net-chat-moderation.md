# chess-net chat moderation primitives

## Why this is in backlog

ADR-0006 ships in-room chat as a friend-only channel: 50-line ring
buffer, players-only, no encryption, no rate limiting, no moderation. For
public rooms (a community server, a streamer letting spectators chat) the
absence of even the simplest controls becomes a real problem — one user
spamming a thousand `ClientMsg::Chat` frames per second can DoS the
broadcast loop, and there's no kick / mute primitive.

This backlog item collects the minimum viable moderation surface so a
public deployment doesn't need a hand-patched fork.

## What good looks like

1. **Per-player rate limit** at the server. Naive: a leaky bucket of
   ~1 msg/sec/player with a small burst (5). Drop excess with
   `Error{"chat rate limited"}`. Fits in `process_chat` next to the
   length validation.
2. **Mute / unmute by side**: `ClientMsg::ChatMute { side }` from a
   player muffles inbound chat from the opposite seat for that
   *connection only* (no server-side enforcement, just a "don't
   broadcast to me" filter). Survives the connection; resets on
   disconnect. Spectators get a separate `ChatMuteAll` toggle.
3. **Server-side word filter** (opt-in via `--chat-filter <path>`):
   plain-text list of substrings; matches get replaced with `***` before
   the line lands in the ring buffer. Nothing fancy — regex / unicode
   normalization is out of scope for v1.
4. **Op-room kick**: a server-config-only "operators" list (env var of
   side+room or just a static `--op` flag). An op connection gets a new
   `ClientMsg::Kick { side }` that drops the target's seat. No UI in the
   TUI for v1 — just a documented wscat flow.

## Notes

- Rate limiting is the cheapest win and the only one that prevents
  resource abuse, so it should ship first even if the others lag.
- `ChatMute` is purely client-side display gating; no need to bump
  `PROTOCOL_VERSION` for it (server-side it's a no-op).
- The word filter is the most controversial: opt-in by default, never
  enabled for `main` (the back-compat room).
- A real "moderator role" with persistent state requires user accounts,
  which is well outside v3 scope. Keep this entry pinned at "primitives,
  not policies".

## Test plan

- Drive 100 chat messages/sec from one client; assert that ≥95 of them
  bounce with the rate-limit error and the broadcast loop stays
  responsive (other clients still see Updates promptly).
- Mute toggle: client A mutes; verify A's read pump drops `Chat` frames
  from the muted side. Confirm the underlying `Chat` payload is
  unchanged for other recipients.
- Word filter: `--chat-filter` with a 3-word list; assert lines
  containing any token render as `***` in `ChatHistory` for late
  joiners.

## Related

- `crates/chess-net/src/server.rs::process_chat` — drop point for the
  rate limit + word filter.
- `crates/chess-net/src/protocol.rs` — `ClientMsg::ChatMute / Kick` would
  be additive; bump `PROTOCOL_VERSION` only for `Kick` (rare).
- `docs/adr/0006-chess-net-spectators-chat.md` — the parent ADR that
  explicitly punted moderation to this entry.
