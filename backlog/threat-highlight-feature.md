# Threat highlight (Display setting)

Status: shipped 2026-05-10. Kept as a maintainer-facing record of the
trade-offs because the SEE value table and the banqi
hidden-piece policy are choices that are likely to be
re-litigated.

## Why this exists

User asked: "想在 Display settings 中新增一個 checkbox 是 能夠
highlight 被叫殺的棋子（不過這個 feature 是不是會很亂 因爲其實整個場上
經常都能同時有很多棋子可以吃其他棋子？）"

The user's instinct was right — the literal "highlight every piece an
opponent could capture next ply" gets visually busy in xiangqi
mid-game (5–12 squares lit up at once). The shipped feature is a
Display-settings dropdown with four levels so the user can pick how
much information they want:

| Mode | Semantic | Typical # highlighted |
|---|---|---|
| Off | nothing | 0 |
| Attacked (mode A) | every piece an opponent can capture next ply | 5–12 mid-game |
| **Net loss** (mode B, default) | mode A filtered by SEE > 0 (only pieces that genuinely lose material) | 0–3 |
| Mate threat (mode C) | opponent piece-squares participating in a checkmate-in-1 threat | 0 (most positions); 1–3 when one exists |

Plus an orthogonal "Hover preview" toggle (off by default) that
rings any of your other pieces that would become newly vulnerable
if the currently selected piece moves away — answers "what is this
piece defending?".

## Architecture

```
chess-core
├── eval/see.rs                  # piece values + Static Exchange Evaluation
├── rules/xiangqi.rs             # attackers_of / attacked_pieces / net_loss_pieces / mate_threat_pieces
├── rules/banqi.rs               # same four (banqi attack relation, hidden pieces excluded)
├── state/mod.rs::GameState      # variant-dispatch wrappers (.attacked_pieces / .net_loss_pieces / .mate_threat_pieces)
└── view.rs::ThreatInfo          # serialised on PlayerView at protocol v6 (#[serde(default)])

clients/chess-web
├── prefs.rs::ThreatMode         # Off / Attacked / NetLoss / MateThreat (default NetLoss)
├── prefs.rs::Prefs.fx_threat_*  # mode + hover toggles, persisted to localStorage
├── pages/picker.rs              # <select> + checkbox in Display settings
├── components/board.rs::ThreatOverlay  # struct passed to <Board>; renders red rings
├── state.rs::hover_threat_squares      # client-side what-if delta
└── style.css::.threat-mark              # red / magenta / soft-red ring styles

clients/chess-tui
├── main.rs                      # --threat-mode / --threat-on-select CLI flags
├── app.rs::ThreatMode           # mirror of the web enum
└── ui.rs::ThreatSets            # cell bg-color tinting via existing intersection_or_piece path
```

## Decisions worth re-litigating

### SEE piece values are hand-picked

`crates/chess-core/src/eval/see.rs::piece_value`:

```text
General  = 10000   (sentinel — any general-on-the-table dominates)
Chariot  = 9
Cannon   = 5       (slight bump above 4.5 fractional, since we use i16)
Horse    = 4
Advisor  = 2
Elephant = 2
Soldier  = 1, becomes 2 once past the river (xiangqi only)
```

These come from the standard xiangqi pedagogy (車 9 / 馬 4 / 砲 4-5
/ 仕相 2 / 兵卒 1-2). Not tuned against game data. Downstream is the
**sign** of `see()`, not its magnitude — Mode B only asks "is this a
losing trade?". So precise weights matter less than relative
ordering.

What we deliberately punted:

- **Cannon = 5 vs 4** — historically cannons are slightly more
  valuable in the opening (need a screen, threatens at range) and
  slightly less in the endgame (no screens left). We pick the higher
  value because Mode B's failure-mode is "missed warning" — better
  to occasionally flag a cannon that's actually OK than to miss one
  that's hung.
- **Soldier past-river = 2 vs 1** — the lateral move is a real
  upgrade, but +1 is a coarse approximation. A more accurate eval
  would scale by row distance to opposing palace; deferred.
- **Banqi values** — banqi rank order (general 6 / advisor 5 /
  elephant 4 / chariot 3 / horse 2 / cannon 1 / soldier 0) doesn't
  match the SEE values above (cannon's special jump-only attack
  makes it value-asymmetric). We use the xiangqi values for both
  variants, with a `net_loss_pieces` carve-out that always flags an
  attacked banqi cannon regardless of SEE — see the cannon fallback
  in `crates/chess-core/src/rules/banqi.rs::net_loss_pieces`.

If users start asking for tuned values: add `Prefs.fx_see_values:
SeeValueTable` and a sub-fieldset under Display settings. Keep the
hard-coded table as the default.

