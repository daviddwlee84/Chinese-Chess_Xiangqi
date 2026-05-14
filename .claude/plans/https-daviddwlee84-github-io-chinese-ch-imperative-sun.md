# Extend WebRTC LAN mode: banqi support + host-side rule/variant picker

## Context

LAN mode (Phase 5 / 5.5) ships as a working WebRTC pairing flow but is
**xiangqi-only**, with **no rule customisation** at the host setup screen.
The variant is hardcoded at exactly one site:

```rust
// clients/chess-web/src/pages/lan.rs:258
let (room, session) = HostRoom::new(RuleSet::xiangqi(), None, /* hints */ false);
```

The protocol envelope (`ServerMsg::Hello.rules: RuleSet` in
`crates/chess-net/src/protocol.rs`) already carries variant + every flag
+ the banqi seed end-to-end — the joiner reads `Hello.rules` and renders
the host's authoritative state via `PlayerView` projections. No new
protocol message and no protocol version bump are needed.

Banqi-specific correctness is already handled by the existing protocol:
the host shuffles once via `ChaCha8Rng::seed_from_u64(banqi_seed)` in
`crates/chess-core/src/setup.rs::build_banqi`, then projects per-side
`PlayerView` (hidden tiles stay opaque per ADR-0004). The joiner never
reconstructs the board locally; it just renders the projection. So
adding banqi to LAN requires **zero changes to** `host_room.rs` or
`transport/webrtc.rs` — only the host's choice of `RuleSet` and a small
joiner-side display.

Three-kingdom-banqi is **deliberately excluded** from the LAN dropdown —
it's a 3-player variant and WebRTC pairing here is 2-player only
(would need a 3-peer mesh redesign, not scoped).

### Intended outcome

Host clicks the variant dropdown on `/lan/host`, picks banqi, optionally
configures preset / house-rules / seed, and clicks "Open room". Joiner
scans QR or pastes offer. Both sides see the same variant + rules
summary and start playing banqi over WebRTC with hidden-tile flips
working correctly. Existing xiangqi LAN flow is preserved (the default
`/lan/host` still opens a casual xiangqi room with no extra clicks).

---

## Commit 1 — variant + rules picker on `/lan/host`

**Critical files (modify):**

- `clients/chess-web/src/pages/lan.rs` — `LanHostPage` (lines ~119–270):
  add form controls before the "Open room" button; read selection on
  submit; pass to `HostRoom::new(rules, …)` at line 258.

**Reused helpers (no changes; just call):**

- `clients/chess-web/src/routes.rs:378` — `build_rule_set(variant, &LocalRulesParams) -> RuleSet`.
  Pure function; no URL roundtrip needed. Branches on variant, picks
  `xiangqi_casual` vs `xiangqi`, and threads `banqi_seed`.
- `clients/chess-web/src/routes.rs:106` — `LocalRulesParams` struct.
  Shareable form-state intermediate; 13 fields (most are AI-only and
  irrelevant to LAN — leave them at `Default::default()`).
- `clients/chess-web/src/routes.rs:224` — `parse_local_rules(get_fn)`.
  Used **only** for the optional URL pre-fill polish (see step 5).

### Changes (in order)

1. **Add signals scoped to `LanHostPage`** (above the existing
   `on_open`/`on_accept` closures):

   ```rust
   let variant = create_rw_signal(Variant::Xiangqi);
   let params  = create_rw_signal(LocalRulesParams::default());
   ```

   The xiangqi default is `xiangqi_casual` (matches `params.strict == false`),
   which is the same default the picker uses for `/local/xiangqi` per
   `routes.rs:475-480`. This preserves the current LAN-host behaviour
   (xiangqi casual room opens with one click).

