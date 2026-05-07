# Snapshot & Replay formats

Two layers, on purpose. JSON is canonical (lossless, used for save/load and the future network protocol). The `.pos` text DSL is human-friendly for hand-written test fixtures and endgame puzzles.

## JSON (canonical)

```rust
use chess_core::state::GameState;

let json: String = state.to_json()?;        // serialize
let state = GameState::from_json(&json)?;   // deserialize
```

Just `serde_json` of `GameState`. Every field is preserved including `history`, `side_assignment`, `no_progress_plies`, `status`. Use this for save files and over-the-wire transport (after [`view::PlayerView`](../crates/chess-core/src/view.rs) projection for hidden-info-safe variants like banqi).

For replays:

```rust
use chess_core::replay::{Replay, ReplayMeta};

let replay = Replay::from_game(&state, ReplayMeta::empty())?;
let json = replay.to_json()?;
let restored = Replay::from_json(&json)?;
```

A `Replay` is `{ version, metadata, initial: GameState, moves: Vec<Move> }`. `Replay::from_game` walks the state's history back to the starting position via `unmake_move` and records the moves in order — so any played-out `GameState` can be turned into a Replay losslessly.

## `.pos` text DSL

Two-player variants only (xiangqi, banqi). Three-kingdoms uses JSON.

```text
# Comments start with '#' and run to end-of-line.
variant: xiangqi              # required: xiangqi | banqi
side_to_move: red             # required: red | black

no_progress_plies: 0          # optional, default 0
house: chain,rush             # optional banqi house rules (comma-list).
                              # Tokens: chain, dark, dark-trade, rush,
                              # horse, cannon. `dark-chain` is accepted
                              # as a back-compat alias for `dark`.
seed: 42                      # optional banqi seed (u64)
side_assignment: red,black    # optional banqi side -> piece-color mapping

board:                        # rows from rank H-1 (top) down to rank 0 (bottom)
  . . . . k . . . .
  . . . . . . . . .
  . . . R R R . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . . . . . .
  . . . . K . . . .
```

### Pieces

Xiangqi-FEN convention. **Uppercase = Red, lowercase = Black**.

| Letter | Piece (Red / Black) |
|---|---|
| `K` / `k` | General (帥/將) |
| `A` / `a` | Advisor (仕/士) |
| `B` / `b` | Elephant (相/象) |
| `R` / `r` | Chariot (俥/車) |
| `N` / `n` | Horse (傌/馬) |
| `C` / `c` | Cannon (砲/炮) |
| `P` / `p` | Soldier (兵/卒) |

`.` = empty square.

For banqi face-down pieces, prefix with `?`:

| Token | Meaning |
|---|---|
| `K` | Face-up Red general |
| `k` | Face-up Black general |
| `?K` | Face-down Red general |
| `?k` | Face-down Black general |

The engine knows the identity behind a face-down piece — that's how `make_move` resolves a player's `Reveal { revealed: None }` request server-side. The `.pos` format makes that identity explicit so test fixtures are deterministic.

### Board layout

- Rows are top-to-bottom: the **top row is the highest rank** (rank 9 for xiangqi, rank 7 for banqi); the **bottom row is rank 0**.
- This matches how a board looks on screen with Red on the bottom.
- Files are left-to-right: `a` (file 0) on the left, `i` (file 8) on the right for xiangqi; `a..d` for banqi.
- Cells separated by whitespace (any amount). Tokens are 1 char (`.`, `K`, …) or 2 chars (`?K`, …). The parser doesn't care about column alignment.

### Header field order

Order doesn't matter. `board:` must be on its own line; rows must be indented (any whitespace prefix). A blank line ends the board section.

## Use cases

### Test fixtures

```rust
// tests/end_conditions.rs
let text = std::fs::read_to_string("tests/fixtures/xiangqi/three-chariot-mate.pos")?;
let mut state = GameState::from_pos_text(&text)?;
state.refresh_status();
assert_eq!(state.status, GameStatus::Won { winner: Side::RED, reason: WinReason::Checkmate });
```

### Endgame puzzle mode

```rust
let initial = GameState::from_pos_text(&fs::read_to_string("puzzles/mate-in-2.pos")?)?;
let replay = Replay::new(initial, ReplayMeta::empty());
let player_state = replay.play_to(0)?;   // hand to the player
// ... player makes moves, you collect them, score ...
```

### Animation playback

```rust
let replay = Replay::from_json(&saved_game)?;
for step in 0..=replay.len() {
    let frame = replay.play_to(step)?;
    render(&frame);
    sleep(animation_delay);
}
```

### Fork from any state

```rust
let replay = Replay::from_json(&saved_game)?;
let mut fork = replay.play_to(midpoint)?;
fork.make_move(&different_move)?;          // diverge from the original
// `fork` now has its own independent history and can keep playing
```

## Limitations

- The `.pos` DSL covers two-player variants only. Three-kingdoms goes through JSON.
- No FEN-style run-length compression (`9.` for nine empties) — every cell is explicit. Fine for hand-written fixtures; if you find yourself generating long .pos strings programmatically, use JSON instead.
- The `.pos` format does not record the `history` field — fixtures load with empty history. Use JSON (`to_json`) when you need to preserve a played-out state with its full move log.

## See also

- [`crates/chess-core/src/snapshot.rs`](../crates/chess-core/src/snapshot.rs) — implementation
- [`crates/chess-core/src/replay.rs`](../crates/chess-core/src/replay.rs) — replay primitives
- [`crates/chess-core/tests/fixtures/`](../crates/chess-core/tests/fixtures/) — example positions
- [`crates/chess-core/tests/end_conditions.rs`](../crates/chess-core/tests/end_conditions.rs) — fixture-driven tests
- [ADR 0004](adr/0004-player-view-projection.md) — why the network ships `PlayerView`, not raw `GameState`
