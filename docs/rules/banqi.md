# Banqi (暗棋 / 盤棋)

> **Status**: skeleton — base banqi rules.
> House-rule extensions live in [`banqi-house.md`](banqi-house.md).

## Board

- 4 files × 8 ranks (half a xiangqi board, oriented sideways)
- All 32 standard xiangqi pieces are placed face-down at random in the 32 cells

## Setup

1. Shuffle all 32 pieces (16 red + 16 black) face-down across the 4×8 grid.
2. The starting player flips any piece on their turn — the **color** they reveal determines their faction for the rest of the game (`SideAssignment` is locked here).
3. From this point onward, players control their assigned color.

The `chess-core` setup uses a seedable `ChaCha8Rng` so games are reproducible from a `RuleSet::banqi_seed`.

## Movement

- Face-down pieces cannot move (only flipped).
- Face-up pieces move **one orthogonal step** to an empty square or to capture an enemy.
- **Cannon**: captures by jumping over **exactly one piece** (face-up or face-down) to land on an enemy. A cannon's non-capturing move is also one step.

## Capture (rank-based)

| Rank | Piece |
|---|---|
| 6 | General (將) |
| 5 | Advisor (士) |
| 4 | Elephant (象) |
| 3 | Chariot (車) |
| 2 | Horse (馬) |
| 1 | Cannon (炮) |
| 0 | Soldier (卒) |

A piece may capture an enemy piece of equal or lower rank, **with two exceptions**:

- **Soldier beats General** (卒剋將). A soldier captures the general; the general cannot capture the soldier.
- **Cannon captures by jumping**, ignoring rank entirely (炮 can take any enemy via a one-piece screen).

## Win / Draw

- **Win**: opponent has no legal moves (all pieces captured, or stuck behind hidden pieces with no flips/moves available).
- **Draw**: 40 plies without a capture or reveal (`no_progress_plies`).
- **Repetition**: threefold position repetition is a draw.

## References

- <https://darkchess.funtown.com.tw/rules2.html>