2. **Render the form** as a single inline `<section class="lan-rules-form">`
   above the existing "Open room" button. Do **not** lift `picker.rs::XiangqiCard`
   or `BanqiCard` — they're 450 + 100 lines of inline JSX with AI options,
   board preview, side selector, and `build_local_href` URL generation
   that LAN doesn't need. The Plan agent confirmed there is no extracted
   `<RulesForm>` component to reuse. Duplicating the *pattern* (signals
   → `LocalRulesParams` → `build_rule_set`) is the right call here;
   extracting a shared component for 2 consumers of different shape is
   premature abstraction. Track `<RulesForm>` extraction in a follow-up
   if a third consumer appears.

   Form contents:

   ```rust
   <section class="lan-rules-form">
       <label>"Variant"</label>
       <select on:change=move |ev| {
           // Update variant signal from select value; reset params on switch
           // so xiangqi house flags don't leak into banqi and vice versa.
           let slug = event_target_value(&ev);
           if let Some(v) = parse_variant_slug(&slug) {
               variant.set(v);
               params.set(LocalRulesParams::default());
           }
       }>
           <option value="xiangqi">"Xiangqi (象棋)"</option>
           <option value="banqi">"Banqi (暗棋)"</option>
           // three-kingdom intentionally NOT listed; see Context.
       </select>

       <Show when=move || variant.get() == Variant::Xiangqi>
           <label>
               <input type="checkbox"
                   prop:checked=move || params.get().strict
                   on:change=move |ev| params.update(|p| p.strict = event_target_checked(&ev))/>
               "Strict (self-check forbidden)"
           </label>
       </Show>

       <Show when=move || variant.get() == Variant::Banqi>
           // Preset dropdown — Purist / Taiwan / Aggressive / Custom.
           // On change: set params.house = matching PRESET_* constant.
           // Custom = whatever the user toggles below.
           <label>"Preset"</label>
           <select on:change=move |ev| { /* set params.house from preset */ }>
               <option value="purist">"Purist (no house rules)"</option>
               <option value="taiwan">"Taiwan (chain + chariot rush)"</option>
               <option value="aggressive">"Aggressive (all flags on)"</option>
               <option value="custom">"Custom…"</option>
           </select>

           // Six house-rule checkboxes, each bound to a HouseRules bitflag.
           // Use HOUSE_TOKENS (routes.rs:432) for label ↔ flag mapping.
           <label><input type="checkbox" .../> "Chain capture (chain)"</label>
           <label><input type="checkbox" .../> "Dark capture (dark)"</label>
           <label><input type="checkbox" .../> "Dark capture trade (dark-trade)"</label>
           <label><input type="checkbox" .../> "Chariot rush (rush)"</label>
           <label><input type="checkbox" .../> "Horse diagonal (horse) — UI flag, not yet wired"</label>
           <label><input type="checkbox" .../> "Cannon fast move (cannon) — UI flag, not yet wired"</label>

           // Seed input — numeric, blank = engine picks random.
           <label>"Seed (optional)"</label>
           <input type="number" placeholder="random"
               on:input=move |ev| {
                   let v = event_target_value(&ev);
                   params.update(|p| p.seed = v.parse::<u64>().ok());
               }/>
       </Show>
   </section>
   ```

   Note on the two "not yet wired" flags: per CLAUDE.md, only `CHAIN_CAPTURE`,
   `DARK_CAPTURE`, `DARK_CAPTURE_TRADE`, `CHARIOT_RUSH` are end-to-end in
   move-gen; `HORSE_DIAGONAL` and `CANNON_FAST_MOVE` parse but are no-op.
   Match `picker.rs::BanqiCard` exactly: show them and label them
   honestly. Don't silently hide — that hides behaviour drift from
   future users.

3. **Wire the room-open call** (currently line 258 of `lan.rs`):

   ```rust
   // before:
   let (room, session) = HostRoom::new(RuleSet::xiangqi(), None, false);
   // after:
   let rules = routes::build_rule_set(variant.get_untracked(), &params.get_untracked());
   let (room, session) = HostRoom::new(rules, None, false);
   ```

   `get_untracked()` is correct here: the open-room handler reads the
   form value once at click time and doesn't want to re-fire on later
   form edits (the room is locked once opened).

4. **Disable the form once the room is open.** After `on_open` succeeds
   (status moves past `Idle`), hide / `disabled` the variant + rules
   inputs so the user can't toggle them mid-pairing. Show the chosen
   rules as a read-only summary line ("Banqi · Taiwan · seed: 42") in
   the existing status area instead. Reuse the `describe_rules` helper
   from commit 2 (define it once, call from both pages).

5. **(Optional polish, still commit 1)** Parse query string on page load
   via `routes::parse_local_rules(...)`:

   ```rust
   // Wrap in use_query() / web_sys::window().location().search()
   let initial = parse_local_rules(|k| /* read query param */);
   params.set(initial);
   // Plus parse a top-level ?variant= via parse_variant_slug
   ```

   Gives free deep-linking — `/lan/host?variant=banqi&house=chain,rush&seed=42`
   pre-fills the form. Sets up a future picker entry ("Host LAN game" button
   per variant card) as a trivial follow-up.

