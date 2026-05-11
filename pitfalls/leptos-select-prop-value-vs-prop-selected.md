# Leptos `<select prop:value=...>` doesn't reliably select the matching `<option>` on first render

**First seen**: 2026-05-11
**Affects**: chess-web's picker, any Leptos `<select>` whose default
selection is bound to a signal via `prop:value` rather than per-option
`prop:selected`.

## Symptom

The user sees a `<select>` dropdown with one option visually selected
(typically the FIRST `<option>` in source order), but the underlying
signal that drives the application's behaviour reflects a DIFFERENT
value (the signal's actual default).

In chess-web specifically:

- Picker shows `Threat highlight: 關閉 / Off` in the dropdown
- User clicks "Start Xiangqi" without changing anything
- Game page renders red `.threat-mark` rings around several pieces
  (the NetLoss highlights — the actual `prefs.fx_threat_mode` default)
- User reports "Threat highlight Off 卻還是顯示" (Off but still showing)

## Root cause

```rust
// BUG — <select> shows the first <option> on initial render even when
// `prop:value` is set to a non-first value.
view! {
    <select prop:value=move || fx_threat_mode.get().as_str()>
        <option value="off">"關閉 / Off"</option>
        <option value="attacked">"…"</option>
        <option value="netLoss">"被捉 / Net loss (recommended)"</option>
        <option value="mateThreat">"叫殺 / Mate threat"</option>
    </select>
}
```

`prop:value` on `<select>` writes to the DOM `.value` property. Per the
HTML spec, setting `<select>.value` to `"netLoss"` should cause the
matching `<option value="netLoss">` to become selected. **But** during
Leptos's initial render, the `<select>`'s `.value` is set BEFORE the
`<option>` children are inserted into the DOM. The browser falls back
to selecting the first option (per the default-selection rule) and
ignores the value we wrote.

After the user interacts with the dropdown, things work correctly —
but the initial visible state lies about what's actually stored in the
signal.

## Fix

Bind selection per-option with `prop:selected` instead:

```rust
view! {
    <select on:change=on_mode_change>
        <option
            value="off"
            prop:selected=move || fx_threat_mode.get() == ThreatMode::Off
        >"關閉 / Off"</option>
        <option
            value="attacked"
            prop:selected=move || fx_threat_mode.get() == ThreatMode::Attacked
        >"被攻擊 / Attacked"</option>
        <option
            value="netLoss"
            prop:selected=move || fx_threat_mode.get() == ThreatMode::NetLoss
        >"被捉 / Net loss"</option>
        <option
            value="mateThreat"
            prop:selected=move || fx_threat_mode.get() == ThreatMode::MateThreat
        >"叫殺 / Mate threat"</option>
    </select>
}
```

`prop:selected` is set on each `<option>` individually after it's been
inserted, so the browser respects exactly one option being marked
selected.

## Prevention

When introducing any `<select>` whose initial value is dynamic
(driven by a signal, URL param, localStorage value, etc.):

1. **Always use `prop:selected` on `<option>` children**, not
   `prop:value` on the `<select>` parent.
2. If the dropdown represents a setting that gates downstream
   behaviour (rendering, network requests, AI search), add a unit
   test or manual smoke check that verifies the dropdown's display
   matches the signal value on first mount, not just after
   interaction.
3. In the picker → game flow, the dropdown sets a Prefs signal that
   the game reads via `expect_context::<Prefs>()`. If the picker's
   display lies about the signal, the user's expectation gets
   silently violated when they navigate to the game.

## Related

- [`leptos-effect-tracking-stale-epoch.md`](leptos-effect-tracking-stale-epoch.md)
  — another Leptos reactive-runtime gotcha (effects firing with
  out-of-order dependencies).
- [`alpha-beta-root-score-pollution.md`](alpha-beta-root-score-pollution.md)
  — class-of-bug doc structure mirrored here.
