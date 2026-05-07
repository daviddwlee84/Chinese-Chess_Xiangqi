# Architecture Decision Record (ADR)

| # | Title | Status |
|---|---|---|
| [0001](0001-rust-workspace-layout.md) | Rust workspace layout | accepted |
| [0002](0002-square-as-u16.md) | `Square(u16)` linear index | accepted |
| [0003](0003-ruleset-as-data-not-trait.md) | `RuleSet` as plain data, not a trait | accepted |
| [0004](0004-player-view-projection.md) | `PlayerView` projection as the network ABI | accepted |
| [0005](0005-multi-room-lobby.md) | Multi-room chess-net + lobby (protocol v2) | accepted |
| [0006](0006-chess-net-spectators-chat.md) | Spectators + in-room chat (protocol v3) | accepted |
| [0007](0007-spectator-check-flag.md) | `PlayerView.in_check` flag (protocol v4) | accepted |
| [0008](0008-engine-chain-lock.md) | Engine-driven 連吃 chain mode (protocol v5) | accepted |

When a new design decision is made, add a row above and create the
matching `0NNN-<slug>.md`. ADR files are append-only — once accepted
they describe the world at that point in time. Subsequent reversals
get a new ADR that supersedes the older one.
