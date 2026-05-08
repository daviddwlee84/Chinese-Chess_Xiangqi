# iOS Safari taps on SVG `<rect>` don't fire `click` (Chrome on iOS works)

**Symptoms** (grep this section):

- Mobile iOS Safari users report **chess pieces won't select on tap** in
  the chess-web client at <https://daviddwlee84.github.io/Chinese-Chess_Xiangqi/>.
  Desktop Chrome / Safari / Firefox with a mouse work fine.
- **Mobile iOS Chrome works**, only mobile Safari is broken — same WebKit
  engine, but Chrome's input glue dispatches `click` differently.
- iOS Safari pops the native **"Copy / Find Selection / Look Up"** callout
  over the SVG board on long-press (when only `user-select: none` /
  `-webkit-user-select: none` is applied; missing `-webkit-touch-callout: none`).
- After the callout is suppressed, **long-press shows the `.cell-hit:hover`
  fill (a small rectangle) but the click handler never runs** — pointer
  events reach the rect, the click synthesis fails.
- Leptos `on:click=move |_| on_click.call(sq)` on `<rect class="cell-hit">`
  works on every platform except mobile Safari.

**First seen**: 2026-05
**Affects**: iOS Safari (verified on iOS 18, iPhone). Mobile Chrome on iOS
**works**, so any "iOS = WebKit" assumption is misleading. Web client crate:
`clients/chess-web` (Leptos 0.6 + Trunk + SVG-only board).
**Status**: workaround landed (commits `4d2f550` → `c1981a5` → `22ba5cf` on
`main`) — see Workaround for the layered fix.

## Symptom

User flow:

1. Open <https://daviddwlee84.github.io/Chinese-Chess_Xiangqi/local/xiangqi>
   in mobile Safari.
2. Tap any piece. Expected: orange selection ring + green dots on legal
   destination squares (same as desktop mouse-click). Actual: nothing
   happens. The status sidebar still says `Turn: Red 紅`, `44 legal moves`.
3. Long-press the same piece. Expected: still nothing — long-press isn't
   bound. Actual (before any of the fixes): native iOS callout
   `Copy | Find Selection | Look Up` opens over the board with blue text-
   selection drag handles around the SVG `<text>` glyph.
