# Banqi: dark-capture, first-flip color, horse diagonal, chain step-UX

## Context

Three banqi (暗棋) issues + one chain-capture UX rework, addressed
together because they all touch banqi move-gen + house rules. None of
these affect xiangqi.

1. **Dark-capture (`暗吃`) UX bug.** When a player has selected their
   piece and targets a hidden tile, the only legal move emitted is
   `Move::Reveal` (flip-only). Both TUI (`clients/chess-tui/src/app.rs`
   line 1118) and web (`clients/chess-web/src/state.rs::find_move`,
   used at `pages/play.rs:178`) look up legal moves by the `(from, to)`
   square pair, but `Move::Reveal::to_square()` returns `None`
   (`crates/chess-core/src/moves.rs:67`) — so "select piece + click
   hidden" matches nothing and falls through to "Illegal move". The fix
   needs an atomic reveal-and-capture move.

   User clarified TWO traditional failure semantics — both are real
   variants and we ship both:
   - **Probe** (default `DARK_CAPTURE`): on rank-fail, target stays
     revealed in place, attacker stays put, turn ends.
   - **Trade** (opt-in `DARK_CAPTURE_TRADE`): on rank-fail with attacker
     smaller than defender, attacker is removed (small piece dies
     attacking a bigger piece). On rank-pass, normal capture.

2. **First-flip determines color — engine bug.** `state/mod.rs:147–152`
   sets `side_assignment.mapping` on the first reveal, but `banqi.rs:38`
   filters pieces by `pos.piece.side == me` where `me = state.side_to_move`
   (the seat id, never remapped). The mapping is set, serialized, and
   ignored. In practice the engine always plays Red-seat-moves-Red,
   Black-seat-moves-Black — matches what the user observed. Fix: route
   piece-color filters through a `current_color()` helper that consults
   the mapping.

3. **`HORSE_DIAGONAL` (`馬斜`) is a no-op flag.** Declared at
   `house.rs:19`, included in `PRESET_AGGRESSIVE`, but `banqi.rs` never
   consumes it. Per user: diagonal one-step, captures any piece
   (rank-ignored).

4. **`連吃` step-by-step UX.** User wants chain captures to feel
   single-step from the player's perspective: click a capture, see the
   next-hop options, click again to extend or press Enter to commit. The
   engine keeps emitting atomic `Move::ChainCapture { path }` (chosen
   approach: smaller change, isolated to clients/) — the UI builds the
   path incrementally and finds the matching atomic move at commit time.

   Note: `CHARIOT_RUSH` is already correct in its rank-vs-gap rule —
   `banqi.rs:121` gates the rank-ignoring multi-square capture on
   `!walked.is_empty()`, exactly the user's stated rule.

## Approach

### A. Engine: route piece-color filters through `current_color()`

`crates/chess-core/src/state/mod.rs` — add:

```rust
/// Piece color the active seat actually controls. Banqi remaps after
/// the first flip locks `side_assignment`; xiangqi/three-kingdom return
/// seat-as-color (mapping is None).
pub fn current_color(&self) -> Side {
    match self.side_assignment.as_ref() {
        Some(sa) => sa.mapping[self.side_to_move.0 as usize],
        None => self.side_to_move,
    }
}
```

`crates/chess-core/src/rules/banqi.rs::generate` — replace
`let me = state.side_to_move` (line 33) with `let me = state.current_color()`.
Audit other `side_to_move` reads inside banqi for the same fix.