6. **Add styles** in `clients/chess-web/style.css` — small section, ~10 lines:
   ```css
   .lan-rules-form { display: flex; flex-direction: column; gap: 6px;
                     margin-bottom: 12px; padding: 8px;
                     border: 1px solid #eee; border-radius: 4px; }
   .lan-rules-form label { display: flex; align-items: center; gap: 4px; }
   ```
   Match the existing form aesthetic in `style.css` (`.chat-input`,
   `.lan-buttons` etc.).

7. **Unit tests** (native) in `routes.rs::tests` are already comprehensive
   (line 445–940). No new tests needed for `build_rule_set` / `parse_local_rules`
   themselves. The new `describe_rules` helper (added in commit 2) is the
   only new pure function and gets its own ~3 unit tests there.

---

## Commit 2 — joiner sees variant + rules on `/lan/join`

**Critical files:**

- `clients/chess-web/src/pages/lan.rs` — `LanJoinPage` (lines ~441–717).
  Specifically, the `ServerMsg::Hello` handler (the joiner currently
  ignores `hello.rules`, so the user has no idea what they're about to
  play until the board renders).

**Changes:**

1. **Capture `rules` from the `Hello` message** in `LanJoinPage`. After
   pairing, the data-channel `onmessage` handler dispatches on
   `ServerMsg`. In the `Hello { rules, view, .. }` arm:

   ```rust
   set_rules_received.set(Some(rules.clone()));
   // existing handling: store view in board signal, set status…
   ```

   Add a `(rules_received, set_rules_received) = create_signal::<Option<RuleSet>>(None)`
   at the top of `LanJoinPage`.

2. **Display the summary** below the existing Status line:

   ```rust
   <Show when=move || rules_received.get().is_some()>
       <p>
           "Playing: " {move || describe_rules(rules_received.get().as_ref().unwrap())}
       </p>
   </Show>
   ```

3. **Add `describe_rules`** as a pure helper in `clients/chess-web/src/state.rs`
   (state.rs is native-buildable per `lib.rs:14` — keeps it out of the
   wasm32-only modules and unit-testable):

   ```rust
   pub fn describe_rules(rules: &RuleSet) -> String {
       let mut parts: Vec<String> = vec![variant_label(rules.variant).to_string()];
       match rules.variant {
           Variant::Xiangqi => {
               parts.push((if rules.xiangqi_allow_self_check { "casual" } else { "strict" }).into());
           }
           Variant::Banqi => {
               if !rules.house.is_empty() {
                   parts.push(format!("house: {}", house_csv_label(rules.house)));
               }
               if let Some(seed) = rules.banqi_seed {
                   parts.push(format!("seed: {seed}"));
               }
           }
           Variant::ThreeKingdomBanqi => {}
       }
       parts.join(" · ")
   }

   fn variant_label(v: Variant) -> &'static str {
       match v {
           Variant::Xiangqi => "Xiangqi",
           Variant::Banqi  => "Banqi",
           Variant::ThreeKingdomBanqi => "Three-Kingdom Banqi",
       }
   }
   ```

   `house_csv_label` reuses `routes.rs::house_csv` logic — either expose
   the existing private helper (`house_csv` at routes.rs:405) as `pub(crate)`
   and call it, OR copy the 8 lines into state.rs. **Prefer `pub(crate)`**:
   single source of truth for the canonical comma-separated form
   ("chain,rush"). Touch the existing function signature only — no
   behaviour change.

4. **Reuse in commit 1's form** — the host also calls `describe_rules`
   to render the read-only summary line after the room opens (step 4
   of commit 1). Single helper, two callers.

5. **Tests** in `state.rs::tests` (or `tests` mod near the helper):

   - `describe_rules(RuleSet::xiangqi_casual())` → `"Xiangqi · casual"`
   - `describe_rules(RuleSet::xiangqi())` → `"Xiangqi · strict"`
   - `describe_rules(RuleSet::banqi_with_seed(PRESET_TAIWAN, 42))` →
     `"Banqi · house: chain,rush · seed: 42"`
   - `describe_rules(RuleSet::banqi(HouseRules::empty()))` →
     `"Banqi"` (no flags, no seed → no extra parts).

---

## Verification

