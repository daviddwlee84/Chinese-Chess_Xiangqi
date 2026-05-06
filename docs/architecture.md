# Architecture

Distilled from the technology selection round. See [`adr/`](adr/) for individual decisions.

## Goals

- Hobby Chinese chess game supporting standard xiangqi (象棋), banqi (暗棋), 三國暗棋, and 大盤 variants.
- Toggleable banqi house rules: 連吃, 暗連, 車衝, 馬斜, 炮快移.
- Self-hostable server for friends; native TUI and Web clients sharing the same engine.
- Mouse + keyboard UX (Vim-style + coordinate input).

## Tech Selection

| Option | TUI | Web | Multiplayer | Core reuse | Verdict |
|---|---|---|---|---|---|
| **Rust + WASM** | ratatui | Yew · Leptos · Dioxus (same core via WASM) | tokio + axum/ws | ★★★★★ | **Chosen** |
| Go (Charm) | bubbletea + lipgloss | templ + htmx (extra effort) | wish (SSH) + ws | ★★★★ | Strong "for fun" alt |
| Python (Textual) | Textual (mouse native) | `textual serve` (terminal-in-browser) | FastAPI + ws | ★★★★ | Fastest to ship |
| TypeScript | Ink (React) | any framework | ws | ★★★★ | OK |
| Unity | hard | WebGL | Mirror | ★ | Over-engineered |

**Why Rust + WASM**: variant explosion (xiangqi vs banqi vs 三國暗棋 vs 大盤 vs N house-rule combinations) is a type-system problem. Rust enums + bitflags express it cleanly without runtime cost. Same `chess-core` crate compiles to native (TUI / server) and `wasm32-unknown-unknown` (browser), so there's exactly one source of truth for rules. Single-binary distribution. Unity-as-renderer remains open via FFI later.

## Workspace Layout

```
chinese-chess/
├── crates/
│   ├── chess-core    # pure logic — board, pieces, rules, state, view
│   ├── chess-engine  # search/eval (planned)
│   ├── chess-net     # wire protocol over chess-core (planned)
│   └── chess-ai      # heuristic + ISMCTS for banqi (planned)
├── clients/
│   ├── chess-cli     # REPL test harness — proves engine end-to-end
│   ├── chess-tui     # ratatui frontend (planned)
│   └── chess-web     # Leptos + WASM frontend (planned)
└── xtask/            # project automation
```

`chess-core` has no IO, no rendering, no platform deps. WASM cleanliness is enforced in CI.

## Key Design Decisions

The five locked-in decisions, each with its own ADR:

1. **`Square(u16)` linear index, not `(file, rank)` tuples.** Scales to 19×19, supports irregular topologies via per-shape mask. ([ADR-0002](adr/0002-square-as-u16.md))
2. **`Move` is a flat enum.** Variants: `Reveal { revealed: Option<Piece> }`, `Step`, `Capture`, `ChainCapture { path: SmallVec<[ChainHop; 4]> }`, `CannonJump`. The `Option<Piece>` on `Reveal` is the network ABI boundary — opponents never see the identity ahead of the flip. ([ADR-0004](adr/0004-player-view-projection.md))
3. **`RuleSet` is plain data, not a trait.** House rules are `bitflags::bitflags!`, presets are named consts. Move generation is free functions dispatching on `Variant` + flag checks. Rejected: trait-object rule layering — kills inlining, fights serde, over-engineers a closed set. ([ADR-0003](adr/0003-ruleset-as-data-not-trait.md))
4. **`GameState` is one concrete struct, not generic.** `TurnOrder` supports 2 or 3 seats (`SmallVec<[Side; 3]>`) so 三國暗棋 isn't a special case in code paths.
5. **`PlayerView::project(&GameState, observer)` is the only externally-visible state.** Hidden pieces map to `VisibleCell::Hidden` with no identity. A proptest enforces no-leak.

## Extensibility Path

| Want to add… | Touch… | Effort |
|---|---|---|
| New variant (e.g. 廣象戲) | `BoardShape::Custom`, `Variant::*`, `rules/<name>.rs`, `setup.rs` | M |
| New house rule | `HouseRules` bitflag + a free function in `rules/house.rs` | S |
| New piece kind | `PieceKind` variant, movement generator, banqi rank | S |
| 3-player turn-skip rules | `TurnOrder::advance_skipping` already exists | S |
| Different render (TUI / Web / Unity) | new client crate consuming `PlayerView` | M |

## Roadmap

| PR | Item |
|---|---|
| 1 (this) | Workspace + `chess-core` foundations + xiangqi + banqi + CHAIN/RUSH + CLI harness |
| 2 | 三國暗棋 board mask + rules; remaining house rules (DARK_CHAIN, HORSE_DIAGONAL, CANNON_FAST_MOVE) |
| 3 | `chess-tui` (ratatui, vim + mouse) |
| 4 | `chess-net` (tokio + ws, server-authoritative, ships PlayerView only) |
| 5 | `chess-web` (Leptos + WASM consuming PlayerView) |
| 6 | `chess-engine` + `chess-ai` (alpha-beta + Zobrist; ISMCTS) |
| 7 | 大盤 variants |
