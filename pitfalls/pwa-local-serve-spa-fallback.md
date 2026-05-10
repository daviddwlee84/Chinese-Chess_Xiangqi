# Local-served PWA: subpath build 404s on every asset, or SPA routes return "Error response — File not found"

**Symptoms** (any combination):

```
$ make serve-web-static WEB_BASE=/Chinese-Chess_Xiangqi
Serving HTTP on :: port 4173 ...
::1 - "GET / HTTP/1.1" 200 -                          # boot loader shows
::1 - "GET /Chinese-Chess_Xiangqi/style-…css" 404 -   # …but every asset fails
::1 - "GET /Chinese-Chess_Xiangqi/chess-web-…js" 404 -
::1 - "GET /Chinese-Chess_Xiangqi/icons/icon.svg" 404 -
…
```

```
# After clicking into a game route from the picker:
http://localhost:4173/Chinese-Chess_Xiangqi/local/xiangqi?hints=1

  Error response
  Error code: 404
  Message: File not found.
```

```
# Even after rebuilding & restarting, the service worker is still
# making requests to /Chinese-Chess_Xiangqi/... that 404, OR every
# navigation outside that subpath returns "404 (from service worker)".
```

These three things have one root cause apiece, and all three bit us
on the first end-to-end run of the new PWA setup. Each is its own
trap.

---

## Trap 1 — `python3 -m http.server` in `dist-static/` serves the
build at `/`, not at the GitHub Pages subpath

**What happens**: `make build-web-static` produces
`clients/chess-web/dist-static/index.html` whose `<base href>` is
`/Chinese-Chess_Xiangqi/` (because we passed `--public-url` to Trunk).
If you then `cd dist-static && python3 -m http.server 4173`, the
server roots `dist-static` at `/`. The browser fetches
`http://localhost:4173/`, gets that `index.html`, sees the `<base
href>`, and resolves every script / stylesheet / WASM URL to
`/Chinese-Chess_Xiangqi/...` — none of which exist on the python
server, because the dist files live at `/style-….css` etc. on disk.

**Why this is confusing**: you DO see the boot loader render (the
inline HTML shell loads fine from `/`). It feels like "almost
working", but no asset ever loads, so the WASM never starts.

**Fix**: stage `dist-static` *under* the base subpath before serving.
That mirrors GitHub Pages' actual layout. The repo's
`make serve-web-static` target does this:

```makefile
serve-web-static: build-web-static
    @base="$(WEB_BASE)"; base="$${base#/}"; base="$${base%/}"; \
    tmp=$$(mktemp -d -t chess-web-pwa-serve); \
    target="$$tmp"; if [ -n "$$base" ]; then target="$$tmp/$$base"; fi; \
    mkdir -p "$$target"; \
    cp -R clients/chess-web/dist-static/. "$$target/"; \
    python3 clients/chess-web/scripts/serve-spa.py "$$tmp" 4173 "/$$base/"
```

The result: `http://localhost:4173/Chinese-Chess_Xiangqi/index.html`
is the dist's index, and every asset URL resolves correctly. Open
exactly that URL — opening `http://localhost:4173/` (no subpath)
just shows whatever is at the python root, which is misleading.

---

## Trap 2 — `python3 -m http.server` has no SPA fallback, so
client-side routes 404

**What happens**: even after fixing Trap 1, clicking from the picker
into `/Chinese-Chess_Xiangqi/local/xiangqi?hints=1` produces:

```
Error response
Error code: 404
Message: File not found.
Error code explanation: 404 - Nothing matches the given URI.
```

This isn't the SW or the SPA; it's python's own error page.
`SimpleHTTPRequestHandler` only resolves real files on disk, and
`/Chinese-Chess_Xiangqi/local/xiangqi` is a Leptos client-side route,
not a file. The Leptos router is willing to handle it, but only after
`index.html` is served — and python returns 404 instead.

