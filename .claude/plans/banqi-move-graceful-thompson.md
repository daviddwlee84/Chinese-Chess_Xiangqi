# Banqi defers colour until first flip + host-side game-setup presets

## Context

Today's game-room behaviour is hard-coded:

- **Banqi** — `setup.rs::build_banqi` always seeds `side_to_move = Side::RED` and `side_assignment = None`. The first room-seat (RED) is forced to make the first reveal; only after that flip does `state.side_assignment` lock in who plays which colour (Taiwan rule: flipper plays the colour they reveal, see `state/mod.rs::banqi_side_assignment`).
- **Xiangqi** — the first joiner is always seated as RED (`chess-net/src/room.rs::next_seat` line 198, and `chess-web/src/host_room.rs:171` for the WebRTC variant).
- **chess-net WS server** — rooms inherit a single CLI-baked `RuleSet`. No per-room customisation.
- **chess-web `/lan/host`** — host picks variant + `HouseRules`, but cannot pick which colour they play nor (for banqi) who flips first.

The user wants two related changes:

1. **Banqi defaults to "either seat flips first"** — no colour pre-commitment until a real flip. Pre-assigning colours becomes an opt-in `HouseRules` flag.
2. **Host-side game-setup presets** — when creating a room (either via WebRTC `/lan/host` or chess-net WS), the host can pre-configure:
   - **Banqi first-flipper**: `Either` (default) / `Host` / `Joiner`
   - **Xiangqi host colour**: `Red` (default) / `Black` / `Random`

Both must work on both deployment paths: WebRTC peer-to-peer LAN play and the central chess-net WS server.

## Approach

The work splits into 7 phases. Phases 1–2 are the core "either flips first" behaviour and are independently shippable. Phases 3–5 add the host-preset UI/protocol surface across both deployments. Phases 6–7 are display tweaks and docs.

### Phase 1 — Engine: banqi defers colour until first flip

`crates/chess-core/src/`:

- `rules/house.rs`: add bit `PREASSIGN_COLORS = 1 << 6`. Banqi-only semantics. Default OFF.
- `state/mod.rs`: helper
  ```rust
  pub fn banqi_awaiting_first_flip(&self) -> bool {
      matches!(self.rules.variant, Variant::Banqi)
          && self.side_assignment.is_none()
          && !self.rules.house.contains(HouseRules::PREASSIGN_COLORS)
  }
  ```
  No change to `make_move` — `banqi_side_assignment(flipper, revealed)` already reads `state.side_to_move` as the flipper, so the deployment layer just sets `side_to_move = <clicker's seat>` before calling `make_move` on the first reveal.
- `view.rs::PlayerView`: add `#[serde(default)] pub banqi_awaiting_first_flip: bool` (no `PROTOCOL_VERSION` bump — older payloads default to `false`). In `project`:
  ```rust
  let awaiting = state.banqi_awaiting_first_flip();
  let legal_moves = if observer == state.side_to_move || awaiting {
      sanitize_for_observer(state.legal_moves(), observer)
  } else { MoveList::new() };
  ```
  Both observers see reveals when awaiting; otherwise the existing side-of-move gate stands.

Tests in `crates/chess-core/tests/banqi_first_flip_color.rs`:
- Keep the existing "RED flips, locks side assignment" path but parameterise the rule set with `HouseRules::PREASSIGN_COLORS` so it documents the legacy mode.
- Add `default_rules_let_black_seat_flip_first`: build state, force `side_to_move = Side::BLACK`, apply `Move::Reveal`, assert `side_assignment` resolves with BLACK as the flipper.
- Add `view_pre_first_flip_exposes_legal_moves_to_both_observers`: `PlayerView::project` from both RED and BLACK sees the 32 reveals; after the first flip only the new `side_to_move` does.
- Add `view_with_preassign_colors_only_lets_red_see_legal_moves`: same setup with `PREASSIGN_COLORS` set, opposite assertion.

### Phase 2 — Deployment: server-side guard relax

Goal: let either seat make the first banqi reveal. No protocol message changes; only relax the existing "not your turn" guard.

`crates/chess-net/src/room.rs::process_move` (line 435):

