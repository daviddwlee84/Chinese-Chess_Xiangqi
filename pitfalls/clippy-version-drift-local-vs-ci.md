# Local clippy is silently behind CI clippy → `-D warnings` push fails

**Symptom**

Push lands cleanly after `cargo clippy --workspace --all-targets -- -D warnings` succeeds locally. The `clippy` job on GitHub Actions then fails with errors that did not exist on the developer machine:

```
error: this `if` can be collapsed into the outer `match`
   --> crates/chess-core/src/state/mod.rs:713:17
    = note: `-D clippy::collapsible-match` implied by `-D warnings`
    = help: for further information visit
      https://rust-lang.github.io/rust-clippy/rust-1.95.0/index.html#collapsible_match
error: could not compile `chess-core` (lib) due to 4 previous errors
```

Or:

```
error: consider using `sort_by_key`
   --> clients/chess-web/src/state.rs:136:9
```

Both lints come from clippy ≥ 1.95 (`collapsible_match` widened, `unnecessary_sort_by` is a 1.95 addition). Local toolchain reports e.g. `clippy 0.1.93` while CI runs `dtolnay/rust-toolchain@stable` and currently resolves to 1.95.

## Root cause

`.github/workflows/ci.yml` pins `dtolnay/rust-toolchain@stable`, which **always** installs the latest stable rustup channel at job start. The repository has **no** `rust-toolchain.toml`, so each developer's local clippy is whatever they last ran `rustup update` against. Drift accumulates silently — every six weeks the rust release adds or strengthens lints, and pushes that look green locally start failing in CI.

The `cargo clippy` invocation itself doesn't warn about the version skew because there is no protocol for "this lint requires clippy ≥ X". Each lint just exists or doesn't.

## Workaround (per-developer, no repo change)

```sh
rustup update stable
rustc --version    # check it matches what CI shipped most recently
cargo clippy --workspace --all-targets -- -D warnings
```

Do this **before** `git push`, especially after a long break from the project or when you see a new clippy warning in someone else's PR you reviewed.

If you want to know what CI is currently using:

```sh
gh run view --job <clippy-job-id> --log | grep "rustc 1\."
# e.g. "stable-x86_64-unknown-linux-gnu updated - rustc 1.95.0 (...)"
```

## Permanent fix (deferred — see backlog)

Pin the toolchain in-repo so local and CI always match:

```toml
# rust-toolchain.toml at workspace root
[toolchain]
channel = "1.95"
components = ["rustfmt", "clippy"]
```

Trade-off: every contributor must download that exact toolchain on first build (rustup does this automatically, but it costs ~150 MB and a minute on a clean machine). Tracked in [TODO.md](../TODO.md) under P? as `[?/S] Pin rust-toolchain.toml to match CI` → see [`backlog/rust-toolchain-pin.md`](../backlog/rust-toolchain-pin.md).

## Why workaround-first instead of fix-first

The drift bites only on a release boundary (every ~6 weeks) and the recovery is one extra commit. Pinning forces every contributor to update the pin every time we want a new lint or compiler feature, and Cargo edition / MSRV management is a project-wide decision worth a separate spike rather than a reflex fix.

## Related

- Session that surfaced this: 2026-05-08, three CI round-trips before green (`0865ad9` → `3333751` → `0ce14c6`). Each round added one lint we hadn't seen locally.
- 2026-05-09 recurrence #5: `clippy::unnecessary_sort_by` flagged `sort_by(|a,b| b.cmp(&a))` in `engines::pick_with_randomness`; CI rust 1.95 vs local rust 1.93. One-line fix: switch to `sort_by_key(|x| Reverse(x))`. The `rustup update stable` workaround at the top of this doc didn't help because `rustup` self-updated but the toolchain still resolved to 1.93 on this machine.
- Same family of lints to expect on future clippy bumps: anything in <https://rust-lang.github.io/rust-clippy/master/index.html> that's marked `correctness` or `style` and added in the latest minor release.
