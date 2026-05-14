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

## First-flip & colour assignment

The engine has two modes for **who is allowed to make the very first reveal**:

### Default — either side may flip first

`PlayerView::banqi_awaiting_first_flip = true` until the first `Move::Reveal` applies. In this state:

- The deployment layer (chess-net server or chess-web `HostRoom`) accepts a `Move::Reveal` from **either seat**. The clicking seat is re-pointed via `state.set_active_seat(seat)` before `make_move` runs.
- The projected `PlayerView::legal_moves` lists all 32 reveals for **both** observers — clients render the board as fully clickable for both players.
- Sidebars show a neutral "Awaiting first flip — either side may flip" / "未翻牌 — 任一方皆可先翻" message in place of "Red to move".

Once the first flip applies, `banqi_side_assignment(flipper, revealed)` locks `state.side_assignment`. The Taiwan rule applies: the **flipper plays the colour they reveal** (e.g. a player who flips a Black piece controls Black for the rest of the game). The pre-flip sentinel clears and the normal seat-of-move gate resumes.

### `HouseRules::PREASSIGN_COLORS` — legacy mode (host = Red, host flips first)

Setting the bit restores the classic behaviour: seat 0 (the room creator) is RED, seat 1 is BLACK, RED moves first and therefore makes the first reveal. The deployment layer's standard "not your turn" guard rejects any attempted reveal from BLACK before RED has flipped.

`PlayerView::banqi_awaiting_first_flip` is always `false` in this mode — the colour assignment is logically pre-committed at room creation. Choose this mode when you want a faster "agreed handshake" (the host always flips the first piece) or for testing legacy behaviour.

### Round-trip + protocol notes

- `PlayerView.banqi_awaiting_first_flip` ships with `#[serde(default)]` — older payloads decode as `false`, which is the correct legacy interpretation. No `PROTOCOL_VERSION` bump.
- The engine still enforces the Taiwan flipper-plays-revealed rule; this change only affects **who** may make the first reveal, not **which colour the flipper ends up playing**.

## Movement

- Face-down pieces cannot move (only flipped). On your turn, `Move::Reveal { at }` flips any face-down tile.
- Face-up pieces move **one orthogonal step** to an empty square or to capture an enemy. Diagonal moves are NOT part of base banqi (the `HORSE_DIAGONAL` house rule adds diagonal *captures* only — see [`banqi-house.md`](banqi-house.md)).
- **Cannon**: captures by jumping over **exactly one piece** (the screen) to land on an enemy. The screen may itself be face-down. A cannon's non-capturing move is also one orthogonal step.

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

- **Win — checkmate-style**: opponent has no legal moves (all pieces captured, or stuck behind hidden pieces with no flips/moves available). Engine emits `WinReason::Stalemate`.
- **Win — material**: only one side has pieces left on the board (`WinReason::OnlyOneSideHasPieces`). Common in 暗吃-heavy games where one player wipes the other before they reveal much.
- **Draw**: 40 plies without a capture or reveal (`DrawReason::NoProgress`, tracked by `no_progress_plies`).
- **Repetition**: threefold position repetition is a draw (TODO — engine work in progress).

## References

- <https://darkchess.funtown.com.tw/rules2.html>
