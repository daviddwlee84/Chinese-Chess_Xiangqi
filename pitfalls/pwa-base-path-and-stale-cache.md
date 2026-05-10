# PWA on GitHub Pages subpath: install fails or app shows stale code

**Symptoms** (any of):

```
Chrome DevTools → Application → Manifest:
  Error: "Page does not work offline" / "Site cannot be installed"
  start_url is not in scope
```

```
Installed PWA opens to a blank page or "Offline and no cached
shell available."
```

```
After deploying a new build, users keep seeing the old version
for hours / days. Hard reload (Cmd-Shift-R) "fixes" it but a
normal reload does not.
```

```
sw.js: Failed to fetch / 404
manifest.webmanifest: Failed to fetch / 404
```

These are three different traps that all happen when a Rust/WASM
PWA is deployed under a non-root path (e.g. GitHub Pages at
`/Chinese-Chess_Xiangqi/`). They are easy to misdiagnose because
the dev-server flow (Trunk on `localhost:8080/`) works fine.

---

## Trap 1 — manifest `start_url` / `scope` written without the base path

**Symptom**: Chrome refuses to install ("start_url is not in scope")
or installs but the launched app loads a blank page.

**Root cause**: A static `manifest.webmanifest` baked at compile
time with `"start_url": "/"` is wrong on GitHub Pages, where the
app actually lives at `/Chinese-Chess_Xiangqi/`. The manifest's
URL fields must include the deploy subpath, *and* the `scope`
must encompass `start_url`, *and* `start_url` must resolve to a
real document on the server.

**Fix**: Treat the manifest as a build artifact, not source. We
keep `public/manifest.webmanifest.tmpl` with `__BASE_PATH__`
placeholders and the post-build hook (`scripts/build-pwa.sh`)
substitutes `TRUNK_PUBLIC_URL` (minus its trailing slash) before
writing `dist/manifest.webmanifest`. For the GitHub Pages target
that produces:

```json
"start_url": "/Chinese-Chess_Xiangqi/",
"scope":     "/Chinese-Chess_Xiangqi/",
"id":        "/Chinese-Chess_Xiangqi/"
```

The `id` field stabilises the install identity across deploys
that change the start URL (e.g. moving from a subpath to the
root domain) — useful for analytics, harmless to set to the same
value as `start_url`.

The same hook also rewrites every `icons[].src` to start with
the base path. Hard-coded `/icons/icon-512.png` breaks; use
`__BASE_PATH__/icons/icon-512.png`.

**Verification**:

```
make build-web-static WEB_BASE=Chinese-Chess_Xiangqi
grep -E '"(start_url|scope|src)"' clients/chess-web/dist-static/manifest.webmanifest
```

Every URL should start with `/Chinese-Chess_Xiangqi/`.

---

## Trap 2 — `sw.js` cached by HTTP, new builds invisible for hours

**Symptom**: You ship a new version. The user reloads the page.
The PWA update toast doesn't appear and `console.log`s show the
old build is still running. After 24 hours (or after a
"force-reload") the new version finally surfaces.

**Root cause**: Browsers, by default, treat `sw.js` like any
other static asset and apply normal HTTP cache rules. GitHub
Pages serves with `Cache-Control: max-age=600` for everything,
so the browser will not re-fetch `sw.js` for 10 minutes — and on
a hot CDN cache it can be hours. Some hosting providers go
further, serving JS with weeks-long caching.

The service worker lifecycle only kicks in when the browser
actually fetches `sw.js` and finds it byte-different. If the
fetch is satisfied from HTTP cache, *the lifecycle never runs*.

**Fix**: Two-pronged.

1. In `pwa.js`, register with `updateViaCache: "none"`:

   ```js
   navigator.serviceWorker.register(swUrl, {
     scope: BASE,
     updateViaCache: "none",
   });
   ```

   This tells the browser to bypass HTTP cache when checking for
   SW updates. It applies to `sw.js` and any `importScripts()`
   it pulls in, but not to other assets.

