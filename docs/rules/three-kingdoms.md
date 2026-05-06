# Three Kingdoms Banqi (三國暗棋)

> **Status**: skeleton — implementation deferred to PR 2. Architecture supports it from PR 1
> via `BoardShape::ThreeKingdom`, `Side(0..3)`, and `TurnOrder` with three seats.

## Concept

A three-player banqi variant. Three teams (e.g. 蜀 / 吳 / 魏) each control one banqi piece-set, played on a half xiangqi board with three home zones.

## Board

- 4×8 grid, possibly with corner cells masked off depending on the variant (TODO: settle on canonical mask).
- Three home zones, one per faction.

## Pieces (per faction)

Each faction starts with 16 pieces (one full banqi set), face-down on the shared grid. TODO: confirm exact distribution per the canonical rule set on the wiki.

## Turn Order

Round-robin (蜀 → 吳 → 魏 → 蜀…). When a faction is fully eliminated, `TurnOrder::advance_skipping` removes them from the cycle.

## Capture

TODO: specify three-way capture rules. Key questions:

- Can faction A capture faction B's pieces under standard banqi rank rules?
- Are there alliance / temporary truce mechanics?
- Win condition: last faction standing? First to eliminate one other?

## Win

TODO.

## References

- <https://zh.wikipedia.org/wiki/三國暗棋>
- <https://zh.wikipedia.org/zh-sg/三國暗棋>
