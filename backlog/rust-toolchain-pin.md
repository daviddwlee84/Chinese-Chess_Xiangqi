# Pin `rust-toolchain.toml` to match CI

## Why this is in backlog

Right now the workspace has **no** `rust-toolchain.toml`. CI uses
`dtolnay/rust-toolchain@stable`, which resolves to whatever rustup
considers `stable` at job start (currently 1.95). Every developer's
local toolchain is whatever they last ran `rustup update` against.
Drift accumulates silently, and pushes that pass `cargo clippy
-- -D warnings` locally start failing in CI roughly every six weeks
when a new stable adds or strengthens a lint.

Concrete recurrence captured in
[`pitfalls/clippy-version-drift-local-vs-ci.md`](../pitfalls/clippy-version-drift-local-vs-ci.md):
on 2026-05-08 a single push needed three CI round-trips
(`0865ad9` → `3333751` → `0ce14c6`) because clippy 1.95 added
`unnecessary_sort_by` and widened `collapsible_match`, neither of
which my local clippy 1.93 reported.

## What "pinning" looks like

Add at the workspace root:

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.95"
components = ["rustfmt", "clippy"]
targets = ["wasm32-unknown-unknown"]
```

`dtolnay/rust-toolchain@stable` in CI then needs to change to
`dtolnay/rust-toolchain` (no `@stable`) so the action reads the file.
Or simpler: `actions-rust-lang/setup-rust-toolchain@v1`, which
respects `rust-toolchain.toml` automatically.

`targets = [...]` lets us drop the explicit
`with: targets: wasm32-unknown-unknown` block in `pages.yml` —
rustup adds the target on first build.

## Trade-offs

**Pro**
- Local `cargo clippy` matches CI exactly. Lint regressions caught
  before push, not after.
- New contributors get the right toolchain automatically (rustup
  handles the install on first `cargo` invocation; ~1 minute,
  ~150 MB).
- Reproducible builds — easier to bisect a bug to a specific
  rustc version.
- WASM target is auto-installed instead of failing with a confusing
  `error[E0463]` until the contributor reads the docs and runs
  `rustup target add wasm32-unknown-unknown`.

**Con**
- Pinning means we **manually** opt into new compiler features and
  new lints. Today, a fresh `rustup update` brings new improvements
  for free; with a pin, someone has to bump the file.
- Bumping the pin becomes a small chore — typically a one-line PR
  every 6 weeks, but it competes with other priorities.
- If the pinned version ever lags far behind, dependency crates may
  start requiring a newer MSRV and we get blocked from adding a dep
  until the bump.

## Recommended cadence if we do pin

- Bump on every stable release that fixes a clippy lint we care
  about, OR every two stables (12 weeks) regardless. Whichever
  comes first.
- Open a "rust toolchain bump" PR rather than batching with feature
  work — single-purpose PR is easy to revert if it surfaces lints
  that we don't have time to fix.

## Alternative considered: don't pin, just `rustup update` discipline

Document in `AGENTS.md` and `CONTRIBUTING.md`: "before pushing,
run `rustup update stable && cargo clippy ...`". Cheap, no chore,
but relies on memory and doesn't help new agents/contributors who
won't read the doc until after they hit the failure.

This is what we do today. It works for the current contributor
count (≈1) but doesn't scale and doesn't help the agent loop —
the agent's local toolchain is whatever the user's machine has,
which by definition lags the runner. The agent already burned
three CI round-trips on this in one session; the third one was
preventable.

## Decision criterion

Pin when **either**:
- We add a second contributor and the "rustup update first"
  discipline can't be enforced socially, OR
- We hit a third recurrence of the same drift (one was 2026-05-08;
  count from there).

Until then, the pitfall doc + `rustup update stable` workaround
is enough.

## Status

Open — waiting on either trigger above.
