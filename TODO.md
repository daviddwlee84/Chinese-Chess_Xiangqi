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

- [ ] **[S] chess-web: in-game rule editor (gear icon → modal)** — Picker-side form already encodes rules into the URL (`?strict=1`, `?house=chain,rush&seed=N`). Add a sidebar gear that opens a pre-filled modal reusing the picker components, navigates to the new query, and starts a fresh game. → [research](backlog/web-ingame-rules-modal.md)
- [ ] **[S] chess-web: WS auto-reconnect with backoff** — PR-1 surfaces "Disconnected — refresh" when the WS read pump closes. Add an exponential-backoff retry that reuses the URL+password and resets the `Hello` flow on success. → [research](backlog/web-ws-reconnect.md)
- [ ] **[S] chess-web: CJK/ASCII glyph toggle in UI** — `glyph::Style` is already plumbed; expose a sidebar toggle + persist in `localStorage`. Shares no logic with chess-tui's `--style` flag.
- [ ] **[M] Promote orient.rs / glyph.rs to a shared client crate** — Both `clients/chess-tui/src/orient.rs` and `clients/chess-web/src/orient.rs` (likewise `glyph.rs`) are byte-identical. Pull into `clients/chess-client-shared` once a third client appears or the copies first diverge. → [research](backlog/promote-client-shared.md)
- [ ] **[S] chess-web: Playwright E2E smoke** — Headless two-tab flow that creates a room, plays a few moves, resigns, and rematches. Cheaper than `wasm-bindgen-test` for full-app smoke. → [research](backlog/web-playwright.md)
- [ ] **[M] chess-net: mixed-variant rooms** — Today one server boots with one `--variant` and every auto-created room copies it. Move to per-room `RuleSet` (a chooser inside the `CreateRoom` form). Architecture supports it (`RoomState` already owns its `GameState`); blocks on UX decisions about a variant filter inside the lobby and what default to apply when joining via a URL with no variant hint.
- [ ] **[M] chess-net: spectator slots** — Third+ connection currently gets `room full`. Replace with a read-only spectator role: receives `Update` like the seats but `Move`/`Resign`/`Rematch` get `Error{"spectators can't play"}`. Adds `RoomSummary.spectators: u8` and a watcher count to the lobby UI.
- [ ] **[S] chess-net: lobby filter + sort** — `/` filter inside the lobby (by id substring or variant); sort by recency / seat count / locked-or-open. Defer until ≥10 concurrent rooms is a real workflow — for a handful of friends the unsorted live list works fine.
- [ ] **[S] chess-net: lobby push debouncing** — `notify_lobby` fires on every seat insertion / removal / status flip; back-to-back churn (rematch storms, reconnect retries) can produce a flurry of identical `Rooms` payloads. Coalesce within ≥250ms windows. Premature today; revisit if observable in the wild.
- [ ] **[S] chess-net: server-side admin / kick / signed token** — Close stuck rooms or force-disconnect a misbehaving client without restarting the whole server. Wants either an admin-only ws path (`/admin/ws?token=…`) or signed JSON tokens accepted by `/ws/<id>`. Folds into a future TLS / auth pass.
- [ ] **[S] WXF notation (相3進5 style)** — Chinese-language traditional xiangqi notation. Pattern lives in crates/chess-core/src/notation/wxf.rs (currently empty). ICCS already implemented; mirror the same encode/decode pair.
- [ ] **[M] Time controls (計時器)** — Per-side clocks: main time + increment, byo-yomi, or sudden death. Belongs in chess-net (server-authoritative tick) or as a thin state wrapper if local-only is enough. Adds GameStatus::Won { reason: Timeout } trigger — the WinReason variant already exists. Out of scope: client-side animation; servers tick authoritatively and broadcast remaining time in PlayerView.
- [ ] **[S] Takeback (悔棋) request/approve protocol** — Local hot-seat undo already works via state.unmake_move() and the chess-cli 'undo' command. This TODO is the multiplayer flow: requester sends Takeback, opponent approves/rejects, server unmakes the last N plies (typically 1 or 2) and broadcasts updated PlayerView. Lives in chess-net once that crate exists.
- [ ] **[S] Handicap mode (讓棋)** — Stronger player removes one or more of their own pieces from the initial setup before play begins (xiangqi tradition: 讓單馬 / 讓雙馬 / 讓單炮 / 讓單車 / 讓雙馬一炮 / 讓雙馬雙炮 / etc). Shape likely: Handicap field on RuleSet (so save/load + replay metadata preserve the choice) consumed by setup::build_xiangqi which skips placing the listed pieces. Xiangqi-only for first cut — banqi's random shuffle complicates the equivalent and can wait until someone asks.

