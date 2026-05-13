---
status: known-bug
first-hit: 2026-05-13
last-hit: 2026-05-13
---

# Leptos `RwSignal<Vec<T>>` queue with self-clear races silently drops messages

## Symptom (verbatim)

LAN host page works (board renders, "You play Red 紅", "Red 紅 to move"),
but:

* **Joiner stuck** on `Waiting for server greeting…` / `Awaiting seat
  assignment` despite sidebar showing `Connected`.
* Joiner sidebar shows `Black 黑 to move` after host moves — meaning
  some `ServerMsg`s arrive but `Hello` is silently lost.
* Host's **own** view doesn't refresh after host moves a piece — sidebar
  stays on `Red 紅 to move` even though the joiner sees the
  `Black to move` update.

No error in the console. Compilation is clean. The previous "queue"
fix (replacing `WriteSignal<Option<ServerMsg>>` with
`RwSignal<Vec<ServerMsg>>`) made the host's *initial* state work but
post-mount messages still vanished.

## Root cause

The buggy pattern:

```rust
// Push side (DC onmessage / WS read pump / host PeerSink::Local):
incoming.update(|v| v.push(msg));

// Drain side (in pages/play.rs effect):
create_effect(move |_| {
    let msgs = incoming.get();   // subscribes
    if msgs.is_empty() { return; }
    for msg in msgs { match msg { ... } }
    incoming.set(Vec::new());    // CLEARS — race here
});
```

The race: any `update` push that lands between `incoming.get()` and
`incoming.set(Vec::new())` is silently overwritten by the clear. In
practice this happens whenever a `ServerMsg` arrives close to when the
effect re-runs:

1. Effect reads `[A, B]`, processes both, then `set(Vec::new())`.
2. Push lands `update(|v| v.push(C))` — vec is now `[C]`.
3. Effect re-runs (notification from the `set([])` in step 1):
   reads `[C]`, processes... wait, this case actually works.

The actual losing race:

1. Effect reads `[A, B]`, processes A.
2. Push lands `update(|v| v.push(C))` — synchronously, JS is single-
   threaded so this can't happen mid-iteration BUT can happen between
   the `.get()` and the `.set()`. Vec is now `[A, B, C]`.

   Wait — that's still recoverable, because `.get()` returned a clone
   of `[A, B]` (we're iterating the clone), and the underlying signal
   still has `[A, B, C]`. Then `set(Vec::new())` clobbers it: vec
   becomes `[]`. C is lost.

The same race fires for the **host's own move** path: host's
`handle.send(Move)` synchronously calls `room.apply` → `fanout` →
`PeerSink::Local::deliver` → `incoming.update`. If this push happens
while the page's effect is mid-drain (e.g. processing `Hello` →
`set_role` → triggers other reactive work that yields...) the push
gets clobbered.

The same race for the **joiner Hello drop**: joiner's
`PlayConnected` mounts on `state == Open`. By the time the effect
first runs, the host may have already pushed `Hello` AND the
`incoming.set(Vec::new())` clear from the previous (empty) effect run
may still be settling. Result: `Hello` lands in `[]`, then gets
clobbered to `[]` again.

The exact race ordering is hard to predict because Leptos's signal
notification is queued via microtasks and the order depends on
`spawn_local` future timing, DC `onopen` event timing, and Show's
component-mount timing.

**The fundamental design flaw is the clear**: ANY signal where one
party reads-and-writes-back-empty, while another party concurrently
appends, is racy regardless of how careful the read-write timing is.

## Workaround

Replace `RwSignal<Vec<ServerMsg>>` with a queue-and-tick pair:

```rust
#[derive(Clone)]
pub struct Incoming {
    queue: Rc<RefCell<VecDeque<ServerMsg>>>,
    tick:  ReadSignal<u32>,
    set_tick: WriteSignal<u32>,
}

impl Incoming {
    pub fn push(&self, msg: ServerMsg) {
        self.queue.borrow_mut().push_back(msg);
        self.set_tick.update(|n| *n = n.wrapping_add(1));
    }

    pub fn drain<F: FnMut(ServerMsg)>(&self, mut f: F) {
        let _tick = self.tick.get();              // subscribe
        let mut q = self.queue.borrow_mut();
        while let Some(msg) = q.pop_front() {
            f(msg);                                // no clobbering
        }
    }
}
```

Why this is race-free:

* The queue (`VecDeque`) is **outside** the reactive system. Pushers
  borrow_mut, append, drop borrow.
* The tick signal is **monotonic**. Pushers increment; drainers
  subscribe + drain. Nothing ever clears or clobbers.
* JS's single-threaded event loop prevents borrow_mut aliasing in
  practice — the drain holds the borrow synchronously, no other
  callback can fire mid-drain.
* If a push lands while the drain is iterating, it goes into the
  same VecDeque but the drain has already iterated past — no
  problem, the next tick increment re-fires the effect, the next
  drain catches it.

See `clients/chess-web/src/transport/mod.rs::Incoming` for the
production implementation.

## Prevention

For any reactive-style queue:

* **NEVER pair `read-and-clear` on the same signal**. Either the
  signal is monotonic (increments only) or you pair it with an
  external mutable buffer.
* If the obvious shape is "push to a Vec, drain in an effect, clear
  the Vec", that's a smell. Use a `VecDeque` outside the signal +
  a `u32` tick instead.
* When debugging "messages randomly disappear" in Leptos, check for
  any `set` / `update` that wipes a previously-pushed value. The
  symptom is intermittent and timing-dependent — easy to confuse
  for a network bug.

## See also

* `clients/chess-web/src/transport/mod.rs` — `Incoming` struct doc.
* `backlog/webrtc-lan-pairing.md` — Phase 5 testing notes.
* Earlier related pitfall: the prior `WriteSignal<Option<ServerMsg>>`
  shape failed for an even simpler reason — synchronous double-`set`
  of a latched signal batches into one effect firing reading the
  LAST value. The intermediate "queue with self-clear" was the
  attempted fix for THAT bug; it fixed the synchronous-double-set
  case but introduced this race.