GitHub Pages handles this for you via the `404.html` trick: when a
real 404 happens, Pages serves `404.html`, and we make `404.html`
identical to `index.html` (`make build-web-static` copies it). The
chess-net `--static-dir` mode handles this via `tower_http`'s
`.fallback(ServeFile::new(dir/"index.html"))`. Plain python http.server
does neither.

**Fix**: ship a tiny SPA-aware static server. The repo includes
`clients/chess-web/scripts/serve-spa.py` (stdlib-only, ~120 lines).
Behaviour:

| Path                                      | Action                          |
| ----------------------------------------- | ------------------------------- |
| in scope, exists                          | serve as-is                     |
| in scope, no extension, doesn't exist     | rewrite to `<base>/index.html`  |
| in scope, has known asset extension       | real 404                        |
| out of scope                              | real 404                        |

`make serve-web-static` invokes it automatically. **Do not** swap
back to `python3 -m http.server` for this build — it will look like
the SPA is broken when in fact the server is the problem.

---

## Trap 3 — A poisoned service worker registered at scope `/` keeps
intercepting navigations after you fix the underlying issue

**What happens**: while iterating on Traps 1 + 2, you opened
`http://localhost:4173/` (no subpath) several times. Each visit
caused `pwa.js` to register a service worker at scope `/` (its
`resolveBase()` heuristic returned `/` since the path ended with
`/`). That SW precached *nothing useful* (every URL it tried 404'd)
but it's still installed, with scope `/`, persistent across page
reloads.

After you fix the Makefile and start the SPA-aware server, you reload
the tab. The page at `/Chinese-Chess_Xiangqi/` registers a *new* SW
at scope `/Chinese-Chess_Xiangqi/`. But the *old* SW at scope `/` is
still there. Any navigation that doesn't match the narrower scope is
still handled by the broken SW — including arbitrary requests like
`/sw.js` or future probes, which then return weird 404s "from
service worker".

**Symptoms** in the network log:

```
GET /sw.js → 404 (from service worker)
GET /local/xiangqi → 404 (from service worker, with the old broken cache)
```

**Fix**: clear the registration before re-testing. DevTools →
Application → Storage → "Clear site data" (Service Workers + Cache
storage are enough; you don't need to wipe localStorage). Or in the
console:

```js
navigator.serviceWorker.getRegistrations()
  .then(rs => Promise.all(rs.map(r => r.unregister())));
caches.keys().then(ks => Promise.all(ks.map(k => caches.delete(k))));
```

Then close the tab and re-open the correct URL
(`http://localhost:4173/Chinese-Chess_Xiangqi/`).

**Prevention**: don't open the served base path's *parent* URL during
development. If you've staged dist-static under `/Chinese-Chess_Xiangqi/`,
go straight there — don't probe `/` "to see what happens". The
`make serve-web-static` target prints the exact correct URL on
startup; copy from there.

---

## Why these three were one bug, conceptually

All three are "the GitHub Pages deployment layout has one shape, and
local quick-and-dirty tools (`python3 -m http.server`, intuitively
just opening `/`) have a slightly different one." The PWA layer
amplifies any path mismatch because:

1. The manifest's `start_url` and `scope` are baked in at build time.
2. The SW's precache list is an absolute URL list, also baked at
   build time.
3. SW scope is sticky across reloads; a wrong-scope registration
   persists until manually cleared.

The defenses we ended up shipping:

- `scripts/build-pwa.sh` derives all paths from `TRUNK_PUBLIC_URL`
  (single source of truth, no duplicate base-path values to drift).
- `scripts/serve-spa.py` mirrors GitHub Pages' SPA fallback locally.
- `Makefile :: serve-web-static` stages dist into the right subpath
  layout and prints the correct URL.
- `pwa.js` reads `<base href>` first (Trunk injects this with
  `--public-url`) and only falls back to `pathname.lastIndexOf('/')`
  when no base element exists. As long as you build via the
  Makefile, the base is correct.

If you ever change one of the three (build script, server, or
registration logic), re-test all three traps before declaring done.
A green Lighthouse score on a fresh profile does not catch the
"poisoned old SW" trap because that profile has nothing cached.