## P3

Someday / nice-to-have.

- [ ] **[L] chess-engine + chess-ai: search and evaluation** — Alpha-beta + iterative deepening + Zobrist transposition for xiangqi. Banqi needs ISMCTS (information-set MCTS) to handle hidden info — research-heavy and a separate problem from xiangqi search. → [research](backlog/chess-ai-search.md)
- [ ] **[S] chess-web: move animations** — CSS transitions on piece translate-translate as a `<g transform>` interpolation. Cheap visual polish; doesn't change protocol. Care needed: the SVG renders pieces by display row/col, so animation needs to track piece identity across renders (key-by-square is wrong for moves).
- [ ] **[S] chess-web: history scrubber + undo via UI** — Engine already has `unmake_move` and `Replay::play_to`. Need a sidebar timeline that replays positions visually without committing to engine state. Reuse `Replay::from_game` so server replays + client scrubbing share one path.
- [ ] **[S] chess-web: mobile layout** — Sidebar collapses below the board on narrow viewports; piece tap target enlarged. Touch input already works via SVG `on:click`; this is purely CSS + viewport meta.
- [ ] **[S] chess-web: PWA / offline cache** — Service worker caches `dist/` so local pass-and-play works offline. Online play needs the server, so PWA only helps the local-only flows.
- [ ] **[S] chess-web: i18n strings table** — All copy is currently inline English+CJK. Pull into a strings module keyed by locale, switch via `?lang=zh-Hant` or browser locale.
- [ ] **[S] Replay format (PGN-equivalent)** — Once notation is full (WXF + ICCS + banqi), the history Vec<MoveRecord> already contains everything. Add a serializer to text format + import. Unblocks observers / spectators / training data.
- [ ] **[S] insta snapshot tests for initial positions** — Lock in the visual rendering of fresh xiangqi/banqi positions. Catches regressions in setup.rs that pass type-checks but place pieces wrong. Plan flagged this; deferred from PR 1 because unit tests in setup.rs already cover key invariants.

## P?

Needs a spike before committing to a real priority. Tag as `[?/Effort]`.

- [ ] **[?/L] Wish/SSH multiplayer alternative to chess-net** — Original analysis (docs/architecture.md) noted Go+wish would give 'ssh chess.you.dev' for free. Rust equivalent: russh (less polished). Spike: is this simpler than chess-net + chess-web for the 'play with friends' scenario, or just neat?
- [ ] **[?/L] Unity renderer via FFI** — Architecture deliberately keeps chess-core platform-agnostic so Unity is reachable. Spike: does building a chess-core C ABI + Unity importer get us anything beyond 'Web with art assets via Leptos + sprites'? Likely not, but documenting the option closes the loop.
- [ ] **[?/M] WebRTC peer-to-peer fallback for chess-web** — Lets two browsers play without chess-net hosting. Needs a signalling step (could reuse chess-net briefly for handshake). Big UX delta: NAT traversal failures are the user's problem, not the server's. Worth it only if hosting cost ever becomes a concern.

## Done

