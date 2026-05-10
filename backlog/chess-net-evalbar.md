# chess-net protocol v6: broadcast win-rate samples + spectator-side eval bar

**Status**: P2 / not started
**Effort**: M (1-2 dev sessions; protocol bump + per-room AI search infra)
**Related**: [`../docs/ai/v5-id-tt.md`](../docs/ai/v5-id-tt.md) (search the broadcasted samples come from),
[`../TODO.md`](../TODO.md) (P3 PWA persistence — also wants `Vec<EvalSample>` shape stable)

## Why this is in backlog

Local mode + chess-tui shipped the AI win-rate display on 2026-05-10:

- `?evalbar=1` (web) / `--evalbar` (TUI) flag enables sampling
- Eval bar (right side of board) + sidebar `紅 % • 黑 %` badge during play
- SVG line chart at game end showing the full trend
- Backed by a `Vec<EvalSample>` that piggy-backs on the existing
  `chess_ai::analyze` calls in the AI move pump and hint pump

Net mode (`/play/<room>`) intentionally was **not** included in v1 to
keep PR scope tight and to allow the design questions below to be
resolved independently of the local-mode UX work. This doc captures
the design questions for net mode so the v6 follow-up can pick up
without re-exploring.

## Scope

Make the eval bar / sidebar badge / end-game chart render in
`pages/play.rs` (chess-web) and chess-tui's net mode, gated by the
existing `hints_allowed` per-room flag (no new permission knob).

## Design choices to resolve before code

### 1. Where does the analyze run?

Three options:

**A. Server-side** — the chess-net server runs `chess_ai::analyze` once
per move, broadcasts `cp` + `win%` in `Update`. Pros: one source of
truth, consistent across spectators, free for clients. Cons: server
CPU cost (currently chess-net is pure routing; adding AI is a sizeable
dependency bump on the server binary). Server can't analyze without a
GameState — but server already owns one (`RoomState.state`).

**B. Player-side** — each connected player runs `chess_ai::analyze`
locally on their `PlayerView`-reconstructed state. Same code path
already used by `pages/play.rs`'s `?debug=1` debug panel. Pros: no
server change required (the wire format only adds an opt-in flag).
Cons: each spectator pays the CPU cost; numbers diverge across slow
vs. fast machines (different `node_budget`-truncated depths produce
different cp values).

**C. Hybrid** — server runs analyze and broadcasts `cp`; clients render
locally without re-analyzing. Cons: still requires chess-ai dep on
server. Pros: same as A but client controls the rendering knobs
(K constant, color scheme).

**Recommendation**: **A or C** (server-side analyze). Per-spectator
divergence (B) is a UX bug — two spectators watching the same game
shouldn't see different "win rates". Server cost is bounded (one
analyze per move, max ~300 ms per room — rooms are bursty during play
and idle otherwise; a busy server with 100 active games at once
≈ 30 s/s CPU which fits comfortably in one core).

### 2. Wire format

Two sub-options:

**a. Per-`Update` push**: every server-side `Update` carries the
single fresh sample. Spectators that join mid-game miss earlier
samples. They could ask for backfill via a separate `History` request,
or accept that joining mid-game shows an incomplete chart.

**b. Full vector in `Hello` / `Spectating` + per-`Update` push**:
joiners get the full backlog; live updates are incremental. Larger
`Hello` payload (typical 30-ply game ≈ 30 × 12 bytes for `EvalSample`
≈ 360 bytes; trivially small).

**Recommendation**: **b**. The backlog vector is the source of truth
for the chart; per-update push is just the live-feed path.

### 3. Permission gating

Current room-creation flow has `hints_allowed: bool` (set by first
joiner's `?hints=1` query param, frozen for the room's lifetime).
Two sub-options for the eval display:

**a. Reuse `hints_allowed`** — same flag controls both the AI hint
panel and the eval bar. Simpler UX, matches the spirit ("AI insight
allowed in this room or not").

**b. New `evalbar_allowed`** — separate toggle for finer control
(maybe a serious match wants no hints but spectators can see
neutral cp/win% display). Cons: more checkboxes in the
create-room form; the distinction is subtle.

**Recommendation**: **a**. If a room operator wants AI insights off,
the eval bar leaks the same information at lower bandwidth.

### 4. Strategy / depth defaults

The server's analyze should use a stable, modest config:

- `Strategy::default()` (currently v5)
- `Difficulty::Normal` (depth 3) — fast enough for per-move broadcast
- `Randomness::STRICT` (deterministic; same position always reports
  the same cp regardless of when it's analyzed)

Power users tuning depth via `?depth=N` URL params should NOT
override the server's eval depth — that's a private knob. Eval bar
shows what the *server* thinks, not what each spectator's
hypothetical analysis would say.

## Wire schema sketch

```rust
// crates/chess-net/src/protocol.rs (PROTOCOL_VERSION 5 → 6)

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvalSample {
    pub ply: u16,
    pub cp_red_pov: i32,        // signed: positive = red advantage
    pub red_win_pct: f32,       // pre-computed; spectators can render directly
}

pub struct RoomSummary {
    // ... existing fields ...
    #[serde(default)]
    pub eval_samples: Vec<EvalSample>,  // empty if !hints_allowed
}

pub enum ServerMsg {
    // ... existing variants ...
    Update {
        // ... existing fields ...
        #[serde(default)]
        new_eval: Option<EvalSample>,   // None if !hints_allowed
    },
}
```

`#[serde(default)]` everywhere so v5 clients can still talk to a v6
server (they just don't see eval samples).

## Tasks

1. Bump `PROTOCOL_VERSION` to 6 in `chess-net/src/protocol.rs`
2. Add `EvalSample` to protocol; add fields per schema above
3. Add `chess-ai` dep to chess-net (server only — client doesn't need
   it for eval display since samples are pre-computed)
4. Server: run analyze in `RoomState::apply_move` after each move,
   push to `Vec<EvalSample>`, broadcast new sample in `Update`
5. Server: include backlog in `Hello` / `Spectating` payloads
6. Client: parse new fields in `pages/play.rs`, plumb through
   `state::ClientView`, mount the same `<EvalBar>` / `<EvalChart>`
   components from local mode
7. chess-tui: same plumbing — read `EvalSample` from server,
   render the same ASCII bar / chart helpers
8. Tests: protocol round-trip, server-side analyze regression,
   client renders empty when `hints_allowed=false`

## Risks / unknowns

- **Server-side WASM-vs-native cp drift**: chess-ai's v5 search uses
  the same Zobrist seeds + node budget on both targets, so cp
  numbers should be bit-identical. But this is an implicit assumption
  worth a regression test (both targets analyze the same fixture,
  assert same cp).
- **Reconnect handling**: a player reconnecting mid-game gets the
  full backlog in `Hello` — but their UI may be in mid-render. Make
  sure the reactive effect that builds the chart handles a "full
  vector arrives at once" event, not just incremental push.
- **Server CPU on big lobbies**: 100 games × analyze ≈ 30 s/s on one
  core at default depth 3. Acceptable for friend-server scale; would
  need throttling for public deployments. Tag as a future TODO if
  observed.

## When to revisit

Once a friend-server deployment actually has multiple concurrent
games and players want eval display in net mode. Currently chess-net
deploys are 1-2 concurrent rooms; local mode covers that audience
fine. Re-prioritize to P1 if multi-room production deployment becomes
a real workflow.