```rust
let is_first_banqi_reveal = matches!(mv, Move::Reveal { .. })
    && self.state.banqi_awaiting_first_flip();
if is_first_banqi_reveal {
    self.state.side_to_move = seat;          // attribute flip to the actual clicker
} else if self.state.side_to_move != seat {
    out.push(Outbound { peer: from,
        msg: ServerMsg::Error { message: "not your turn".into() } });
    return;
}
// existing make_move / broadcast_update continues
```

Same logic for `clients/chess-web/src/host_room.rs`'s peer-message router (HostRoom wraps the same `Room::apply` path, so once `room.rs` is patched both deployments inherit the fix).

Add a chess-net integration test: two peers join a banqi room with default rules; the second (BLACK seat) sends `Reveal` first; assert it succeeds, `Update` broadcasts to both, and the new `current_color` reflects BLACK-seat-plays-revealed-colour. Add the negative test: same setup with `?preassign=1` URL param (introduced in Phase 3); BLACK's first reveal returns `Error { "not your turn" }`.

### Phase 3 — Per-room config: protocol surface

A new `RoomConfig` struct carries host presets through both deployments. Fields:

```rust
pub struct RoomConfig {
    pub rules: RuleSet,                          // variant + house bits
    pub host_color: Option<Side>,                // None = system default (RED)
    pub first_flipper: FirstFlipper,             // Either (default) / Host / Joiner
}

pub enum FirstFlipper { Either, Host, Joiner }
```

`FirstFlipper` only applies to banqi when `PREASSIGN_COLORS` is OFF. Otherwise the colour is fixed and the engine's existing "Red moves first" rule decides.

#### Wire formats

**chess-net WS URL params** (extending the existing `?password=` precedent):
- `?host_color=red|black|random`
- `?first_flipper=either|host|joiner`
- `?preassign=1`

Parsed inside `room.rs::join_player` only when the room is first created (subsequent joiners' params are ignored — the room's config is locked). Persisted on `RoomState` for the lobby `Rooms` snapshot.

**chess-web `/lan/host` → `/lan/join` offer envelope** (`transport/webrtc.rs::encode_sdp` line 431):

Extend the JSON envelope to carry an optional config payload:
```rust
pub fn encode_sdp_v2(kind: &str, sdp: &str, config: Option<&RoomConfig>) -> String { … }
```
Old envelopes deserialise via `serde(default)` and yield `config = None` → falls back to the engine's default rules (back-compat with any in-flight invitations from the previous version).

#### chess-net seat assignment

`room.rs::next_seat` currently always returns RED first, BLACK second. Update to:
```rust
pub fn next_seat(&self) -> Option<Side> {
    match self.seats.len() {
        0 => Some(self.config.host_color.unwrap_or(Side::RED)),
        1 => Some(self.seats[0].0.opposite()),
        _ => None,
    }
}
```
`host_color = Random` is resolved to a concrete `Side` once at room creation (server uses `ChaCha8Rng` for determinism with `?random_seed=...`, or `thread_rng` otherwise).

#### Initial side_to_move (banqi only)

After `build_banqi` runs (`side_to_move = Side::RED` by default), the room layer overrides:
```rust
match config.first_flipper {
    FirstFlipper::Either => { /* leave side_to_move untouched; banqi_awaiting_first_flip() == true allows either */ }
    FirstFlipper::Host   => state.side_to_move = config.host_color.unwrap_or(Side::RED),
    FirstFlipper::Joiner => state.side_to_move = config.host_color.unwrap_or(Side::RED).opposite(),
}
```

### Phase 4 — chess-web `/lan/host` picker UI

`clients/chess-web/src/pages/lan.rs::LanHostPage`:

- After the existing variant radio + house-rule checkboxes, render new fields:
  - **Xiangqi** (and banqi when `PREASSIGN_COLORS` checked): "I play as: Red / Black / Random" radio. Default Red.
  - **Banqi** (when `PREASSIGN_COLORS` unchecked): "First flipper: Either / Host / Joiner" radio. Default Either.
