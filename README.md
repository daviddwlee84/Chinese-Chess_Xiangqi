# Chinese Chess

A hobby Chinese chess game in Rust supporting:

- **Standard Xiangqi (象棋)** — 9×10, two players
- **Banqi (暗棋)** — 4×8 dark chess, rank-based capture
- **Three Kingdoms Banqi (三國暗棋)** — banqi variant with three teams *(planned)*
- **House rules** — toggleable: 連吃, 暗連, 車衝, 馬斜, 炮快移
- **Large board variants (大盤)** — n×m via `BoardShape::Custom` *(planned)*

## Goals

- Modular core (`chess-core`) reusable across native TUI and browser (WASM)
- Self-hostable server for play with friends
- Mouse + keyboard UX (Vim-style + coordinate input)

## Layout

```
crates/chess-core    — pure game logic (this is where everything starts)
crates/chess-engine  — search/eval (planned)
crates/chess-net     — websocket protocol (planned)
crates/chess-ai      — heuristic + ISMCTS for banqi (planned)
clients/chess-cli    — REPL test harness for chess-core
clients/chess-tui    — ratatui frontend (planned)
clients/chess-web    — Leptos + WASM frontend (planned)
```

See [`docs/architecture.md`](docs/architecture.md) for the full design.

## Build

```bash
cargo check --workspace
cargo test -p chess-core
cargo run -p chess-cli
```

WASM cleanliness check (proves `chess-core` has no platform deps):

```bash
cargo build --target wasm32-unknown-unknown -p chess-core
```

## License

MIT OR Apache-2.0
