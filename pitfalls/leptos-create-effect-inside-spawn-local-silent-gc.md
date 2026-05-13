---
status: known-bug
first-hit: 2026-05-13
last-hit: 2026-05-13
---

# Leptos `create_effect` inside `spawn_local` is silently GC'd — never re-runs

## Symptom (verbatim)

LAN joiner page reaches "Status: WaitingForOpen" after generating the
answer SDP, but never transitions to "Playing" even though the host
has accepted the answer and the DataChannel is open on both sides
(verified by host being able to move pieces — chess room state
machine sees both peers).

In code, `connect_as_joiner` sets up `state` as
`ReadSignal<ConnState>`. The DataChannel's `onopen` callback calls
`state.set(ConnState::Open)`. The page has an effect:

```rust
spawn_local(async move {
    match connect_as_joiner(cfg, OfferBlob(blob)).await {
        Ok(jh) => {
            let state = jh.session.state;
            create_effect(move |_| {
                if state.get() == ConnState::Open {
                    set_play_session.set(Some(session_for_play.clone()));
                    set_status.set(JoinStatus::Playing);
                }
            });
            set_status.set(JoinStatus::WaitingForOpen);
        }
    }
});
```

The effect runs ONCE initially (with `state == Connecting`, no-op),
then never re-runs even when `state.set(Open)` fires. No console
error, no panic — just silently dead.

## Root cause

`create_effect` requires a Leptos owner context. It registers the
effect with the current owner (typically a component or a parent
effect), which holds it alive and propagates cleanup.

Inside a `spawn_local` future, there is **no owner**:
`wasm_bindgen_futures::spawn_local` is plain async with no Leptos
context. `create_effect` falls back to creating an "unowned" effect,
which has no retainer. The effect's closure is dropped at the end
of the current microtask once the spawn_local future returns.
Without the closure, signal subscribers can't fire it — the effect
is functionally GC'd.

The signal still receives the `set` calls; they just have no
subscribers. The page sits in `WaitingForOpen` forever.

This is NOT documented prominently in Leptos 0.6, and the failure
mode is silent. A debug-mode warning would help.

## Workaround

Move the `create_effect` to **component scope** (any direct child of
a `#[component]` body) where the owner is the component's lifetime.
Use a holder signal pattern to bridge the spawn_local future's
results back to the component-scope effect:

```rust
#[component]
pub fn LanJoinPage() -> impl IntoView {
    let (state_holder, set_state_holder) =
        create_signal::<Option<ReadSignal<ConnState>>>(None);
    let session_holder: Rc<RefCell<Option<Session>>> = Rc::new(RefCell::new(None));

    // Effect at component scope — properly owned + retained.
    {
        let session_holder = session_holder.clone();
        create_effect(move |_| {
            if let Some(state_sig) = state_holder.get() {
                if state_sig.get() == ConnState::Open {
                    if let Some(session) = session_holder.borrow().clone() {
                        set_play_session.set(Some(session));
                        set_status.set(JoinStatus::Playing);
                    }
                }
            }
        });
    }

    let on_generate = Callback::new(move |_: ()| {
        spawn_local(async move {
            match connect_as_joiner(...).await {
                Ok(jh) => {
                    *session_holder.borrow_mut() = Some(jh.session.clone());
                    // Triggers the component-scope effect to subscribe
                    // to the state signal.
                    set_state_holder.set(Some(jh.session.state));
                }
            }
        });
    });
}
```

The effect subscribes to BOTH `state_holder` (fires once when
spawn_local sets it) AND the inner `state_sig` (fires when the
DataChannel opens). The component owner keeps the effect alive
across both fires.

## Defensive: double-check `dc.ready_state()` after installing `onopen`

Even with the owner fix, on fast LAN paths the DataChannel can open
**before** the Rust `onopen` closure is wired to the channel — the
SCTP handshake can complete in <1 ms but the
`ondatachannel` → `Closure::wrap` → `set_onopen` chain is multiple
microtasks. The browser fires `onopen` exactly once and won't re-fire
when a handler is added later. Mitigation:

```rust
fn install_dc_handlers_for_joiner(...) {
    // ... set_onopen / set_onmessage / set_onclose ...

    // Defensive: cover the race where the DC opens before the handler.
    if dc.ready_state() == RtcDataChannelState::Open {
        state.set(ConnState::Open);
    }
}
```

This is cheap and idempotent (the state signal is monotonic on the
`Connecting → Open` edge). Without it, fast-LAN cases would hang.

## Prevention

For any Leptos code that creates effects:

* **NEVER call `create_effect` inside `spawn_local`** without
  explicitly providing an owner via `with_owner`. The default-owner
  case silently drops the effect.
* If you need an effect that reacts to the result of an async
  operation, define the effect at component scope and use a holder
  signal that the async operation `set`s.
* When wiring browser event handlers via `web_sys::set_on*` /
  `Closure::wrap`, **always check the underlying state immediately
  after** to cover the case where the event already fired before
  the handler was installed.

## See also

* `clients/chess-web/src/pages/lan.rs::LanJoinPage` — production
  use of the holder pattern.
* `clients/chess-web/src/transport/webrtc.rs::install_dc_handlers_for_joiner`
  — defensive `ready_state` check.
* Leptos 0.6 docs:
  https://docs.rs/leptos/0.6/leptos/fn.create_effect.html — does
  not warn about the spawn_local case.
