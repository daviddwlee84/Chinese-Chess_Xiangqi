# Three Kingdoms Banqi (三國暗棋) implementation

**Status**: P1
**Effort**: M
**Related**: `TODO.md` · `crates/chess-core/src/board/shape.rs` (BoardShape::ThreeKingdom variant) · `crates/chess-core/src/rules/three_kingdom.rs` (stub) · `docs/rules/three-kingdoms.md` (skeleton) · `crates/chess-core/src/state/turn_order.rs` (TurnOrder::three_player + advance_skipping)

## Context

User listed 三國暗棋 as one of the four target variants alongside xiangqi, banqi, and 大盤. The reference URL the user shared (`zh.wikipedia.org/zh-sg/三國暗棋`) confirms this is a banqi-family three-player variant on a half-board, NOT a three-player xiangqi.

PR 1 deferred the actual rules implementation but built the architecture to support it without rework:

- `Side(u8)` accepts 0/1/2 (not a fixed enum)
- `TurnOrder` is `SmallVec<[Side; 3]>` with `two_player()` and `three_player()` constructors plus `advance_skipping(eliminated: &[Side])` for when one faction is wiped out
- `BoardShape::ThreeKingdom` exists in the enum (currently a stub that builds an empty 4×8 board)
- `GameState` is one concrete struct that supports n_players ≥ 2

## Investigation

PR 1 only built scaffolding. Before implementing, settle:

1. **Board topology** — is the canonical board the same 4×8 banqi grid with three home zones overlaid, or is it a non-rectangular shape with a notch / extra cells? Wiki article needs careful reading.
2. **Piece distribution per faction** — does each of 蜀/吳/魏 get 16 pieces (full banqi set × 3 = 48 pieces on a 32-cell board → impossible), or a reduced set (32 / 3 ≈ 10 each + a king)?  Likely the latter; need to confirm.
3. **Capture rules** — standard banqi rank rules apply between any two factions, or are there alliance/non-aggression mechanics?
4. **Turn order** — straight round-robin (蜀 → 吳 → 魏 → 蜀)?
5. **Win condition** — last faction with non-empty piece set, or first to checkmate someone, or a points-based scoring?

## Options considered

| Option | Pros | Cons |
|---|---|---|
| A. Implement the simplest 3-faction round-robin with standard banqi rank rules and "last faction standing" win | Fast to ship; matches the architecture already laid down | May not match the "real" rules |
| B. Read the wiki carefully and implement faithfully | Matches user expectation of the named variant | Higher up-front research |
| C. Make the rule choices configurable (alliance on/off, points/elimination) | Flexible | Premature; nobody's asked for the variations yet |

## Current blocker / open questions

- Need the user to spec the exact 三國暗棋 ruleset they want, or commit to reading the wiki and picking sensible defaults.
- The `BoardShape::ThreeKingdom` mask is currently `false` for all cells (PR-1 placeholder). Once topology is decided, fill in the mask.

## Decision (if any)

Pending user spec.

## References

- <https://zh.wikipedia.org/wiki/三國暗棋>
- <https://zh.wikipedia.org/zh-sg/三國暗棋>
- `docs/rules/three-kingdoms.md` skeleton