- ✅ [2026-05-07] [P2/M] chess-web: Leptos + WASM frontend — `clients/chess-web` ships as a single-page Leptos+Trunk SPA covering all three variants (xiangqi local + online; banqi local + online; three-kingdom renders the empty 4×8 stub with a "WIP" overlay per CLAUDE.md). SVG-only rendering — no art assets — code-as-presentation, mirroring chess-tui's intersection layout. Both modes share one `<Board>` component: local mode owns `GameState` and projects `PlayerView` after every move; online mode receives `PlayerView` via `gloo-net` WS to `chess-net`'s `/ws/<room>` and `/lobby` endpoints. `orient.rs` + `glyph.rs` are byte-copies of the chess-tui modules with cloned tests (drift caught by both crates' CI). chess-net gains a `static-serve` feature (default-on) and `--static-dir <path>` flag that mounts `tower_http::services::ServeDir` as the route fallback so a single binary serves the SPA + WS in production; chess-web depends on chess-net with `default-features = false` so axum/tokio do not leak into the WASM build. New `make play-web` / `make build-web` / `make stop-web` targets + `scripts/play-web.sh` mirror the existing tmux harnesses. Trunk dev-server proxies `/ws*` and `/lobby` to chess-net at :7878. Reconnect / mobile layout / move animations / Playwright E2E / promote-orient-glyph-to-shared-crate are separate follow-ups in P2/P3.

- ✅ [2026-05-06] [P2/M] chess-net: multi-room lobby + optional password — `crates/chess-net` server now keys rooms by string id (`Arc<Mutex<HashMap<String, Arc<Mutex<RoomState>>>>>`). New routes: `GET /ws/<id>` (auto-creates the room), `GET /lobby` (live `ServerMsg::Rooms` push on every state change), `GET /rooms` (JSON snapshot for `curl`). Optional `?password=<secret>` query param locked in by the first joiner; mismatched joiners get `Error{"bad password"}` before any `Hello` and are dropped. Default room `main` (served from `/` and `/ws`) is permanent so v1 clients keep working. `chess-tui` adds `Screen::HostPrompt`, `Screen::Lobby`, `Screen::CreateRoom` plus the `Connect to server…` picker entry; `--lobby <ws>` and `--password <pw>` flags. Lobby uses a second sync `tungstenite` worker — no tokio added to the TUI. New ADR-0005, `make play-lobby` launcher, 8 server-smoke tests, 3 protocol round-trips. Mixed-variant servers / spectators / lobby filter / debouncing / admin tokens / TLS are separate P2 follow-ups.

- ✅ [2026-05-06] [P2/M] chess-net MVP: single-room websocket server + chess-tui --connect + tmux launcher — `crates/chess-net` ships an axum-ws server (`chess-net-server`) hosting one game/room. Wire schema: tagged JSON enums `ServerMsg` (Hello/Update/Error) and `ClientMsg` (Move/Resign), `Move::Reveal` stays `revealed: None` end-to-end. First connection = Red, second = Black, third gets "room full". `chess-tui --connect ws://...` runs a sync `tungstenite` worker thread + `std::sync::mpsc` channels (no tokio in TUI). `make play-local` / `scripts/play-local.sh` opens a tmux session with two clients in window 0 and the server in window 1. 8 new tests (6 protocol round-trip + 2 end-to-end smoke). Lobby / reconnect / time controls / takeback still deferred (kept as separate TODO items).

- ✅ [2026-05-06] [P2/M] chess-tui: ratatui frontend — interactive TUI for xiangqi + banqi. Vim cursor (hjkl) + arrows + mouse, Enter/Space select-or-commit, `u` undo, `f` flip (banqi). Two render styles: CJK glyphs (帥仕相俥傌炮兵 / 將士象車馬砲卒, default with color) and ASCII (--style ascii). Banqi displays in traditional 8×4 layout (transposed from 4×8 model). Variant picker on bare invocation; --as black flips xiangqi orientation for net-readiness. Engine stays presentation-free — orient.rs + glyph.rs live in the client.

- ✅ [2026-05-06] [P1/S] End-condition unit tests (checkmate / stalemate / banqi no-moves / no-progress) — tests/end_conditions.rs verifies refresh_status produces Won{Checkmate} / Won{Stalemate} / Won{OnlyOneSideHasPieces} from .pos fixtures. Pattern reusable for endgame puzzles.

Recently shipped. When implementing an active item, in the same commit run:

```
scripts/promote-todo.sh --title "<substring>" --summary "<one-line shipped summary>"
```

This moves the entry here using the dated `Done` syntax and re-validates.

- ✅ [2026-05-06] [P1/L] Foundational chess-core + workspace scaffolding — full xiangqi (perft-validated 1=44, 2=1920, 3=79666) + base banqi + CHAIN_CAPTURE/CHARIOT_RUSH house rules + PlayerView projection (no-leak proptest) + ICCS notation + chess-cli REPL. 8-crate workspace + WASM cleanliness in CI. 71 tests passing.

<!-- Prune older entries into CHANGELOG.md once prior-year items appear here
     or this section grows past ~20 entries. -->
