# Rust + WASM as a PWA — chess-web takeaway

This is the long-form companion to the brief `CLAUDE.md` gotcha.
It captures the architecture decisions, the build flow, the
update lifecycle, and every trap we hit while turning the Rust +
Leptos + Trunk SPA into an installable, offline-capable PWA on
two hosting targets.

It is meant to be the document you re-read in a year when you
have to debug something here, and as a transferable recipe if
you build another Rust/WASM app and want a PWA on top.

---

## TL;DR

- **A PWA is the website + 3 extras**: a manifest, a service
  worker, and an icon set. The Rust/WASM bundle stays unchanged.
- **The service worker is plain JS** — written by hand, no
  Workbox, no npm. Templated and rendered by a Trunk post-build
  hook so the precache list is always in sync with the hashed
  filenames Trunk just emitted.
- **Single template, two deploy targets**. The same hook runs
  for `make build-web` (chess-net `--static-dir`, base = `/`) and
  `make build-web-static WEB_BASE=Chinese-Chess_Xiangqi` (GitHub
  Pages, base = `/Chinese-Chess_Xiangqi`). It reads
  `TRUNK_PUBLIC_URL` and substitutes `__BASE_PATH__` so the
  manifest's `start_url` / `scope` and every precached URL are
  correct for that deployment.
- **Update UX is "prompt then reload", not auto-reload**. A
  fresh build precaches but the old SW keeps controlling the
  page; the new one stays in `waiting` until the user clicks the
  toast we render. We do not auto-reload because that would
  interrupt 對局.
- **Online endpoints are bypassed by the SW.** The `/ws*` /
  `/lobby` / `/rooms` / `/api/*` routes flow straight to the
  network; the existing failure surface is intact.

---

## File layout

```
clients/chess-web/
  index.html                    # adds <link rel="manifest">,
                                # <link rel="apple-touch-icon">,
                                # <script defer src="pwa.js">,
                                # and Trunk copy-file directives
                                # for the public/ assets.
  Trunk.toml                    # adds [[hooks]] stage = "post_build"
  Cargo.toml                    # adds web-sys features Event,
                                # EventTarget, Navigator, CustomEvent
  build.rs                      # unchanged — already injects
                                # CHESS_WEB_BASE_PATH for routes.rs
  scripts/
    build-pwa.sh                # Trunk post-build hook
    regenerate-placeholder-icons.sh
  public/
    sw.js.tmpl                  # service worker template
    manifest.webmanifest.tmpl   # web app manifest template
    pwa.js                      # JS bridge: SW register, install
                                # prompt capture, update lifecycle,
                                # online detection
    icons/
      icon.svg                  # source-of-truth design
      icon-maskable.svg         # variant cropped for adaptive icons
      favicon.svg               # small SVG favicon
      icon-192.png              # rasterized placeholder
      icon-512.png              # rasterized placeholder
      icon-maskable-512.png     # rasterized placeholder
      apple-touch-icon-180.png  # iOS home-screen icon
  src/
    components/pwa.rs           # Leptos UI: PwaState, banner,
                                # button, toast, offline indicator
    components/mod.rs           # pub mod pwa;
    app.rs                      # PwaState::hydrate() + provide_context
                                # PwaUpdateToast + OfflineIndicator
                                # mounted globally in AppShell
    pages/picker.rs             # PwaInstallBanner above the hero
    components/sidebar.rs       # PwaInstallButton next to FX toggles
  style.css                     # .pwa-banner / .pwa-toast /
                                # .pwa-online-dot styles
```

The PWA glue is intentionally split: anything that needs to fire
*before* WASM finishes booting (capturing `beforeinstallprompt`,
registering the SW) is in `public/pwa.js`. Everything that needs
to render UI is in Rust/Leptos.

---

## Build flow

```
trunk build
  └─ Rust → app-[hash].js + app-[hash]_bg.wasm
  └─ CSS  → style-[hash].css
  └─ copy-file (data-trunk rel="copy-file"):
       og-image.png
       public/sw.js.tmpl       → dist/sw.js.tmpl
       public/manifest.webmanifest.tmpl → dist/manifest.webmanifest.tmpl
       public/pwa.js           → dist/pwa.js
  └─ copy-dir:
       public/icons            → dist/icons/

post_build hook (scripts/build-pwa.sh):
  1. find dist/ for .js/.wasm/.css/.html/.svg/.png/.webmanifest
     (skip og-image.png, 404.html, sw.js, *.tmpl)
  2. prefix each with TRUNK_PUBLIC_URL → PRECACHE_MANIFEST JSON
  3. shasum -a 256 of concatenated file hashes → APP_VERSION
  4. awk substitute __APP_VERSION__ / __BASE_PATH__ /
     __PRECACHE_MANIFEST__ in templates
  5. write dist/sw.js + dist/manifest.webmanifest
  6. rm dist/sw.js.tmpl + dist/manifest.webmanifest.tmpl
```

