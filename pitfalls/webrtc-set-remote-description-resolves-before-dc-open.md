---
status: known-bug
first-hit: 2026-05-13
last-hit: 2026-05-13
---

# `RtcPeerConnection::set_remote_description` resolves before DataChannel opens — early `dc.send_with_str` silently fails

## Symptom (verbatim)

LAN host page enters game successfully:

* Sidebar: `You play Red 紅`, `Red 紅 to move`, `Connected`.
* Board renders. Host can move pieces. State machine advances.

LAN joiner page (on the same machine, two browser tabs, localhost
LAN — fastest possible network):

* Sidebar: `Connected`, `Awaiting seat assignment`.
* Board area: `Waiting for server greeting…`
* Status flips from `WaitingForOpen` → `Playing` (so the
  DataChannel open detection works), but `PlayConnected` then sits
  with `role: None, view: None` because `Hello` and `ChatHistory`
  never arrive through `Session.incoming`.

No console errors. No transport-layer failure visible.

## Root cause

`RtcPeerConnection::set_remote_description(answer)` resolves as
soon as the SDP is applied — that's just an SDP-parse + state
transition. It does NOT wait for the underlying DataChannel
SCTP handshake to complete. The DTLS + SCTP handshake happens
asynchronously after `setRemoteDescription` resolves.

The host page was doing:

```rust
hh.accept_answer(blob).await?;          // SDP applied, returns FAST
let (room, session) = HostRoom::new(...);
room.attach_remote_player_dc(dc)?;      // calls join_player → fanout
                                         // → dc.send_with_str(Hello)
                                         // → DC still "connecting"!
                                         // → silently fails (Err)
```

`PeerSink::Remote(dc)::deliver` discards the result of
`dc.send_with_str`:

```rust
PeerSink::Remote(dc) => {
    if let Ok(text) = serde_json::to_string(&msg) {
        let _ = dc.send_with_str(&text);   // <-- DROPS error
    }
}
```

So the `Hello` + `ChatHistory` for the joiner are silently lost.
Later, the DC fully opens, joiner's `state` flips to `Open`,
joiner mounts `PlayConnected`, but the queue is empty and stays
empty — `Hello` was dropped on the wire.

This is hard to spot because:

* `accept_answer` returning Ok suggests "everything's fine".
* `dc` exists and `dc.ready_state()` returns... a value that's
  not `Open` yet.
* The joiner's DC eventually opens, so the user sees `Connected`
  in the sidebar, leading them to expect game state to follow.

The race is timing-dependent. On slow networks, the gap between
`accept_answer` and DC open is hundreds of milliseconds. On
localhost it's <10 ms — but still wide enough to lose the
synchronous `attach_remote_player_dc` call.

## Workaround

Wait for `dc.ready_state() == "open"` before fanning out via the DC.
A polling helper covers both the "open eventually" happy path and
the "never opens" failure mode (with a timeout):

```rust
pub async fn wait_for_dc_open(dc: &RtcDataChannel, timeout_ms: u32) -> bool {
    let deadline = js_sys::Date::now() + timeout_ms as f64;
    loop {
        if dc.ready_state() == RtcDataChannelState::Open { return true; }
        if js_sys::Date::now() > deadline { return false; }
        // sleep 50 ms via setTimeout-wrapped Promise
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(win) = web_sys::window() {
                let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 50);
            }
        });
        let _ = JsFuture::from(promise).await;
    }
}
```

Page code:

```rust
hh.accept_answer(blob).await?;
let dc = hh.dc.borrow().clone().ok_or(...)?;
if !wait_for_dc_open(&dc, 10_000).await {
    return error("DataChannel did not open within 10 s");
}
let (room, session) = HostRoom::new(...);
room.attach_remote_player_dc(dc)?;     // safe — DC is Open now
```

Alternative approaches considered + rejected:

* **Wrap `dc.set_onopen` as a Promise** — works, but loses the
  timeout path. We need the timeout to surface "network blocked"
  failures (e.g. AP isolation, firewall).
* **Buffer messages inside `PeerSink::Remote`** until DC opens —
  more invasive, ties the routing layer to DC lifecycle. The
  page-level wait is cleaner.
* **Defer `attach_remote_player_dc` itself to fire on DC open** —
  same as above; pushes complexity into the routing layer when
  the page is the natural place to await async setup.

## Prevention

When using browser DataChannel-style transports:

* **NEVER assume that the SDP-application `await` (i.e.
  `set_remote_description` / `accept_answer`) means the channel
  is usable**. SDP application is a state-machine update; the
  actual transport handshake is separate and asynchronous.
* **Always check `dc.ready_state() == "open"` before sending**.
  The browser will silently fail (`Err`) on a non-open channel,
  which most "dispatch and forget" wrappers (including ours)
  discard.
* **When wrapping `dc.send_with_str` results**, log or surface
  the error. A `let _ =` is fine for steady-state messages where
  occasional drops are acceptable, but at handshake-time you
  want loud failures.
* For greeting-style messages (one-shot per peer), prefer waiting
  synchronously until the channel is fully ready rather than
  firing-and-praying.

## See also

* `clients/chess-web/src/transport/webrtc.rs::wait_for_dc_open` —
  the helper.
* `clients/chess-web/src/pages/lan.rs::LanHostPage::on_accept` —
  the wait site.
* `pitfalls/leptos-create-effect-inside-spawn-local-silent-gc.md`
  — adjacent bug on the joiner page, same Phase 5 testing session.
* `pitfalls/leptos-rwsignal-queue-self-clear-race.md` — earlier
  bug in the same code path.
* MDN: `RTCDataChannel.readyState` —
  https://developer.mozilla.org/en-US/docs/Web/API/RTCDataChannel/readyState