- Bind to a new signal `chosen_config: RwSignal<RoomConfig>`; replace the existing `chosen_rules` commit path with one that builds the full `RoomConfig`.
- Pass `Some(&config)` into the new `encode_sdp_v2` envelope so the joiner sees the host's choices before accepting.
- `HostRoom::new` signature extends to `new(config: RoomConfig, password: Option<String>, hints: bool)`; the line-171 seat assignment uses `config.host_color`.

`clients/chess-web/src/pages/lan.rs::LanJoinPage`:

- Parse the offer envelope; show the host's chosen variant + rules + colour preferences as a read-only summary above the "Accept invite" button. ("Host plays Black — you will be Red and move first." style copy.)

### Phase 5 — chess-tui room-creation flow

`clients/chess-tui/src/`:

- The lobby's "create room" prompt (`c` key in lobby) currently takes only a room name. Extend it to a multi-line form:
  - Variant (radio)
  - Strict / casual (xiangqi)
  - House rules (banqi)
  - Host colour (xiangqi + banqi-preassign)
  - First flipper (banqi-no-preassign)
- The resulting `RoomConfig` is encoded into the URL query string when connecting: `ws://host/ws/room?host_color=black&first_flipper=joiner&preassign=1`.
- Direct `--connect` invocation: add CLI flags `--host-color`, `--first-flipper`, `--preassign` that emit the same query string.

### Phase 6 — Pre-first-flip sidebar text

`clients/chess-tui/src/ui.rs` (sidebar lines 1876–1893) and `clients/chess-web/src/pages/play.rs` (lines 608–637) plus `pages/local.rs`:

- When `view.banqi_awaiting_first_flip` is true, replace "Red to move" / "Your turn:" / "Opponent:" with a single neutral string. Same code path serves Local and Net.
- **Copy**: `Awaiting first flip — either side may flip` (English); `未翻牌 — 任一方皆可先翻` (CJK glyph fallback). One helper per client, kept locally — no new shared crate per the existing `orient.rs` / `glyph.rs` duplication policy.
- Web click guard at `pages/play.rs:464-467`: when `v.banqi_awaiting_first_flip`, allow the click regardless of `v.observer == v.side_to_move`. Server is still authoritative.

### Phase 7 — Documentation

- `docs/rules/banqi.md`: new section **First-flip & colour assignment** describing the default ("either side may flip; revealed colour determines mapping per Taiwan rule") and the legacy `PREASSIGN_COLORS` mode.
- `docs/rules/banqi-house.md`: add a `PREASSIGN_COLORS` row (`1 << 6`, default OFF, banqi-only).
- `docs/adr/`: new ADR-000N **Per-room host presets via URL params + offer envelope** capturing why we chose URL params over a `ClientMsg::CreateRoom` message (back-compat, simpler, no protocol-version bump on the existing message types).

## Phasing & shippability

Each phase is mergeable on its own:

- **P1 alone**: engine gains `PREASSIGN_COLORS` bit + helper + view field. Tests pass. No user-visible change because the deployment layer still always uses `side_to_move = RED`.
- **P1 + P2**: net+host-room let either seat flip first in banqi default rules. UI still says "Red to move" pre-flip (cosmetic). New TUI/web sidebar copy can land in P6.
- **P3 alone (on top of P2)**: protocol surface for host config — server seat assignment honours `host_color`; banqi first-flipper preference flows through.
- **P4/P5**: UI to drive P3.

If sequencing pressure exists, P6 (sidebar text) can land with P2 since it doesn't depend on P3+.

## Critical files

