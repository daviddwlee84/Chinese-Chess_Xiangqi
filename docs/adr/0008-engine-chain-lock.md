# ADR 0008 — Engine-driven 連吃 chain mode (protocol v5)

Status: accepted (shipped 2026-05-07)
Supersedes: nothing
Related: ADR-0004 (PlayerView projection), ADR-0007 (in_check flag)

## Context

`HouseRules::CHAIN_CAPTURE` (連吃) lets a banqi piece keep capturing
after its first capture as long as further enemies are reachable. The
original v0 design baked the entire chain into a single
`Move::ChainCapture { from, path: SmallVec<[ChainHop; 4]> }` — the
move generator emitted every legal chain-extension permutation up
front, the player picked one, and the engine applied all hops
atomically.

Two problems surfaced once chess-tui and chess-web shipped a
click-to-play UX on top of this:

1. **Same-line bias.** `gen_chain_extensions` only walked the same
   direction as the seeding capture. Real banqi chains turn corners —
   capture north, then east, then south. Generating every multi-hop
   permutation in any direction would explode the move list.
2. **UI chain-builder duplication.** Both clients had to maintain their
   own state machine for "user clicked their first capture target,
   now show what extensions are legal" — and that machine had to
   re-implement parts of the move generator (filtering legal moves
   whose `path[0..n]` starts with the user's prefix). The web client's
   first cut shipped buggy because the local copy diverged from the
   engine's notion of what extends a chain.

The user feedback that crystallised the rewrite (paraphrased from the
in-thread session): "連吃 chains should work step-by-step, in any
direction, and the engine — not the UI — should track that I'm still
mid-chain."

## Decision

Move chain bookkeeping into `GameState`. The atomic `Move::ChainCapture`
variant stays in the enum (snapshot back-compat + same-direction
extensions still emit it for the engine's own use), but the
player-facing path is now per-hop:

```rust
pub struct GameState {
    /* ... */
    /// `Some(sq)` while a 連吃 chain is live: the piece at `sq` just
    /// captured, the turn does NOT advance, and `legal_moves` is
    /// filtered to further captures from `sq` plus a single
    /// `Move::EndChain { at: sq }` terminator.
    #[serde(default)]
    pub chain_lock: Option<Square>,
}

pub enum Move {
    /* ... */
    /// Explicit "I'm done — pass the turn" issued by the player while
    /// `chain_lock` is set. Clears the lock and advances the turn.
    /// `at` is the locked square (carried so undo can restore).
    EndChain { at: Square },
}
```

State machine in `state/mod.rs`:

1. `make_move` applies the user's move, then calls
   `compute_chain_lock_after(&move)`. For a chain-eligible capture
   (variant + `CHAIN_CAPTURE` flag + attacker actually moved), it asks
   `has_capture_from(state, landing, attacker)` — true iff the
   attacker has at least one further legal capture from its new
   square in any direction the piece is allowed to attack
   (orthogonal, cannon-jump, or +diagonal under `HORSE_DIAGONAL`).
2. If chain-eligible: `chain_lock = Some(landing)`. **Skip the
   `turn_order.advance()` call.** The same player gets to move again.
3. `legal_moves` is filtered to captures whose `origin == chain_lock`,
   plus a synthetic `Move::EndChain { at: chain_lock }`. The
   terminator is the explicit "release" gesture.
4. Each subsequent hop runs the same path and either keeps `chain_lock`
   live or clears it (no further captures available → chain naturally
   ends → turn advances).
5. `Move::EndChain` clears `chain_lock` and advances the turn.

Undo (`unmake_move`) reads `MoveRecord.chain_lock_before` (a new
`#[serde(default)]` field on `MoveRecord`) and restores the pre-move
lock, then rewinds the turn iff the chain lock isn't currently
restored to `Some` (i.e. iff the move that's being undone advanced
the turn).

Client-side: clicks during chain mode go through a single branch:

- click on `chain_lock` square → send `Move::EndChain`
- click on a legal chain-mode capture target → send the corresponding
  `Move::Capture` / `Move::DarkCapture`
- any other click → ignored, with a hint "連吃 active — capture from
  the locked piece, or click it to end"

`PlayerView` exposes `chain_lock: Option<Square>` (also
`#[serde(default)]`) so the client can render the locked piece
distinctly without re-deriving the state.

## Consequences

- **Captures in any direction.** The chain follows the engine's full
  capture geometry — no more same-line restriction. Cannon-jump-then-
  orthogonal-step or horse-diagonal-then-orthogonal sequences work.
  Tested via `crates/chess-core/tests/banqi_chain_mode.rs::perpendicular_capture_in_chain_keeps_lock`
  and the cannon / horse-diagonal chain extension tests added under
  the rule fixes.
- **Single source of truth for chain legality.** Clients no longer
  duplicate any chain logic — they just render `legal_moves` and ship
  one `Move` per click. `find_move(view, locked, sq)` looks up the
  matching capture; `end_chain_move(view)` builds the EndChain.
- **`PlayerView.chain_lock` is the only chain state crossing the
  wire.** The atomic `Move::ChainCapture` variant stays serializable
  for snapshot replay, but live network traffic is per-hop.
- **Wire bump.** Protocol v5 adds three `#[serde(default)]` fields:
  `PlayerView.chain_lock`, `PlayerView.current_color` (independent of
  this ADR — covers banqi first-flip seat→colour mapping), and
  `MoveRecord.chain_lock_before`. v4 clients deserialise v5 messages
  with the defaults; v5 clients deserialise v4 messages cleanly.
- **DarkCapture interaction.** A `Move::DarkCapture` resolved as
  Probe or Trade does NOT activate chain mode (the attacker stayed
  put or died — there's no new landing square to lock). Only the
  Capture outcome can chain. Tested in `banqi_chain_mode.rs` and
  `banqi_dark_capture.rs`.
- **Engine refuses to advance the turn while a chain is live.**
  Move-gen running for the WRONG side during chain mode would
  emit nonsense; legal-move filter + the implicit `side_to_move`
  freeze prevents that. Pressing Esc in TUI / clicking the locked
  piece in either client is the only way to bail out.

## Alternatives considered

- **Keep atomic-only `Move::ChainCapture` and ship a richer move
  generator** that emits all multi-direction chains. Rejected: the
  branching factor for a 4×8 board is already high enough that
  enumerating 4-deep multi-direction chains visibly slowed the move-
  gen pass, and the wire payload (a `SmallVec<[ChainHop; 4]>` per
  candidate) ballooned. The state-machine alternative caps the
  legal-move list at "captures from one square + one EndChain".
- **Track chain state on the client** (the original chess-web
  prototype). Rejected after the first round of bugs — the chain
  builder had to re-derive what the engine considered a legal
  extension, and any divergence caused either silent illegal moves
  or "phantom dots" in the UI.
- **Cap chains at length N.** Rejected as a non-rule constraint — if
  the rules let you chain forever, the engine should let you chain
  forever. (In practice the board's piece count caps it at ~16.)
