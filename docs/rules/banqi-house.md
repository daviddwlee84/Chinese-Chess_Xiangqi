# Banqi House Rules (家規)

> **Status**: skeleton — formal spec to be filled in alongside implementation.

Six independent toggles. Combine freely. Three named presets bundle common combos.

## Toggles

### `CHAIN_CAPTURE` — 連吃

After a capture, if the capturing piece can immediately capture another enemy piece **in any direction the piece is allowed to attack** (orthogonal for most; cannon-jump for cannons; +diagonal for horses with `HORSE_DIAGONAL`), it **must** continue OR explicitly end the chain — the engine refuses to advance the turn while a chain is live.

Engine state machine (`GameState.chain_lock`):

1. After a chain-eligible capture lands the attacker on a new square `sq`, the engine sets `chain_lock = Some(sq)` and does NOT advance the turn.
2. While locked, `legal_moves` is filtered to captures originating at `sq`, plus a single `Move::EndChain { at: sq }` terminator. The terminator is the explicit "I'm done — pass the turn" move.
3. The chain naturally ends when the attacker has no further captures from its current square (`chain_lock` clears automatically and the turn advances).

Player-facing UX: each hop is a separate click / `Move::Capture` (or `Move::DarkCapture`) — single-step, NOT an atomic multi-hop move. To end early: click the locked piece (TUI/web) or press `Esc` (TUI). Engine still ships an atomic `Move::ChainCapture { from, path }` variant in `Move`'s enum for back-compat with older snapshots and for the same-direction extension generator, but that path isn't user-facing in the current clients.

Chains-with-dark-hops (atomic chain captures that pass through hidden tiles) is a Phase 2 follow-up — see `TODO.md`.

### `DARK_CAPTURE` — 暗吃

Atomically reveal a face-down piece and resolve a capture in one move. The player commits to the action without prior knowledge of the target's identity. Three outcomes (resolved at apply-time from the revealed piece's rank vs the attacker's):

- **Capture** — attacker outranks (or is one of the rank-bypass cases below): attacker takes the target's square, defender removed.
- **Probe (default)** — attacker is outranked: target stays revealed in place, attacker stays put, turn ends. Information cost only.
- **Trade** — attacker is outranked AND `DARK_CAPTURE_TRADE` is set: attacker is removed instead of staying put.

**Rank bypasses** (the dark-capture *always* resolves to Capture regardless of the revealed piece's rank):

- **Cannon attacker via jump-over-screen.** Standard banqi cannons capture any piece via jump; the dark-capture path mirrors that. (Cannons do NOT emit a 1-step adjacent dark-capture — only the jump variant.)
- **Horse attacker on a diagonal hit under `HORSE_DIAGONAL`.** 馬斜's "any piece" diagonal capture extends to the dark-capture path. Orthogonal horse dark-capture still obeys rank.

Emission paths in the move generator:
- 1-step orthogonal onto a hidden tile (any non-Cannon piece).
- Chariot rush (`CHARIOT_RUSH`) onto a hidden blocker past a gap.
- Cannon jump-over-screen onto a hidden tile.
- Horse diagonal onto a hidden tile (`HORSE_DIAGONAL`).

Renamed from `DARK_CHAIN` (same bit position; old snapshots and the `dark-chain` token still parse). The combination `DARK_CAPTURE | CHAIN_CAPTURE` is informally called 暗連; chain captures via the engine `chain_lock` machine can step through dark-captures naturally because each hop is a separate move. (The atomic `Move::ChainCapture` variant still stops at face-down tiles — chains-with-dark-hops on the atomic path is the Phase 2 follow-up tracked in `TODO.md`.)

### `DARK_CAPTURE_TRADE` — 暗吃 (搏命變體)

Modifies `DARK_CAPTURE`. On rank-fail, the *attacker* is removed from the board (the small piece dies attacking a larger one) instead of the probe-stay-put behavior. The defender stays revealed in place. Implies `DARK_CAPTURE`.

### `CHARIOT_RUSH` — 車衝

Chariot moves any number of empty squares in a line (xiangqi-style ray). With at least one empty square between attacker and target, the chariot may capture **any** piece (rank ignored). Adjacent (no gap) captures still follow the standard rank rules.

### `HORSE_DIAGONAL` — 馬斜

Adds **diagonal one-step *captures only*** to the horse — the horse may NOT slide diagonally onto an empty square. Orthogonal moves (steps + captures) are unchanged. Diagonal captures **ignore rank** (any piece, including General); diagonal dark-captures (under `DARK_CAPTURE`) likewise resolve to Capture regardless of the revealed piece's rank. Orthogonal captures still follow the standard rank rules — the bypass is diagonal-only.

### `CANNON_FAST_MOVE` — 炮快移

Cannon may move any number of empty squares in a line for non-capturing moves (capture still requires the one-piece jump). Speeds up cannon repositioning without changing capture geometry. *(Currently accepted but not wired into move-gen — see TODO.md.)*

### `PREASSIGN_COLORS` — 預先指定顏色 (bit `1 << 6`)

Restores legacy banqi pairing: seat 0 (room creator / host) is fixed as **Red**, seat 1 (joiner) is fixed as **Black**, and Red is forced to make the first reveal. The deployment layer's standard "not your turn" guard rejects any Black-first reveal attempt.

Default is **off**, in which case banqi defers colour assignment until the first flip — see [`banqi.md` § First-flip & colour assignment](banqi.md#first-flip--colour-assignment). The Taiwan flipper-plays-revealed rule applies in both modes; this flag only controls *who* may make the first reveal.

Engine surface:
- `GameState::banqi_awaiting_first_flip()` returns `true` iff variant is banqi, no flip has happened yet, and this flag is **off**.
- `PlayerView::banqi_awaiting_first_flip` projects the same signal to clients (with `#[serde(default)]` for back-compat).

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