2. In `pwa.js`, opportunistically call `registration.update()`
   on `window.focus`. This catches the case where the user
   leaves the tab open for a long time, switches to it, and
   triggers a check.

If you control the server, you can also add `Cache-Control:
no-cache` on `sw.js` itself; on GitHub Pages you can't, so
`updateViaCache: "none"` is the only lever and it's enough.

**Verification**:

```
# After making a code change:
make build-web-static WEB_BASE=Chinese-Chess_Xiangqi
make serve-web-static WEB_BASE=Chinese-Chess_Xiangqi
# Tab open from previous build → reload (regular Cmd-R, not
# Cmd-Shift-R). PwaUpdateToast should appear within ~1 second.
```

---

## Trap 3 — first run online works, second run offline shows a white page

**Symptom**: User installs the PWA online, plays a game,
everything is fine. User opens the app the next day with the
phone on airplane mode → blank white page or "ERR_FAILED" in
the corner. Investigation shows Cache Storage has only a couple
of small files, missing the JS / WASM bundle.

**Root cause**: Service worker registration is async and runs
*concurrently* with the initial page load. By the time the SW
finishes installing and would intercept fetches, the page has
already loaded its JS / WASM via the regular browser HTTP
fetch — those bytes never went through the SW, so they never
got a chance to be cached.

If your SW relies on **runtime cache only** (cache-on-fetch),
the first online visit will load the bundle without populating
the cache. The second visit (offline) finds an empty cache and
fails.

**Fix**: **Build-time precache list**. The post-build hook
walks `dist/` after the build finishes, lists every shipping
asset, and bakes the URLs into the SW's `install` handler:

```js
const PRECACHE_URLS = [
  "/Chinese-Chess_Xiangqi/index.html",
  "/Chinese-Chess_Xiangqi/app-9f3ac8b1c2.js",
  "/Chinese-Chess_Xiangqi/app-9f3ac8b1c2_bg.wasm",
  "/Chinese-Chess_Xiangqi/style-abc123def.css",
  "/Chinese-Chess_Xiangqi/manifest.webmanifest",
  "/Chinese-Chess_Xiangqi/icons/icon-192.png",
  // ...
];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) =>
      Promise.all(PRECACHE_URLS.map(u => cache.add(u))),
    )
  );
});
```

Now the SW's install step actively fetches every asset into the
cache, regardless of what the page itself has loaded. The
second visit (offline) finds a fully populated cache.

We use individual `cache.put()` calls instead of `cache.addAll`
because `addAll` is atomic — one 404 fails the whole install,
which is unfortunate during deployment races. With per-URL
puts wrapped in `try/catch`, a single missed asset doesn't
brick the install.

**Verification**:

```
# DevTools → Application → Cache Storage → expand
#   chinese-chess-pwa-<hash>:
# Should list every JS/WASM/CSS file Trunk emitted, plus
# the manifest, plus every icon.
```

If the list is short or missing the hashed JS/WASM, the
post-build hook didn't run or the precache list ended up
empty. Check `clients/chess-web/scripts/build-pwa.sh` against
the `find` filter (mismatched extensions silently exclude
files).

---

## Bonus: don't precache `og-image.png`

`og-image.png` is the social-card image used by Facebook /
LinkedIn / Discord crawlers. It's not part of the runtime
shell — caching it eats space for no benefit, and worse, if
you ever swap it for a new design, the SW will keep serving
the old one to crawlers that re-fetch through the same origin.

The `find` filter in `build-pwa.sh` explicitly excludes
`og-image.png`. If you add other social / SEO assets, exclude
them too unless they are needed offline.

---

## Bonus: 404.html is a GitHub Pages quirk, not a precache target

`make build-web-static` copies `index.html` to `404.html` so
GitHub Pages serves the SPA shell on unknown URLs (so direct
links to `/Chinese-Chess_Xiangqi/play/myroom` work). The SW
also has its own SPA fallback (network-first navigation →
cached `index.html`), so we deliberately exclude `404.html`
from precache — caching both would just waste a slot.
