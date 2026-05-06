# Standard Xiangqi (象棋)

> **Status**: skeleton — fill in details as canonical reference.

## Board

- 9 files × 10 ranks (90 intersections — pieces sit on intersections, not cells)
- Palace (九宮): 3×3 region on each player's back rank; General and Advisors confined here
- River (楚河漢界): horizontal divide between rank 4 and rank 5; Elephants cannot cross

## Pieces (16 per side)

| Piece | Red | Black | Count |
|---|---|---|---|
| General | 帥 | 將 | 1 |
| Advisor | 仕 | 士 | 2 |
| Elephant | 相 | 象 | 2 |
| Chariot | 俥 | 車 | 2 |
| Horse | 傌 | 馬 | 2 |
| Cannon | 炮 | 砲 | 2 |
| Soldier | 兵 | 卒 | 5 |

## Initial Position

TODO: ASCII diagram of the standard setup.

## Movement

- **General**: one step orthogonally; confined to palace
- **Advisor**: one step diagonally; confined to palace
- **Elephant**: exactly two diagonal steps with no piece on the midpoint (象眼); cannot cross river
- **Chariot**: any number of orthogonal squares until blocked
- **Horse**: L-shape (one orthogonal + one diagonal outward) with no piece on the orthogonal step (馬腿)
- **Cannon**: moves like chariot when not capturing; captures by jumping exactly one piece (炮架) to land on an opposing piece
- **Soldier**: one step forward before crossing river; one step forward or sideways after crossing; never backward

## Special Rules

- **Flying General (飛將)**: the two generals may not face each other on an empty file. Treat as illegal — moving a piece off such a file with the generals exposed loses immediately.
- **Check / Checkmate / Stalemate**: standard.
- **Long check / perpetual chase**: TODO — specify the official rule (e.g. AXF / Asian rules).

## Draw Conditions

- Threefold repetition
- 60-move rule (no capture for 60 plies)
- Insufficient material
- Mutual agreement

## Notation

See [`../notation.md`](../notation.md). Both ICCS (`h2e2`) and WXF (`炮二平五`) supported.

## References

TODO: link to authoritative sources.
