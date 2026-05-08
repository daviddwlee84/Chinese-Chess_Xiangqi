# Chinese Chess

A hobby Chinese chess game in Rust supporting:

- **Standard Xiangqi (象棋)** — 9×10, two players
- **Banqi (暗棋)** — 4×8 dark chess, rank-based capture
- **Three Kingdoms Banqi (三國暗棋)** — banqi variant with three teams *(planned)*
- **House rules** — toggleable: 連吃, 暗連, 車衝, 馬斜, 炮快移

## Goals

- Modular core (`chess-core`) reusable across native TUI and browser (WASM)
- Self-hostable server for play with friends
- Mouse + keyboard UX (Vim-style + coordinate input)

## Layout

```
crates/chess-core    — pure game logic (this is where everything starts)
crates/chess-engine  — search/eval (planned)
crates/chess-net     — websocket protocol + self-hostable server
crates/chess-ai      — heuristic + ISMCTS for banqi (planned)
clients/chess-cli    — REPL test harness for chess-core
clients/chess-tui    — ratatui frontend (xiangqi + banqi, vim+mouse, CJK glyphs)
clients/chess-web    — Leptos + WASM frontend for local and online play
```

See [`docs/architecture.md`](docs/architecture.md) for the full design.

## Build

```bash
cargo check --workspace
cargo test -p chess-core
cargo run -p chess-cli       # line-oriented REPL
cargo run -p chess-tui       # interactive TUI (CJK glyphs, vim + mouse)
```

Useful TUI flags: `--style ascii` (letter glyphs), `--no-color`, `--as black`
(render from Black's perspective for testing the orientation flip).

WASM cleanliness check (proves `chess-core` has no platform deps):

```bash
cargo build --target wasm32-unknown-unknown -p chess-core
```

Web frontend:

```bash
make play-web                         # local dev: chess-net + trunk serve
make serve-web-prod ADDR=0.0.0.0:7878 # release WASM + compressed static serve
make build-web-static                 # GitHub Pages artifact under clients/chess-web/dist-static
```

Use `play-web` only for local development. Remote users should hit the
production path so they download the release WASM bundle, not Trunk's
debug/dev-server output.

The GitHub Pages build is local-play ready. Online play from Pages needs a
separate chess-net server URL entered in the Web lobby (`wss://...` on HTTPS).

## License

MIT OR Apache-2.0

## Roadmap & lessons learned

Forward-looking work — long-term ideas, deferred items, things needing
evaluation — lives in [`TODO.md`](TODO.md), prioritised P1 → P3 with effort
estimates (S/M/L/XL). Items with accompanying research, design notes, or paused
troubleshooting link to a corresponding [`backlog/<slug>.md`](backlog/) doc.

Backward-looking knowledge — past traps and non-obvious debugging — lives in
[`pitfalls/`](pitfalls/), titled by symptom so future-you can grep the error
message and land on the root cause + workaround instead of re-debugging from
scratch.

## Resources

- [Free Online Chinese Chess | Play Xiangqi](https://www.xiangqi.com/)