`refresh_status` (`state/mod.rs:281`) — keep `winner: me.opposite()`
returning the SEAT (chess-net's win-detection routes by seat). Display
layers (chess-tui banner, chess-web banner, `PlayerView` consumers)
should consult `side_assignment.mapping` to render the *piece-color*
side-name when present. Audit chess-tui/chess-web banner code for
banqi-specific labels.

Test: `crates/chess-core/tests/banqi_first_flip_color.rs` — assert that
after RED flips a Black piece, the next legal moves move the Black
pieces (which used to incorrectly be Red).

### B. Engine: rename `DARK_CHAIN` → `DARK_CAPTURE`, add `DARK_CAPTURE_TRADE`

`crates/chess-core/src/rules/house.rs`:

```rust
const CHAIN_CAPTURE       = 1 << 0;  // 連吃
const DARK_CAPTURE        = 1 << 1;  // 暗吃 (probe variant) — renamed from DARK_CHAIN
const CHARIOT_RUSH        = 1 << 2;  // 車衝
const HORSE_DIAGONAL      = 1 << 3;  // 馬斜
const CANNON_FAST_MOVE    = 1 << 4;  // 炮快移 (still no-op)
const DARK_CAPTURE_TRADE  = 1 << 5;  // 暗吃 trade variant — implies DARK_CAPTURE
```

- `normalize()`: drop the old `DARK_CHAIN → CHAIN_CAPTURE` implication.
  Add `DARK_CAPTURE_TRADE → DARK_CAPTURE`.
- `PRESET_AGGRESSIVE`: keep the bit (now named `DARK_CAPTURE`), add
  `HORSE_DIAGONAL` (already present).
- The bit position for `DARK_CAPTURE` (1 << 1) is unchanged from the old
  `DARK_CHAIN`, so existing snapshots load correctly.

CLI/URL token surface:
- `clients/chess-tui` `--house dark` → `DARK_CAPTURE` (already maps to
  bit 1; just keep the alias).
- New token: `dark-trade` → `DARK_CAPTURE_TRADE`.
- `clients/chess-web/src/routes.rs::parse_local_rules` — same.

### C. Engine: new `Move::DarkCapture` variant

`crates/chess-core/src/moves.rs`:

```rust
/// 暗吃 — capture a face-down piece by revealing it atomically.
/// Only emitted when `HouseRules::DARK_CAPTURE` is set. Wire form:
/// client sends `revealed: None, attacker: None`; server fills both.
/// Outcome (capture / probe / trade) computed at apply-time from
/// `revealed`'s rank vs. `attacker`'s rank + `DARK_CAPTURE_TRADE` flag.
DarkCapture {
    from: Square,
    to: Square,
    revealed: Option<Piece>,  // server-filled
    attacker: Option<Piece>,  // server-filled (needed for undo when attacker dies)
},
```

Update helpers:
- `origin_square` → `from`.
- `to_square` → `Some(to)` (so UI lookup matches).
- `resets_no_progress` → `true` (always — reveal + potential capture).

`crates/chess-core/src/state/mod.rs::apply_inner` for `DarkCapture`:
1. Reveal the target.
2. If `revealed.kind` rank check vs `attacker.kind` (banqi `can_capture`)
   passes → move attacker `from → to`, defender removed.
3. Else if `DARK_CAPTURE_TRADE` flag set → attacker removed at `from`,
   defender stays revealed at `to`.
4. Else (probe) → attacker stays at `from`, defender stays revealed at
   `to`.

`unapply_inner` for `DarkCapture`: read `attacker` and `revealed` from
the move; recompute outcome from rules; restore attacker at `from`,
restore defender to `to` as hidden (reset `revealed: false`). Whether
attacker died or not, restore both.

`normalize_for_apply`: fill `revealed` and `attacker` from the board
when they are `None`.

### D. Engine: emit `DarkCapture` in move generation

`crates/chess-core/src/rules/banqi.rs::gen_for_face_up_piece` — when
target is hidden:

```rust
Some(target) if !target.revealed => {
    if house.contains(HouseRules::DARK_CAPTURE) {
        out.push(Move::DarkCapture {
            from, to,
            revealed: None,
            attacker: None,
        });
    }
    // else: face-down blocks (existing behavior).
}
```

For chariot-rush + dark-capture: when the rush ray's blocker is hidden
and `DARK_CAPTURE` is on, emit a `DarkCapture` at the blocker too. This
covers 車衝暗吃.

### E. Engine: wire `HORSE_DIAGONAL`

`crates/chess-core/src/rules/banqi.rs::gen_for_face_up_piece` — when
piece is `PieceKind::Horse` and `house.contains(HouseRules::HORSE_DIAGONAL)`,
also iterate over the 4 diagonal `Direction`s. For each:
- Empty target → `Move::Step` (diagonal slide).
- Enemy face-up → `Move::Capture` (rank IGNORED — any piece).
- Enemy face-down + `DARK_CAPTURE` → `Move::DarkCapture` (atomic
  reveal+capture; outcome decided at apply-time as usual).
- Own piece → blocked.

Check `crates/chess-core/src/coord.rs` for an existing
`Direction::DIAGONAL` array or define one.

Tests: `crates/chess-core/tests/banqi_horse_diagonal.rs` — assert
diagonal moves emitted, diagonal captures ignore rank.

### F. Chain step-UX (`連吃`) — atomic engine + incremental client UI

Engine: leave `Move::ChainCapture` and `gen_chain_extensions` unchanged.

Client state machine (mirrored across TUI and web):
- **No selection**: cursor moves freely.
- **Piece selected**: highlight legal targets. Clicking a non-capture
  target commits a `Step`. Clicking a capture target enters chain-builder.
- **Chain-builder active**: highlight committed-hops in one color, next-hop
  options in another. The next-hop options are computed by filtering
  legal moves whose path begins with the current builder path:
  - Single `Capture`/`DarkCapture` whose `to` would extend the path → next-hop option.
  - `ChainCapture` whose `path[0..n]` matches the builder → next-hop option(s).
  - Press Enter (or click an empty / non-extending square) to commit.
  - Esc cancels back to original selection.
- **Commit**: find the unique move whose path equals the builder
  (`Capture` for length-1, `ChainCapture` for length≥2). Send to engine
  / network.

`clients/chess-tui/src/app.rs`:
- Extend `GameView` and `NetView` with `chain_builder: Option<ChainBuilder>`.
- `dispatch_select_or_commit` (line 1077) forks: if click is on a capture
  target whose first hop is in legal moves AND there's at least one
  multi-hop `ChainCapture` starting with this hop, enter chain-builder
  instead of committing the 1-step capture.
- Add Action::ChainEnd (mapped to Enter when chain-builder active) to
  commit.
- Render path-so-far with a distinct highlight in `draw_board`.

`clients/chess-web/src/pages/play.rs` + `clients/chess-web/src/state.rs`:
- Mirror the same state machine. Add `chain_builder: RwSignal<Option<…>>`.

In this round, 暗吃 is single-hop ONLY — chain-builder doesn't extend
across `DarkCapture` hops (chain stays in visible territory). The
"true 暗連" (chains-with-hidden-hops) is deferred to Phase 2.

### G. ICCS notation

`crates/chess-core/src/notation/iccs.rs`:
- Encode `Move::DarkCapture { from, to, .. }` → `"<from>x?<to>"`.
- Decode the same form, building `Move::DarkCapture { from, to, revealed: None, attacker: None }`.
- Existing `flip a0` and `a3xb3` syntax unchanged.

### H. Snapshot + chess-web rule picker

- `crates/chess-core/src/snapshot.rs`: existing `house: dark` token works
  unchanged (same bit). Add encoding for `DARK_CAPTURE_TRADE` as
  `dark-trade`.
- `clients/chess-web/src/pages/picker.rs`: add a checkbox for
  `dark-trade`.

## Files to modify

- `crates/chess-core/src/rules/house.rs`
- `crates/chess-core/src/rules/banqi.rs`
- `crates/chess-core/src/state/mod.rs`
- `crates/chess-core/src/moves.rs`
- `crates/chess-core/src/notation/iccs.rs`
- `crates/chess-core/src/snapshot.rs`
- `crates/chess-core/src/coord.rs` (only if `Direction::DIAGONAL` absent)
- `clients/chess-tui/src/app.rs`
- `clients/chess-tui/src/cli.rs` (for `dark-trade` token)
- `clients/chess-web/src/state.rs`
- `clients/chess-web/src/pages/play.rs`
- `clients/chess-web/src/pages/picker.rs`
- `clients/chess-web/src/routes.rs`
- New tests:
  - `crates/chess-core/tests/banqi_first_flip_color.rs`
  - `crates/chess-core/tests/banqi_dark_capture.rs` (probe + trade,
    make/unmake round-trip)
  - `crates/chess-core/tests/banqi_horse_diagonal.rs`

## Deferred (Phase 2 follow-ups)

- **`暗連` chains-with-hidden-hops**: extend `ChainHop` to support
  `Option<Piece>` for dark hops. Bumps wire shape of `Move::ChainCapture`
  → network protocol bump. Skipped this round to keep serde stable.
- **Engine-state chain mode**: replace atomic `Move::ChainCapture` with
  per-hop captures + `state.chain_lock`. Larger turn-semantics refactor
  considered if/when chess-ai needs to reason about chain branches as
  separate plies.

## Verification

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p chess-core
cargo test --workspace
cargo build --target wasm32-unknown-unknown -p chess-core
```

Manual TUI:
```bash
cargo run -p chess-tui -- banqi --house dark,chain,rush,horse --seed 42
# - First flip RED→Black: confirm next moves operate on Black-color pieces
#   (currently they'd incorrectly stay on Red).
# - Select horse, click hidden tile diagonally adjacent (HORSE_DIAGONAL
#   reachable + DARK_CAPTURE): atomic reveal+capture; rank ok → captured.
# - Probe path: select small piece, dark-capture a hidden bigger one →
#   target revealed, your piece stays put, turn ends.
# - Trade path: same with --house dark-trade,... → your small attacker
#   dies, target stays revealed.
# - Chain UX: capture, see next-hop highlight, click again to extend,
#   Enter to commit; Esc cancels mid-chain back to selection.
```

Manual web:
```bash
make play-web
# /local/banqi?house=dark,chain,rush,horse&seed=42 → same checks.
# /local/banqi?house=dark,dark-trade,chain&seed=42 → trade variant.
```

Networked sanity (no protocol bump expected):
```bash
make play-local VARIANT=banqi
# Confirm protocol still v4; both sides serialize/deserialize the new
# Move::DarkCapture cleanly (the new variant adds a serde tag but doesn't
# break older messages).
```
