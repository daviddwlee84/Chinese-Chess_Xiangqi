# ADR 0002: Square as `u16` linear index

## Context

We support multiple board shapes:

- 9×10 standard xiangqi
- 4×8 banqi
- Three-kingdoms (irregular shape, holes in the grid)
- 大盤 variants (n×m, possibly up to 19×19)

A `(file, rank)` tuple coord type bakes a rectangular assumption into every signature in the crate.

## Decision

```rust
#[repr(transparent)]
pub struct Square(pub u16);
```

A linear index. The `Board` knows its `BoardShape` and converts between `Square` and `(File, Rank)` as needed. Off-board / hole squares are excluded by a per-shape mask, never by coord arithmetic.

`u16` accommodates 19×19 = 361 with room to spare; future expansion (e.g. Go board) won't force a type change.

## Consequences

- `Square` is `Copy`, fits in a register, hashes cheaply, serializes as a single integer.
- Move enum stays small — `Square` is 2 bytes.
- Board iteration uses the shape's mask; iterating "all valid squares" is uniform across shapes.
- `Direction` arithmetic happens via `Board::step` rather than `Square + Δ` — a small ergonomic cost paid once.

## Rejected alternatives

- `(File, Rank)` tuple: rectangular bias, larger move enum, awkward for 3K topology.
- `enum Square { Xiangqi(...), Banqi(...), ... }`: explodes pattern matches everywhere; provides no type safety in practice because `chess-core` already pairs board + state.
