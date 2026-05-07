# chess-net system messages in chat (player joined / left / etc.)

## Why this is in backlog

`ChatLine.from` is `Side` today, which means every chat line shows a
specific player's color. Real chat UIs need a "system" sender for events
like "Black has joined", "Spectator joined (5 watching)", "Game starts in
3s", "Red resigned". ADR-0006 explicitly punted this for v3 — chat is
strictly player-authored — but the absence shows up the moment two
strangers play: there's no in-channel signal of who's in the room.

## What good looks like

1. Replace `ChatLine.from: Side` with an enum:

   ```rust
   pub enum ChatFrom {
       Player(Side),
       System,        // server-generated narration
   }
   pub struct ChatLine { from: ChatFrom, text: String, ts_ms: u64 }
   ```

2. Server emits `Chat { line: ChatLine { from: ChatFrom::System, text, ts_ms } }`
   for: seat join/leave, spectator join/leave, rematch reset, resign,
   game-end summary.
3. Clients render system lines in a muted color (e.g. `--muted` /
   `Color::DarkGray`) without the "Red:"/"Black:" prefix.
4. System lines also live in the 50-line ring buffer so late joiners get
   the recent narration.

## Notes

- This is a wire-breaking change to `ChatLine` (different shape), so it
  bumps `PROTOCOL_VERSION` to 4 and likely needs a `serde(untagged)` or
  custom `Deserialize` to decode the v3 `from: Side` shape into
  `ChatFrom::Player(side)`.
- Volume control: a busy room could spam system lines (every spectator
  arrival/departure pushes the player log around). Mitigations: collapse
  consecutive joins ("3 spectators joined") or only narrate seat changes,
  not spectators.
- Internationalization is on the table: the `text` field could become
  `kind: SystemEventKind` and the client formats locally. Out of scope
  for v1; English string is fine.
- Lichess and chess.com both render system lines distinctively from
  player chat — match that pattern.

## Test plan

- A room with 0 spectators → first spectator joins → `ChatHistory` for
  the next late spectator includes the join system line.
- v3 client connecting to a v4 server (back-compat): server keeps
  emitting a v3-shaped `from: Side` for the system line by aliasing
  System → some sentinel side OR by skipping system lines entirely on the
  v3 path. Document the chosen behavior in the ADR.
- Render tests: chess-tui `format_chat_line` and chess-web `<ChatLineView>`
  both special-case System with a different glyph/color.

## Related

- `crates/chess-net/src/protocol.rs::ChatLine` — type to extend.
- `crates/chess-net/src/server.rs::process_chat` / `broadcast_to_all`
  — emission points (also seat insert/remove + rematch in
  `handle_room_socket`).
- `clients/chess-tui/src/ui.rs::format_chat_line`,
  `clients/chess-web/src/components/chat_panel.rs::ChatLineView` — render
  sites.
- `docs/adr/0006-chess-net-spectators-chat.md` — the ADR that punted
  this.