`TRUNK_PUBLIC_URL` is set by Trunk's `--public-url` flag:

| Make target            | `--public-url`               | `BASE_PATH` (in templates) |
| ---------------------- | ---------------------------- | -------------------------- |
| `make build-web`       | (default `/`)                | `""` (empty)               |
| `make build-web-static`| `/$WEB_BASE/`                | `/$WEB_BASE`               |

The empty string for the server case is intentional — manifest
`start_url: "/"` and `scope: "/"` are written verbatim. For the
GitHub Pages case `start_url: "/Chinese-Chess_Xiangqi/"` and
every precache URL becomes `/Chinese-Chess_Xiangqi/...`.

---

## Service worker strategy

```
fetch event:
  same origin? └ no  → ignore (let browser fetch)
  is /ws, /lobby, /rooms, /api/*? → bypass
  navigation? → network-first, fall back to cached index.html
  hashed asset (-[8+hex].(js|wasm|css))? → cache-first
  other same-origin GET → stale-while-revalidate
```

**Why network-first for navigations.** SPA route changes (Leptos
router) are intercepted at the document level only on the
initial load. With network-first we always get a fresh
`index.html` when online, and an `index.html`-from-cache when
offline → the Leptos router still takes over and renders the
right page client-side.

**Why cache-first for hashed assets.** Trunk hashes JS/WASM/CSS
by content; the filename is its own integrity check. If the
filename matches, the bytes are correct, and we can serve from
Cache Storage forever. The next build emits a different filename
which will miss the cache and be fetched fresh.

**Why bypass online endpoints.** chess-net needs to handle
disconnection itself; we don't want the SW second-guessing or
caching `/lobby` Server-Sent updates.

> The PWA install prereq is also what enables LAN multiplayer:
> once the service worker is registered, the two phones can
> play 1v1 over WebRTC with NO server in the loop. See
> [`docs/lan-multiplayer-webrtc.md`](lan-multiplayer-webrtc.md)
> for the full pattern.

---

## Update lifecycle

```
user has the app open (controlled by SW v1)
  ↓
trunk build emits new bundle, new sw.js (different APP_VERSION)
  ↓
deploy
  ↓
user reloads (or focuses tab → pwa.js calls registration.update())
  ↓
browser fetches new sw.js (updateViaCache: "none" guarantees no HTTP cache hit)
  ↓
new SW installs (precaches new bundle into chinese-chess-pwa-<new-hash>)
  ↓
new SW state: "installed" + navigator.serviceWorker.controller exists
  ↓
pwa.js fires CustomEvent("pwa:update-ready")
  ↓
Leptos PwaUpdateToast shows up
  ↓
user clicks "Reload"
  ↓
window.__pwa.applyUpdate() posts {type: "SKIP_WAITING"} to waiting SW
  ↓
SW calls self.skipWaiting() — becomes the active worker
  ↓
controllerchange fires on window
  ↓
pwa.js reloads the page, new bundle takes over.
```

**Explicit non-features**: we do *not* call `clients.claim()` in
the SW, and we do *not* auto-reload. Both would bypass the user's
consent and risk reloading the page mid-game.

---

## Install lifecycle

### Chromium (Chrome / Edge / Brave / Samsung Internet / Vivaldi)

```
user visits site
  ↓
manifest + SW pass installability checks (HTTPS / localhost,
  manifest with name + icons + start_url + display, scope match)
  ↓
browser fires beforeinstallprompt on window
  ↓
pwa.js: event.preventDefault() + stash
  ↓
pwa.js fires CustomEvent("pwa:install-available")
  ↓
PwaInstallBanner / PwaInstallButton appear
  ↓
user clicks → window.__pwa.install() → deferredPrompt.prompt()
  ↓
browser shows native install dialog
  ↓
user accepts → appinstalled fires → CustomEvent("pwa:installed")
  ↓
banner / button hide, PwaState.standalone flips true
```

### iOS Safari (16+)

There is **no `beforeinstallprompt`** on iOS. The user has to
tap Share → "Add to Home Screen" themselves. We detect iOS via
UA + iPadOS-as-Mac-with-touch heuristic and render
`PwaInstallBanner` in its `--ios` variant — text + a hint icon
rather than a button.

### Firefox (desktop)

No install prompt at all on desktop Firefox. The banner stays
hidden (no `beforeinstallprompt`, not iOS, not standalone).
Firefox Android does install via menu → "Install" but doesn't
fire `beforeinstallprompt` either; Android Chrome users get the
nice flow.

### Already installed

If `display-mode: standalone` matches at boot, `PwaState.standalone`
is `true` and the banner / button stay hidden for the lifetime
of that session.

---