```bash
# 1. Workspace sanity (every crate still compiles, no leptos-into-pure-modules leak)
cargo check --workspace

# 2. Native tests — covers routes::build_rule_set + parse_local_rules
#    (existing 80+ tests) + the new describe_rules helper (3 new tests)
cargo test -p chess-web

# 3. Clippy clean
cargo clippy --workspace --all-targets -- -D warnings

# 4. Format clean (CI requires)
cargo fmt --check

# 5. WASM compile — the actual target
cargo build --target wasm32-unknown-unknown -p chess-web

# 6. Local xiangqi smoke (regression: ensure existing flow unchanged):
make play-web
# Tab A: http://localhost:8080/lan/host  (default = Xiangqi, no toggles)
#   → confirm form shows Xiangqi + Strict unchecked
#   → click "Open room"  → status shows "Playing: Xiangqi · casual"
# Tab B: http://localhost:8080/lan/join  → paste offer
#   → confirm joiner sees "Playing: Xiangqi · casual"
# Make 2-3 moves both sides; confirm board syncs as before.

# 7. Local banqi smoke (new path):
make play-web
# Tab A: /lan/host → variant dropdown → "Banqi (暗棋)"
#   → preset = Taiwan, seed = 42
#   → Open room → status: "Playing: Banqi · house: chain,rush · seed: 42"
# Tab B: /lan/join → paste/scan offer
#   → confirm joiner status: "Playing: Banqi · house: chain,rush · seed: 42"
#   → confirm both sides render the SAME 4×8 hidden layout
#     (since seed=42 forces deterministic shuffle; both sides should
#      agree on which 32 tiles are face-down where)
# Flip a tile from each side; confirm the flipped piece appears
# identically on both ends. Capture, chain capture, verify rules fire.

# 8. Banqi without seed (default random):
# Tab A: /lan/host → variant = Banqi → preset = Purist (no flags) → no seed
#   → Open room → status: "Playing: Banqi"  (no "house:" or "seed:" parts)
# Tab B: → confirm joiner sees same; board layouts agree (host shuffles
#   once, projection works as for xiangqi)

# 9. Deep-link smoke (commit 1's optional URL pre-fill):
open http://localhost:8080/lan/host?variant=banqi&house=chain,rush&seed=42
# Confirm form initialises with: variant dropdown = Banqi, preset = "Custom"
# (or auto-detect as Taiwan if exact match), checkboxes for chain + rush
# checked, seed input = 42.

# 10. Form-locking behaviour after open:
# Tab A: open a room (any variant)  → confirm variant dropdown is
# disabled (or hidden) and form values can't be changed mid-pairing.

# 11. iOS QR scan + banqi joiner — manual on iPhone if available:
# After cargo build --target wasm32-unknown-unknown -p chess-web,
# make build-web-static WEB_BASE=/Chinese-Chess_Xiangqi, deploy.
# Confirm the banqi LAN flow works through QR scan + camera path
# (uses existing jsQR loader fixed in earlier commit).
```

**Cannot verify locally:** physical-LAN pairing across two real devices
for banqi. Same caveat as Phase 5.5 — the in-browser two-tab flow uses
the WebRTC loopback path which already proved working for xiangqi, so
adding banqi to the same channel is mechanical (no new transport edges,
no protocol changes). If the user runs the cross-device test, the only
new failure mode is "banqi rules misbehave" — same risk as any banqi
session in `chess-net` today.

---

## Out of scope (call out in commit body)

- **Picker "Host LAN game" entry point.** A button on each picker
  variant card linking to `/lan/host?variant=...&house=...&seed=...`.
  Trivial once commit 1's deep-link parsing lands; track via
  `scripts/add-todo.sh` if user wants the picker integration.
- **In-game rule editing.** Once a room opens, rules are locked.
  Re-opening with different rules requires a new room. Matches
  chess-net `Room` semantics; rule editing would need a "rematch with
  new rules" message that doesn't exist in protocol v4 / v5.
- **Three-kingdom-banqi over WebRTC.** 3-player; doesn't fit 2-peer
  pairing. Would need a 3-peer mesh topology — separate redesign.
- **`<RulesForm>` component extraction.** Premature with 2 consumers
  (picker + LAN host) of differing shape. Re-evaluate when a third
  consumer appears or when the two drift in a confusing way.
- **Banqi `?role=spectator` over WebRTC.** Spectator role exists in
  chess-net protocol but WebRTC currently treats every peer as a
  player. Adding spectators needs a third peer + projection routing,
  separate from this work.
- **House-rule flags `HORSE_DIAGONAL` / `CANNON_FAST_MOVE` wired in
  move-gen.** UI form will show them (matching picker), but they
  remain no-op until `rules/banqi.rs::generate` consumes them. Already
  tracked in `TODO.md` as base-engine work.