4. After applying `-webkit-touch-callout: none` + `user-select: none`: the
   callout is gone, but long-press now flashes a faint translucent square
   over one cell (that's `.cell-hit:hover` firing). Click handler still
   never runs.
5. Open the same URL in iOS Chrome → everything works. Same device, same
   WebKit underneath.

No JS console errors. `cargo check --workspace` and `cargo build --target
wasm32-unknown-unknown -p chess-web` are both green. Bug is browser-level.

## Root cause

Three independent iOS-Safari-only behaviours stack up and each blocks taps
in a different way:

1. **Text-selection on SVG `<text>`** — when the user holds a finger on
   the board, iOS interprets it as a "select text" gesture against the
   piece glyph (`俥`, `馬`, …) which is a real `<text>` element, raises
   the system callout, and *consumes* the touch so no `click` synthesis
   ever happens. CSS `user-select: none` blocks the actual selection but
   not the callout — that needs the **separate** `-webkit-touch-callout:
   none` property.

2. **`:hover` ghost-click** — iOS Safari emulates `:hover` on the first
   tap so devices without real hover capability still see hover styles.
   When an element has a `:hover` rule defined, the first tap triggers
   the hover state and the synthesized `click` is *suppressed* until the
   second tap. With a transparent `.cell-hit:hover` fill, the user sees
   nothing on tap and assumes the rect isn't clickable.

3. **`click` synthesis on bare SVG `<rect>` is unreliable on iOS Safari**
   even after (1) and (2) are fixed. WebKit synthesizes `click` for
   non-button elements only when several heuristics align (`cursor:
   pointer`, hit-test paint, no overlapping touch interception). For SVG
   `<rect class="cell-hit">` with `fill: transparent` we hit a path where
   Safari decides the tap is "ambiguous" and silently drops the click
   event. Mobile Chrome on iOS uses different input glue and dispatches
   `click` correctly — that's why the same WebKit renderer works there.

The compounding makes triage hard: each individual CSS fix moves the
needle (callout disappears, hover stops eating the click) but the SVG
`click` synthesis quirk is the *last* gate, and it's the one that needs
a JS-level change.

## Workaround

Three commits, layered. All landed on `main`:

### 1. Suppress iOS text-selection callout on the SVG container

`clients/chess-web/style.css`, on `.board`:

```css
.board {
    display: block;
    width: 100%;
    height: auto;
    max-height: 80vh;
    user-select: none;
    -webkit-user-select: none;
    -webkit-touch-callout: none;
    touch-action: manipulation;
}
```

`-webkit-touch-callout: none` is the load-bearing property — `user-select`
alone does not stop the system Copy / Look Up menu. `touch-action:
manipulation` also kills the legacy 300ms tap delay and blocks
double-tap-zoom on the board.

### 2. Gate `:hover` to hover-capable devices

```css
.cell-hit {
    fill: transparent;
    cursor: pointer;
    touch-action: manipulation;
    pointer-events: all;
}
@media (hover: hover) and (pointer: fine) {
    .cell-hit:hover { fill: rgba(212, 165, 92, 0.18); }
}
```

`@media (hover: hover) and (pointer: fine)` matches mouse + trackpad only
— phones / tablets skip the `:hover` rule entirely so iOS no longer
turns the first tap into a hover gesture. `pointer-events: all` ensures
the transparent rect is hit-tested even on iOS versions that mishandle
SVG `fill: transparent` for `pointer-events: visiblePainted` (default).

### 3. Replace `on:click` with `on:pointerup` on the SVG hit-test layer

`clients/chess-web/src/components/board.rs` (around line 250 of
`hit_cells`):

```rust
out.push(
    view! {
        <rect
            class="cell-hit"
            x=x y=y width=CELL height=CELL
            on:pointerup=move |_| on_click.call(sq)
        />
    }
    .into_view(),
);
```

This is the **decisive** change. Pointer events are the W3C unified
input model (mouse / touch / pen) and iOS 13+ Safari fires `pointerup`
natively from a touch release without going through Safari's SVG `click`
synthesis path. Mouse on desktop still fires `pointerup` on button
release, so behaviour is unchanged there.

**Don't** keep `on:click` alongside `on:pointerup` — on devices that
fire both, the handler runs twice and a tap-to-select followed by tap-
to-move executes the move on the first tap.

## Prevention

- For any new tappable SVG element in `clients/chess-web/`, prefer
  `on:pointerup` (or `on:pointerdown` if "tap to start" feels right) over
  `on:click`. Reserve `on:click` for native HTML `<button>` / `<a>` —
  those don't have the SVG synthesis quirk.
- When you add a `:hover` rule to anything tappable on touch, gate it
  with `@media (hover: hover) and (pointer: fine)`. The desktop hover
  feedback survives, mobile ghost-click is avoided.
- Treat "iOS Chrome works, iOS Safari doesn't" as a reliable signal that
  you've hit either a click-synthesis quirk or a touch-callout / hover
  pseudo issue — the underlying WebKit is the same, but Chrome's input
  glue is different.
- The native `cargo test -p chess-web` suite cannot catch any of this
  (it's wasm-only behaviour, browser-only). Smoke-test the deployed URL
  on a **real iPhone**, mobile Safari specifically, before announcing a
  mobile-friendly release. iOS Chrome smoke-test is a *false negative*
  for these bugs.

## Related

- `clients/chess-web/src/components/board.rs` — `hit_cells` is the only
  SVG element with a tap handler in the chess-web crate; the rest of the
  UI uses HTML `<button>` / `<a>`.
- `clients/chess-web/style.css` — `.board` and `.cell-hit` rules.
- Sibling pitfall:
  [`leptos-router-base-trailing-slash.md`](leptos-router-base-trailing-slash.md)
  — also a "deploys cleanly, breaks only in real browser" trap, and also
  doesn't surface in `cargo test`.
- Spec references:
  - [W3C Pointer Events L3](https://www.w3.org/TR/pointerevents3/)
  - [MDN: `touch-action`](https://developer.mozilla.org/en-US/docs/Web/CSS/touch-action)
  - [MDN: `-webkit-touch-callout`](https://developer.mozilla.org/en-US/docs/Web/CSS/-webkit-touch-callout)
  - [MDN: `@media (hover)`](https://developer.mozilla.org/en-US/docs/Web/CSS/@media/hover)
- Original session that diagnosed this: 2026-05-08, fix shipped over
  three commits as iterations against real-device testing on iPhone iOS.
