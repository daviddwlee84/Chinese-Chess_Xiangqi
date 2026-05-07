# chess-web in-game rule editor (gear icon → modal)

## Why this is in backlog

PR-1 ships a picker-side rules form that builds the URL query string
(`/local/banqi?house=chain,rush&seed=42`, `/local/xiangqi?strict=1`).
Re-configuring rules mid-session means navigating back to the picker,
re-filling the form, and clicking "Start" — all the form state is lost,
and the in-progress game is abandoned without a confirmation.

The follow-up: an in-game gear icon next to the variant label that opens
a modal showing the same controls, pre-filled from the current
`RuleSet`. Apply rebuilds the URL (so it stays shareable) and starts a
fresh game; cancel does nothing.

## What good looks like

1. Gear button on the sidebar (chess-tui has none — this is web-only).
2. Modal reuses the same Leptos components as the picker cards. Don't
   reinvent the form — refactor `XiangqiCard` / `BanqiCard` so the form
   bodies are extractable into `<XiangqiRuleForm/>` / `<BanqiRuleForm/>`
   that take an initial `LocalRulesParams` and emit on submit.
3. Pre-fill: read `state.rules` (which is what `build_rule_set` produced)
   and reverse-derive a `LocalRulesParams`. The reverse mapping is
   trivial since `LocalRulesParams` is the URL-shape of `RuleSet`:
   - `xiangqi_allow_self_check` ↔ `!strict`
   - `house` is identical
   - `banqi_seed` ↔ `seed`
4. On submit, call `leptos_router`'s `use_navigate()` with the new path
   (e.g. `/local/banqi?house=chain&seed=99`). The page re-renders
   `<LocalGame/>` from scratch — `LocalRulesParams` → `RuleSet` →
   `GameState::new(rules)` — and the modal closes.
5. Confirmation: if the in-progress game has any history, show a
   "Discard current game?" check before navigating.

## Notes

- Apply-without-restart is **not** a goal. Banqi seed change mid-game is
  meaningless (the shuffle is fixed at `GameState::new`); xiangqi
  strict/casual change mid-game would require revalidating history.
- The picker form already encodes everything in the URL. The modal is
  just a second editor of the same query — no new state shape.
- For online play, the gear should be hidden / disabled. Rule changes
  are a server concern; see `backlog/chess-net-protocol.md` and the
  P2 entry for mixed-variant rooms.

## Test plan

- Open `/local/banqi`, gear → modal pre-checks default flags + seed
  blank. Toggle `chain`, set seed=7, apply → URL becomes
  `/local/banqi?house=chain&seed=7` and a fresh game starts.
- Open `/local/xiangqi?strict=1`, gear → strict radio is selected.
  Switch to casual, apply → URL strips `?strict=1`.
- Mid-game with moves played: gear → toggle, apply → confirmation
  prompt fires. Cancel keeps state.

## Related

- `clients/chess-web/src/routes.rs` — `LocalRulesParams` + parser
- `clients/chess-web/src/pages/picker.rs` — XiangqiCard / BanqiCard
  (extract `<XiangqiRuleForm/>` / `<BanqiRuleForm/>` here)
- `clients/chess-web/src/pages/local.rs` — `<LocalGame/>` (host of the
  gear button)
- `docs/trunk-leptos-wasm.md` — for `use_query_map` / `use_navigate`
  patterns