| Path | Change |
|---|---|
| `crates/chess-core/src/rules/house.rs` | add `PREASSIGN_COLORS = 1 << 6` |
| `crates/chess-core/src/state/mod.rs` | add `banqi_awaiting_first_flip()` |
| `crates/chess-core/src/view.rs` | add `banqi_awaiting_first_flip` field; relax `legal_moves` gate |
| `crates/chess-core/tests/banqi_first_flip_color.rs` | split tests; add BLACK-flips-first + view-exposure cases |
| `crates/chess-net/src/protocol.rs` | add `RoomConfig` + `FirstFlipper` types |
| `crates/chess-net/src/room.rs:198, 257, 435` | per-room config; relaxed `process_move` guard; `next_seat` honours `host_color`; banqi `side_to_move` init from `first_flipper` |
| `crates/chess-net/src/server.rs:125, 412` | parse new URL params on first `join_player` |
| `crates/chess-net/tests/...` | integration: BLACK-flips-first (default), `?preassign=1` rejects it, `?host_color=black` swaps seats |
| `clients/chess-web/src/transport/webrtc.rs:431` | extend offer envelope with optional `RoomConfig` |
| `clients/chess-web/src/host_room.rs:171` | `HostRoom::new(config, …)` honours `host_color` |
| `clients/chess-web/src/pages/lan.rs` | new picker controls; commit `RoomConfig`; join-page shows host's choices |
| `clients/chess-web/src/pages/play.rs:464-467, 608-637` | sidebar copy + click-guard relax |
| `clients/chess-web/src/pages/local.rs` | sidebar copy |
| `clients/chess-web/src/pages/picker.rs` | "Pre-assign colors" checkbox in banqi card |
| `clients/chess-web/src/routes.rs` | add `preassign` token to `?house=` |
| `clients/chess-tui/src/ui.rs:1876-1893` | sidebar copy |
| `clients/chess-tui/src/lobby.rs` (or equivalent) | "create room" form extension |
| `clients/chess-tui/src/main.rs` | CLI flags `--host-color`, `--first-flipper`, `--preassign` |
| `docs/rules/banqi.md`, `docs/rules/banqi-house.md` | new sections |
| `docs/adr/000N-room-config-via-url.md` | ADR for URL-param-based config |

## Reused primitives

- `state.banqi_side_assignment(flipper, revealed)` (`state/mod.rs:319-332`) already encodes the Taiwan rule — once the deployment layer sets `side_to_move = clicker_seat`, mapping is correct.
- `routes.rs::parse_local_rules` (URL token round-trip) extends by one token.
- `ChaCha8Rng` (already imported for `banqi_with_seed`) handles the deterministic side-coin-flip for `host_color = Random`.
- `HostRoom` already routes peer `ClientMsg`s through `Room::apply` — patching `room.rs` lights up both WebRTC and WS deployments.

## Verification

```bash
# 1. Engine: new tests pass, nothing regresses
cargo test -p chess-core banqi_first_flip
cargo test --workspace

# 2. Server: integration tests for the relaxed guard + URL-param config
cargo test -p chess-net

# 3. Format / lint / WASM
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --target wasm32-unknown-unknown -p chess-core
cargo build --target wasm32-unknown-unknown -p chess-web

# 4. End-to-end (TUI, two peers, banqi default)
make play-local VARIANT=banqi
# BLACK-seat terminal: press `f` on any hidden tile → flip succeeds.
# Sidebars say "Awaiting first flip" until the flip resolves.

# 5. End-to-end (TUI, host-config presets)
cargo run -p chess-net -- --port 7878 banqi &
cargo run -p chess-tui -- --connect 'ws://127.0.0.1:7878/ws/r?host_color=black&first_flipper=joiner'
cargo run -p chess-tui -- --connect 'ws://127.0.0.1:7878/ws/r'
# Host (first to connect) seated as BLACK; joiner seated as RED.
# Joiner is forced to flip first per `first_flipper=joiner`.

# 6. End-to-end (chess-web LAN host)
make play-web
# Browser 1: /lan/host → pick banqi, "Joiner flips first" → generate invite
# Browser 2: /lan/join → paste invite → join-page summary shows host's choice
# After WebRTC connection, Browser 2 (joiner) flips first; Browser 1 must wait.

# 7. End-to-end (legacy mode)
cargo run -p chess-net -- --port 7878 banqi
cargo run -p chess-tui -- --connect 'ws://127.0.0.1:7878/ws/r?preassign=1'
# Second client (BLACK seat) attempts `f` first → server returns
# Error{"not your turn"}, UI surfaces it. Matches today's behaviour.
```

Post-merge: in the same commit run
`scripts/promote-todo.sh --title "<closest TODO substring>" --summary "Banqi default = either-seat-flips; host can pre-configure colour + first flipper"`
if there's a matching TODO entry; otherwise the change ships standalone.