## Local verification recipes

### 1. Server mode end-to-end

```
make build-web
cargo run -p chess-net -- --port 7878 \
  --static-dir clients/chess-web/dist xiangqi
# open http://127.0.0.1:7878/
```

DevTools checks:

- **Application → Manifest**: should be parsed (no errors).
  `start_url` = `/`, `scope` = `/`, icons all 200.
- **Application → Service Workers**: one registration, source
  `http://127.0.0.1:7878/sw.js`, status "activated and is
  running". Click "Update on reload" while developing.
- **Application → Cache Storage**: one cache named
  `chinese-chess-pwa-<12hex>` containing every dist asset.
- **Lighthouse PWA audit**: run with "Mobile" emulation. Target
  score ≥ 90. Common deductions: missing maskable icon (we
  ship one), no offline page (we use the SPA shell fallback,
  Lighthouse accepts that).

### 2. GitHub Pages mode end-to-end

```
make serve-web-static WEB_BASE=Chinese-Chess_Xiangqi
# open http://localhost:4173/Chinese-Chess_Xiangqi/
```

This is the most important verification because the base path
is the most error-prone surface. Inspect:

```
grep start_url clients/chess-web/dist-static/manifest.webmanifest
# expect: "start_url": "/Chinese-Chess_Xiangqi/",

grep -o '"/Chinese-Chess_Xiangqi/[^"]*"' clients/chess-web/dist-static/sw.js | head -5
# expect every precache URL prefixed with /Chinese-Chess_Xiangqi/
```

### 3. Offline test

```
DevTools → Application → Service Workers → check "Offline"
Reload the page.
Navigate to /local/xiangqi
Play 5 moves.
```

Expected: full functionality. The page loads from cache, the
WASM bundle runs, no network requests succeed but the local
engine handles everything.

If you also wanted online play: DevTools shows the WS connection
fails (`/ws/main` cannot reach the server). The lobby page
shows the existing "connection failed" UI. Our `OfflineIndicator`
in the corner has flipped from green to dim red.

### 4. Update test

```
# Tab open on the previous build.
# Edit any file in clients/chess-web/src/
make build-web                # or build-web-static
# Switch back to the open tab. Reload (Cmd-R).
```

Expected: **PwaUpdateToast appears**. It does so because:

1. The reload triggers `navigator.serviceWorker.register("./sw.js")`
   again, which respects `updateViaCache: "none"` and bypasses
   HTTP cache to fetch the freshly written `sw.js`.
2. The new SW byte-differs from the old (different
   `APP_VERSION`), so it installs.
3. The new SW enters `waiting` because the old one is still
   controlling the page.
4. `pwa.js` sees `state === "installed"` plus
   `navigator.serviceWorker.controller` non-null and dispatches
   `pwa:update-ready`.
5. `PwaUpdateToast` subscribed to that event renders.

Click the toast. Expected: 1 second later the page reloads
running the new bundle.

### 5. Mobile install (Android Chrome)

```
make build-web-static WEB_BASE=Chinese-Chess_Xiangqi
make serve-web-static WEB_BASE=Chinese-Chess_Xiangqi
```

In another terminal find the LAN IP (`ifconfig | grep inet`),
then on the phone visit `http://<lan-ip>:4173/Chinese-Chess_Xiangqi/`.
Wait for the picker — `PwaInstallBanner` should appear at the
top. Tap "Install"; the OS-level install dialog confirms; the
app drawer now has a 帥 icon.

> Service workers technically need a secure context, but
> `localhost` and (on Android) LAN IPs over plain HTTP are
> usually treated as secure for development. For production
> you need HTTPS — GitHub Pages provides this automatically.

### 6. Mobile install (iOS Safari)

