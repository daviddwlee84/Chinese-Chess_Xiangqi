# ADR 0004: `PlayerView` projection as the network ABI

## Context

Banqi (and 三國暗棋) hide piece identities until they're flipped. A naive design serializes `GameState` over the wire — and immediately leaks every face-down piece's identity to every observer.

We need:

1. A type the network layer can serialize that is **provably free of hidden info** for a given observer.
2. A way to model "I flipped a piece, but the identity wasn't decided in advance" without making `Move` polymorphic.

## Decision

```rust
pub struct PlayerView {
    pub observer: Side,
    pub cells: Vec<VisibleCell>,
    pub side_to_move: Side,
    pub status: GameStatus,
    pub legal_moves: MoveList,
    /* + dimensions + shape */
}

pub enum VisibleCell {
    Empty,
    Hidden,                  // banqi face-down: identity is opaque
    Revealed(PieceOnSquare),
}

impl PlayerView {
    pub fn project(state: &GameState, observer: Side) -> Self;
}
```

`PlayerView` is the **only** struct the server hands to clients. `GameState` itself never crosses the wire.

For the flip-on-touch problem:

```rust
pub enum Move {
    Reveal { at: Square, revealed: Option<Piece> },
    /* ... */
}
```

- Client → Server: sends `Reveal { at, revealed: None }` (the player doesn't know what they'll flip).
- Server: applies the move using the authoritative deck/seed; fills in `Some(piece)`.
- Server → Clients: broadcasts the post-flip move with `Some(piece)`.

The `Option` collapses both states (pre-flip and post-flip) into one variant. Undo restores the face-down state because the original `Some(piece)` is in `history`.

## Consequences

- Server is authoritative for all randomness and hidden state.
- Property test: `serde_json::to_string(&view)` for any `(state, observer)` must not contain the byte sequence of any hidden piece's identity. Any future bug that leaks information fails this test loudly.
- Clients can be dumb terminals — they consume `PlayerView`, render it, and ship `Move`s back. No client-side state machine needed.
- Spectators are just another observer with no `side_to_move` privileges.

## Consequences (gotchas)

- The `Option<Piece>` on `Reveal` means client and server speak slightly different dialects of the same enum — flagged in `chess-net` docs when that crate lands.
- For replays, history stores the `Some(piece)` form so observers can re-watch with full information.