### Banqi hidden pieces don't count as attackers

In banqi, face-down pieces have unknown identity. We chose:

- Hidden defenders are NOT considered (they may or may not be
  threatened — the UI doesn't have a stable identity to ring).
- Hidden attackers are NOT considered (we don't know what they are,
  so we can't say what they attack).

The alternative ("paranoid mode": treat hidden attackers as the
worst-case identity) would flag too many revealed defenders ("this
horse is hung because the adjacent face-down piece COULD be a
chariot"). UX-wise that's noise on every move where a flip might
happen, which is most moves in early banqi.

If users request paranoid mode later, add `Prefs.fx_banqi_paranoid:
bool` (default off) and route it as an extra arg through
`crate::rules::banqi::attackers_of`.

### Mate-threat is depth-2 (one opponent ply ahead)

`mate_threat_pieces` simulates "skip my turn → opponent moves →
am I checkmated?". This catches the strict 叫殺 / mate-in-1.

We deliberately did NOT do depth-N mate threats:

- Depth-3 ("opponent threatens mate-in-2") explodes the search:
  ~30 × 30 × 30 = 27k make/unmake calls per turn instead of ~900.
  Comfortable for native, painful for WASM.
- The visual signal is also too vague — depth-3 mate threats often
  rely on cooperation by the defender ("if I do nothing AND
  opponent finds the right reply…"), which is hard to teach via a
  ring on the board.

If users want mate-in-2 threats: gate behind a fourth Mode (`Mode D
- Mate-in-2`) so the cost is opt-in.

### Protocol v6 backward-compat strategy

`PlayerView.threats: ThreatInfo` was added with `#[serde(default)]`.
Older clients (v5) deserialise `threats` as the empty default, which
the renderer treats as "Off mode" — graceful silent degrade. v6
clients connecting to a v5 server (which doesn't populate the
field) see the same "Off mode" — equivalent to having the user set
the dropdown to Off.

Same pattern as `in_check` (v3→v4) and `chain_lock` (v4→v5) earlier
in `view.rs`.

### Hover preview lives in client code, not on `PlayerView`

Hover state changes per pointer move; computing it server-side and
shipping over the wire would burn bandwidth and add latency. The
client computes the delta locally:

1. take the current view, reconstruct a casual xiangqi GameState,
2. snapshot `attacked_pieces(observer)`,
3. remove the hovered piece, recompute `attacked_pieces(observer)`,
4. return the set difference.

Compute cost: ~2 ms for two `attacked_pieces` walks on 9×10. Safe
to run on every hover change.

Banqi hover: returns empty (the `reconstruct_xiangqi_state_for_analysis`
helper refuses to rebuild banqi state because that would leak hidden
identities into client-readable form). Acceptable trade-off — banqi's
hidden information already makes hover-preview less actionable
there.

## Test coverage

- `chess_core::eval::see::tests` — 5 tests pin SEE on the
  no-attacker, free-general, equal-trade, soldier-takes-chariot, and
  past-river-bump cases.
- `chess_core::rules::xiangqi::tests` — `attacked_pieces_finds_threatened_chariot`,
  `net_loss_excludes_defended_chariot`, `net_loss_excludes_general_in_check`,
  `mate_threat_empty_when_already_mated`, `mate_threat_empty_in_opening`.
- `chess_core::rules::banqi::tests` — `banqi_threat_ignores_hidden_attacker`,
  `banqi_threat_includes_revealed_attacker`.
- `chess_core::view::tests` — `fresh_xiangqi_view_has_safe_threat_info`,
  `three_chariot_view_populates_threat_lists`,
  `pre_v6_view_deserializes_with_empty_threats` (backward-compat).
- `chess_web::state::tests` — `hover_preview_empty_in_opening`,
  `hover_preview_empty_for_opponent_piece`.

## What we'd do differently next time

- **TUI hover trigger**: TUI re-uses `--threat-on-select` (selection
  as a hover proxy) because there's no terminal `pointerover` event.
  A keybind (e.g. `T`) to toggle the on-select preview interactively
  would be friendlier — currently you have to restart the TUI to
  flip the flag. File when requested.
- **Web hover via real `pointerover`**: same selection-as-hover
  proxy applies. Adding per-cell `pointerover` listeners to the SVG
  cells layer would make the preview feel snappier (no need to
  click). Not done yet because click-driven covers touch + keyboard
  for free.
- **Net-mode mate-threat across spectators**: mate-threat is
  computed per observer in `PlayerView::project()` so each side sees
  threats against its own general. Spectators (RED-by-default
  observer) see threats against Red — fine, but a multi-spectator
  view that lets you toggle whose POV you're watching is a separate
  feature, not adjacent to this one.