Same URL, but the banner shows the iOS variant ("加到主畫面 /
Add to Home Screen"). Tap the share icon at the bottom of
Safari, scroll to "Add to Home Screen", confirm.

---

## Replacing the placeholder icons

```
# Edit clients/chess-web/public/icons/icon.svg          (any-purpose)
# Edit clients/chess-web/public/icons/icon-maskable.svg (maskable safe area)
clients/chess-web/scripts/regenerate-placeholder-icons.sh
# commit the new SVGs + regenerated PNGs
```

The script uses `rsvg-convert` if available (sharper text), and
falls back to ImageMagick. Modern Chrome / Edge / Firefox happily
use the SVG entries from the manifest, but iOS Safari only
accepts PNG for the apple-touch-icon — that's why the script
also rasterizes a 180×180 PNG.

**Maskable variant**: Android's adaptive icon system crops up
to ~10% off each edge to mask into circles, squircles, etc.
Keep all important artwork inside the inner ~80% (a 410×410
centred safe area for a 512×512 icon). The committed
`icon-maskable.svg` shrinks the disc + glyph relative to the
full-bleed background to demonstrate the safe zone.

---

## Renaming the app (home-screen label)

The label that shows up under the icon after "Add to Home Screen"
is **not** a single string — three platforms read three fields,
and any rename has to touch every spot that platform reads:

| Where it appears                                      | Field                                              | File                                            |
| ----------------------------------------------------- | -------------------------------------------------- | ----------------------------------------------- |
| Android / Chrome / Edge — home-screen + app drawer    | `short_name` in the manifest                       | `clients/chess-web/public/manifest.webmanifest.tmpl` |
| Android / Chrome — install dialog + Play Console copy | `name` in the manifest                             | same file                                       |
| iOS / iPadOS Safari — "Add to Home Screen"            | `<meta name="apple-mobile-web-app-title">`         | `clients/chess-web/index.html`                  |
| Desktop window title bar (after install)              | document `<title>`                                 | `clients/chess-web/index.html`                  |
| Chromium app shortcuts long-press menu                | `shortcuts[].short_name` in the manifest           | `manifest.webmanifest.tmpl`                     |

iOS in particular ignores `short_name` entirely — if you only
edit the manifest, your iPhone testers will still see the old
label. Likewise, the manifest `name` is what shows up in the
Chromium install confirmation dialog ("Install <name>?"), so
leaving it out of sync produces a visibly different install vs.
post-install experience.

Already-installed devices keep the *old* label until the user
removes the icon and re-adds it — the OS treats the home-screen
shortcut as immutable metadata. The new SW activating does not
re-prompt for a new label. Tell your testers up-front.

### Recipe

```bash
# 1. Edit manifest.webmanifest.tmpl — at minimum short_name,
#    and probably name and shortcuts[].short_name too.
# 2. Edit index.html — apple-mobile-web-app-title (iOS) and
#    optionally <title> (desktop standalone window).
# 3. Rebuild for both targets so you can sanity-check the
#    rendered manifest in dist/ and dist-static/:
make build-web
make build-web-static WEB_BASE=Chinese-Chess_Xiangqi
grep -E '"(short_)?name"|apple-mobile' \
  clients/chess-web/dist/manifest.webmanifest \
  clients/chess-web/dist-static/manifest.webmanifest \
  clients/chess-web/dist/index.html
```

The post-build hook re-runs because templates are copied fresh,
and `APP_VERSION` will bump (the manifest content changed → the
hash of dist contents changed), which means the
[update lifecycle](#update-lifecycle) toast will fire on the
next visit for already-online users — they get the new
`PwaUpdateToast`, click Reload, and the new manifest is fetched.
But again, **the home-screen icon label only refreshes on
re-install**.

---

## How to clear all caches (debugging stale state)

DevTools approach:

```
Application → Storage → "Clear site data" (untick Cookies if
you want to keep local prefs).
```

Manual approach — paste in the console:

```js
caches.keys().then(keys => Promise.all(keys.map(k => caches.delete(k))));
navigator.serviceWorker.getRegistrations()
  .then(rs => Promise.all(rs.map(r => r.unregister())));
```

After running, hard reload (Cmd-Shift-R / Ctrl-Shift-R).

---

## Future work

Tracked in `TODO.md`:

- IndexedDB / OPFS persistence for in-progress games (so
  closing the tab doesn't lose state).
- Web Share API for FEN/PGN of the current position.
- Replace placeholder icons with proper artwork.

Worth considering if traffic / install count grows:

- **Workbox**. We avoided it because hand-written 200-line
  `sw.js` is auditable and has zero npm footprint, but Workbox
  brings well-tested precaching, navigation routes, expirations,
  background sync — worth migrating to if the SW grows past
  ~500 lines.
- **Background Sync**. Save completed games to a server when
  connectivity returns. Requires a server endpoint chess-net
  doesn't have today.
- **Push Notifications**. Notify when an opponent moves in your
  asynchronous correspondence game. Needs Web Push subscription
  storage on chess-net. iOS 16.4+ supports this *only* for
  installed PWAs.
- **Periodic Background Sync**. Useful for prefetching daily
  puzzles. Chrome-only, low priority.
- **File System Access API**. Save / load PGN files locally
  without round-tripping through the server.

---

## Cross-references

- `pitfalls/pwa-base-path-and-stale-cache.md` — the symptoms-
  first list of every trap we hit while wiring this up.
- `docs/architecture.md` — high-level chess-web architecture;
  ADR-0001 explains why presentation lives client-side rather
  than in chess-core, which is what makes the SPA installable
  in the first place.
- `docs/trunk-leptos-wasm.md` — Trunk + Leptos quirks. The
  `[[hooks]]` mechanism we use here is documented in the Trunk
  manual; see also the existing notes about `data-trunk
  rel="copy-file"` not honouring `--public-url` (the post-build
  hook side-steps this).
