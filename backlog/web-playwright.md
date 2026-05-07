# chess-web Playwright E2E smoke

## Why this is in backlog

PR-1 has one `wasm-bindgen-test` (asserts the picker mounts) and 15
native unit tests for `orient` / `glyph` / `routes` / `state`. That's
enough to catch logic regressions but not enough to catch:

- Lobby → create room → both clients join → board renders → moves work
  end-to-end (the full happy path)
- Race conditions in the `Hello` / `Update` flow under realistic browser
  concurrency
- CSS regressions — the WIP overlay covers the board, the toast appears
  on errors, etc.

A Playwright (or Cypress) headless flow tests all of that without a
human checking a browser.

## Recipe

1. Boot chess-net + Trunk dev-server in a CI container (already wired up
   via `make play-web` for local dev — CI just needs the `cargo install
   trunk` + `rustup target add wasm32-unknown-unknown` prereq).
2. Playwright spawns two browser contexts (= two tabs), navigates to
   `http://localhost:8080/lobby`.
3. Tab A creates room "playwright-test", lands on `/play/playwright-test`.
4. Tab B sees the room appear (live `Rooms` push), joins it.
5. Both render their `<Board>`. Tab A makes a soldier-forward move; Tab B
   sees the update.
6. Tab A resigns; both see `Won{Resignation}` in the sidebar.
7. Both click Rematch; fresh game starts.

## Cost estimate

- Setup: ~1 hour to wire Playwright config + a single test file.
- CI: adds a job that boots tmux-less (just `chess-net-server` +
  `trunk serve` in background) and runs `npx playwright test`.
- Maintenance: low — the test exercises the full surface so most internal
  refactors don't break it.

## Why "P2 not P1"

The flow is already manually testable via `make play-web` + two browser
tabs. Until chess-web has multiple contributors who can introduce
regressions, the manual smoke is sufficient.
