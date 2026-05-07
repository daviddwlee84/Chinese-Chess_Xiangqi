# Banqi House Rules (家規)

> **Status**: skeleton — formal spec to be filled in alongside implementation.

Six independent toggles. Combine freely. Three named presets bundle common combos.

## Toggles

### `CHAIN_CAPTURE` — 連吃

After a capture, if the capturing piece can immediately capture another enemy piece (in the same line of motion, by its normal capture rules), it **may** continue. The chain ends when:

- the next square is empty (cannot capture),
- the next square holds a face-down piece (chains-with-dark-hops is a Phase 2 follow-up — see TODO.md),
- the next square holds a friendly piece, or
- the player chooses to stop.

The whole chain is one logical move (one history entry, one network message).

### `DARK_CAPTURE` — 暗吃

Atomically reveal a face-down piece and resolve a capture in one move. **Probe semantics by default**: if the revealed piece outranks (or is friendly), the target stays revealed in place and the attacker stays put; the turn ends regardless. The player commits to the action without prior knowledge of the target's identity.

Renamed from `DARK_CHAIN` (same bit position; old snapshots and the `dark-chain` token still parse). The combination `DARK_CAPTURE | CHAIN_CAPTURE` is informally called 暗連, but chain captures still stop at face-down tiles in this round — see TODO.md "chains-with-dark-hops" for the Phase 2 extension.

### `DARK_CAPTURE_TRADE` — 暗吃 (搏命變體)

Modifies `DARK_CAPTURE`. On rank-fail, the *attacker* is removed from the board (the small piece dies attacking a larger one) instead of the probe-stay-put behavior. The defender stays revealed in place. Implies `DARK_CAPTURE`.

### `CHARIOT_RUSH` — 車衝

Chariot moves any number of empty squares in a line (xiangqi-style ray). With at least one empty square between attacker and target, the chariot may capture **any** piece (rank ignored). Adjacent (no gap) captures still follow the standard rank rules.

### `HORSE_DIAGONAL` — 馬斜

Adds 4 diagonal one-step moves to the horse alongside its existing 4 orthogonal ones. **Diagonal captures ignore rank** (any piece, including General). Orthogonal captures still follow the standard rank rules.

### `CANNON_FAST_MOVE` — 炮快移

Cannon may move any number of empty squares in a line for non-capturing moves (capture still requires the one-piece jump). Speeds up cannon repositioning without changing capture geometry. *(Currently accepted but not wired into move-gen — see TODO.md.)*

## Presets

```rust
HouseRules::PRESET_PURIST     // empty: classic banqi
HouseRules::PRESET_TAIWAN     // CHAIN_CAPTURE | CHARIOT_RUSH
HouseRules::PRESET_AGGRESSIVE // CHAIN_CAPTURE | DARK_CAPTURE | CHARIOT_RUSH | HORSE_DIAGONAL
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
