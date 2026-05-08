# Leptos effect captures stale epoch when tracking both upstream + token

**Symptom**

Local vs-AI on `/local/xiangqi?mode=ai&...`: the AI made its very first move (when AI=Red goes first) but never moved again. AI=Black configurations never moved at all. No "AI thinking…" banner appeared on the player's stuck turn. Engine unit tests all passed; the move pump in `clients/chess-web/src/pages/local.rs` was the culprit.

**Setup**

The page has a `move_epoch: RwSignal<u32>` bumped on every state-mutating event (player click, AI move, undo, new game). The intent is "AI task captures epoch at start, drops the result if epoch changed by the time `choose_move` returns." Click handlers were ordered:

```rust
state.update(|s| { s.make_move(&mv).unwrap(); s.refresh_status(); });
selected.set(None);
move_epoch.update(|n| *n = n.wrapping_add(1));
```

The AI effect was:

```rust
create_effect(move |_| {
    let cur_epoch = move_epoch.get();
    let v = view.get();                        // ← also tracked
    if v.side_to_move != cfg.ai_side { return; }
    if ai_thinking.get_untracked() { return; }
    ai_thinking.set(true);
    let snapshot = state.get_untracked();
    spawn_local(async move {
        TimeoutFuture::new(80).await;
        if move_epoch.get_untracked() != cur_epoch {
            ai_thinking.set(false);
            return;                             // ← always tripped
        }
        ...
    });
});
```

**Root cause**

Leptos fires the effect on *every* tracked-signal change. Because `view.get()` was tracked, the effect re-ran twice per click:

1. `state.update(...)` mutates state → `view` memo recomputes → effect runs. `move_epoch` is **still old**, so `cur_epoch` captures the stale value. Sees AI's turn, sets `ai_thinking=true`, spawns the task with `cur_epoch = N`.
2. `move_epoch.update(...)` bumps to N+1 → effect runs again. `ai_thinking` is now `true` (set by run #1) → early-return.
3. Task wakes after 80 ms: `move_epoch.get_untracked() == N+1`, captured `cur_epoch == N` → mismatch → bails out, sets `ai_thinking=false`. No AI move.

The first AI=Red move worked because the effect runs once on mount with `move_epoch == 0` and no prior bump — `0 == 0` so the check passes.

**Fix**

Track only the token (`move_epoch`); read the upstream state via `with_untracked`. The effect then runs exactly once per epoch bump, after both `state` and `move_epoch` have settled:

```rust
create_effect(move |_| {
    let cur_epoch = move_epoch.get();
    let v = view.get_untracked();              // ← no longer tracked
    if v.side_to_move != cfg.ai_side { return; }
    ...
});
```

**Why this is a class of bug, not a one-off**

Any "epoch token + content signal" pattern in Leptos has the same trap when click handlers update them in the order `content → token`. Either:

- Track only the token (this fix), **or**
- Reorder click handlers so the token is bumped *first*, then content (works but forces every callsite to remember the order — fragile).

The tracked-content version is also wasteful: it runs the effect twice per click for no benefit.

**Test that would have caught it**

Hard to unit-test directly — the buggy code is `#[cfg(target_arch = "wasm32")]`-gated and depends on Leptos's reactive runtime. Integration test options for follow-up:

- A `wasm-bindgen-test` covering "after N player moves, AI has made N AI moves" — would need the leptos-test runtime.
- A Playwright test against `make play-web` that drives clicks and asserts the AI replies (tracked in `backlog/web-playwright.md`).
- Extracting the move-pump decision into a pure helper `fn ai_should_move(view, cfg, ai_thinking) -> bool` — testable with `cargo test`, but doesn't catch the *timing* aspect (which is the actual bug).

**See also**

- `clients/chess-web/src/pages/local.rs` — fix landed here.
- `.claude/plans/ai-vs-chatgpt-deep-research-docs-ai-dee-typed-stonebraker.md` — original plan introducing the pump.
- [`docs/ai/README.md`](../docs/ai/README.md) — version index for the engine the pump drives. The pump itself is engine-agnostic; this trap applies to any `Strategy` variant.
- [`backlog/chess-ai-search.md`](../backlog/chess-ai-search.md) — v5 (Web Worker) is where the pump's epoch dance becomes a real cancellation token rather than a "drop the result" check.
