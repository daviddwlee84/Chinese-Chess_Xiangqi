# TODO

Long-term backlog for Chinese Chess. See AGENTS.md
for the maintenance workflow that agents should follow.

> **For agents**: when the user surfaces an idea explicitly **not** being
> implemented this session (signals: "maybe later", "nice to have",
> "工程量太大需要再評估", "先記下來"), add it here with priority + effort tags.
> Do not create new `ROADMAP.md` / `IDEAS.md` / `BACKLOG.md` files —
> `TODO.md` is the single backlog index. Long-form research goes in
> [`backlog/<slug>.md`](backlog/).

<!-- Use the exact section order: P1, P2, P3, P?, Done.
     The bundled scripts/todo-kanban.sh validator only inspects top-level
     `- [ ]` and `- ✅` items inside these sections. Prose paragraphs,
     blockquotes, indented sub-bullets, HTML comments, and `---` rules are
     ignored — feel free to add inline guidance like this without breaking
     machine readability. -->

## P1

Likely next batch — items you'd reach for if you sat down to work today.

- [ ] **[M] Three Kingdoms Banqi (三國暗棋) implementation** — Build BoardShape::ThreeKingdom mask + rules/three_kingdom.rs with 3-side capture rules. Architecture supports it (TurnOrder, Side, BoardShape variant exist); rules logic and home-zone layout need spec from canonical source. → [research](backlog/three-kingdoms-banqi.md)
- [ ] **[S] Wire DARK_CHAIN house rule** — Chain captures pass through face-down squares; auto-reveal as chain progresses. Requires changing ChainHop.captured to Option<Piece> and updating sanitize_for_observer for the network ABI. See docs/rules/banqi-house.md.
- [ ] **[S] Wire HORSE_DIAGONAL house rule** — Banqi horse moves like xiangqi horse (L-shape with 馬腿 leg block) when flag enabled. Reuse pattern from rules/xiangqi.rs::gen_horse.
- [ ] **[S] Wire CANNON_FAST_MOVE house rule** — Banqi cannon non-capturing slide goes any number of squares (capture still requires the one-piece jump). Reuse the ray walk from rules/banqi.rs::gen_chariot_rush.
- [ ] **[M] Threefold repetition draw detection** — Add Zobrist hashing on Board + (side_to_move) and a position-count map. Update on make_move/unmake_move. Triggers GameStatus::Drawn when count >= rules.draw_policy.repetition_threshold. See state/repetition.rs (currently a stub).

## P2

Worth doing, no rush.

- [ ] **[M] chess-tui: ratatui frontend** — Replace the chess-cli REPL with a real TUI: vim-style cursor (hjkl), click selection, ICCS quick-input, board rendered with ratatui. Mouse + keyboard parity. Connects directly to chess-core for local play.
- [ ] **[M] chess-net: server-authoritative websocket protocol** — tokio + axum + ws. Server runs the authoritative GameState; clients only ever see PlayerView (the no-leak ABI is the whole point of view.rs). Lobby + reconnection out of scope for first cut. → [research](backlog/chess-net-protocol.md)
- [ ] **[M] chess-web: Leptos + WASM frontend** — Same chess-core compiled to wasm32-unknown-unknown (already verified clean). Consumes PlayerView from chess-net. Mouse-first; vim-style keys for parity with TUI.
- [ ] **[S] WXF notation (相3進5 style)** — Chinese-language traditional xiangqi notation. Pattern lives in crates/chess-core/src/notation/wxf.rs (currently empty). ICCS already implemented; mirror the same encode/decode pair.
- [ ] **[M] Time controls (計時器)** — Per-side clocks: main time + increment, byo-yomi, or sudden death. Belongs in chess-net (server-authoritative tick) or as a thin state wrapper if local-only is enough. Adds GameStatus::Won { reason: Timeout } trigger — the WinReason variant already exists. Out of scope: client-side animation; servers tick authoritatively and broadcast remaining time in PlayerView.
- [ ] **[S] Takeback (悔棋) request/approve protocol** — Local hot-seat undo already works via state.unmake_move() and the chess-cli 'undo' command. This TODO is the multiplayer flow: requester sends Takeback, opponent approves/rejects, server unmakes the last N plies (typically 1 or 2) and broadcasts updated PlayerView. Lives in chess-net once that crate exists.

## P3

Someday / nice-to-have.

- [ ] **[L] chess-engine + chess-ai: search and evaluation** — Alpha-beta + iterative deepening + Zobrist transposition for xiangqi. Banqi needs ISMCTS (information-set MCTS) to handle hidden info — research-heavy and a separate problem from xiangqi search. → [research](backlog/chess-ai-search.md)
- [ ] **[M] Large-board variants (大盤)** — BoardShape::Custom already exists with u128 mask (sufficient up to 128 cells; widen to two u128s if some variant exceeds that). Need per-variant setup and rules. Candidates: 廣象戲 (19×19), 13×14 modern variants. Pick one canonical first.
- [ ] **[S] Replay format (PGN-equivalent)** — Once notation is full (WXF + ICCS + banqi), the history Vec<MoveRecord> already contains everything. Add a serializer to text format + import. Unblocks observers / spectators / training data.
- [ ] **[S] insta snapshot tests for initial positions** — Lock in the visual rendering of fresh xiangqi/banqi positions. Catches regressions in setup.rs that pass type-checks but place pieces wrong. Plan flagged this; deferred from PR 1 because unit tests in setup.rs already cover key invariants.

## P?

Needs a spike before committing to a real priority. Tag as `[?/Effort]`.

- [ ] **[?/L] Wish/SSH multiplayer alternative to chess-net** — Original analysis (docs/architecture.md) noted Go+wish would give 'ssh chess.you.dev' for free. Rust equivalent: russh (less polished). Spike: is this simpler than chess-net + chess-web for the 'play with friends' scenario, or just neat?
- [ ] **[?/L] Unity renderer via FFI** — Architecture deliberately keeps chess-core platform-agnostic so Unity is reachable. Spike: does building a chess-core C ABI + Unity importer get us anything beyond 'Web with art assets via Leptos + sprites'? Likely not, but documenting the option closes the loop.

## Done

- ✅ [2026-05-06] [P1/S] End-condition unit tests (checkmate / stalemate / banqi no-moves / no-progress) — tests/end_conditions.rs verifies refresh_status produces Won{Checkmate} / Won{Stalemate} / Won{OnlyOneSideHasPieces} from .pos fixtures. Pattern reusable for endgame puzzles.

Recently shipped. When implementing an active item, in the same commit run:

```
scripts/promote-todo.sh --title "<substring>" --summary "<one-line shipped summary>"
```

This moves the entry here using the dated `Done` syntax and re-validates.

- ✅ [2026-05-06] [P1/L] Foundational chess-core + workspace scaffolding — full xiangqi (perft-validated 1=44, 2=1920, 3=79666) + base banqi + CHAIN_CAPTURE/CHARIOT_RUSH house rules + PlayerView projection (no-leak proptest) + ICCS notation + chess-cli REPL. 8-crate workspace + WASM cleanliness in CI. 71 tests passing.

<!-- Prune older entries into CHANGELOG.md once prior-year items appear here
     or this section grows past ~20 entries. -->
