# Banqi House Rules (家規)

> **Status**: skeleton — formal spec to be filled in alongside implementation.

Five independent toggles. Combine freely. Three named presets bundle common combos.

## Toggles

### `CHAIN_CAPTURE` — 連吃

After a capture, if the capturing piece can immediately capture another enemy piece (in the same line of motion, by its normal capture rules), it **may** continue. The chain ends when:

- the next square is empty (cannot capture),
- the next square holds a face-down piece (unless `DARK_CHAIN` is also enabled),
- the next square holds a friendly piece, or
- the player chooses to stop.

The whole chain is one logical move (one history entry, one network message).

### `DARK_CHAIN` — 暗連

Modifies `CHAIN_CAPTURE`. The chain may continue **through face-down squares**. When the chain enters a face-down square, that piece is revealed atomically as part of the move; if the revealed piece is friendly or unbeatable, the chain stops there.

Implies `CHAIN_CAPTURE`.

### `CHARIOT_RUSH` — 車衝

Chariot moves any number of empty squares in a line (xiangqi-style ray), and captures the first enemy piece on that line. Replaces base banqi's 1-step chariot move.

### `HORSE_DIAGONAL` — 馬斜

Horse moves like a xiangqi horse: L-shape with leg (馬腿) blocking. Replaces base banqi's 1-step horse move.

### `CANNON_FAST_MOVE` — 炮快移

Cannon may move any number of empty squares in a line for non-capturing moves (capture still requires the one-piece jump). Speeds up cannon repositioning without changing capture geometry.

## Presets

```rust
HouseRules::PRESET_PURIST     // empty: classic banqi
HouseRules::PRESET_TAIWAN     // CHAIN_CAPTURE | CHARIOT_RUSH
HouseRules::PRESET_AGGRESSIVE // CHAIN_CAPTURE | DARK_CHAIN | CHARIOT_RUSH | HORSE_DIAGONAL
```

## Composition Example

```rust
use chess_core::rules::{RuleSet, HouseRules};

let rules = RuleSet::banqi(HouseRules::CHAIN_CAPTURE | HouseRules::CHARIOT_RUSH);
let mut state = chess_core::state::GameState::new(rules);
```

Each flag controls one branch in `rules/banqi.rs::generate`. No traits, no boxing.

## References

- <https://darkchess.funtown.com.tw/rules2.html>
