#!/usr/bin/env python3
"""Tiny SPA-aware static server for local PWA verification.

GitHub Pages serves the dist with a `404.html` fallback that's a copy of
`index.html`, so direct links to client-side routes like
`/Chinese-Chess_Xiangqi/local/xiangqi` work. Plain `python3 -m
http.server` does not — it returns its own error page on a missing
file, which is why `make serve-web-static` was 404'ing on every SPA
route.

This handler mirrors the GH Pages behaviour: any GET inside the base
scope that doesn't resolve to a file falls back to `<base>/index.html`.
Outside the scope (or for paths with extensions like .js / .wasm),
return the normal 404 so a real missing asset is still surfaced.

Usage:
    serve-spa.py <serve-dir> <port> <base-prefix>
    # e.g. serve-spa.py /tmp/staging 4173 /Chinese-Chess_Xiangqi/
    # base-prefix may be "/" or empty for root-served deployments.

No external deps — stdlib only, runs on the bundled python3 of any
modern macOS / Linux dev box.
"""
from __future__ import annotations

import os
import sys
from http.server import HTTPServer, SimpleHTTPRequestHandler

if len(sys.argv) != 4:
    print(__doc__, file=sys.stderr)
    sys.exit(2)

SERVE_DIR = os.path.abspath(sys.argv[1])
PORT = int(sys.argv[2])
BASE = "/" + sys.argv[3].strip("/")  # always leading slash, no trailing
if BASE == "/":
    BASE = ""  # treat as root-served

# A path is "asset-like" if it has a file extension we precache or
# explicitly want to 404 on. Anything else (no extension) is treated
# as an SPA route and falls back to index.html.
ASSET_EXTS = {
    ".js",
    ".wasm",
    ".css",
    ".html",
    ".svg",
    ".png",
    ".jpg",
    ".jpeg",
    ".webp",
    ".ico",
    ".webmanifest",
    ".json",
    ".map",
    ".txt",
    ".woff",
    ".woff2",
}


class SpaHandler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=SERVE_DIR, **kwargs)

    def do_GET(self):  # noqa: N802 (stdlib name)
        # Strip query string for the file lookup.
        raw = self.path.split("?", 1)[0]

        # Inside the SPA scope?
        in_scope = raw == BASE or raw == BASE + "/" or raw.startswith(BASE + "/") or BASE == ""
        if not in_scope:
            return super().do_GET()

        # Resolve to a real file inside SERVE_DIR.
        rel = raw.lstrip("/")
        full = os.path.normpath(os.path.join(SERVE_DIR, rel))
        # Sanity: don't escape SERVE_DIR.
        if not full.startswith(SERVE_DIR):
            self.send_error(403)
            return

        # File exists and is a regular file → normal handling.
        if os.path.isfile(full):
            return super().do_GET()

        # Directory request → serve index.html if present, else fall through.
        if os.path.isdir(full):
            idx = os.path.join(full, "index.html")
            if os.path.isfile(idx):
                return super().do_GET()

        # Asset-shaped path that doesn't exist → real 404.
        ext = os.path.splitext(rel)[1].lower()
        if ext in ASSET_EXTS:
            return super().do_GET()

        # SPA route → rewrite to {BASE}/index.html so the Leptos router
        # can take over.
        target_index = (BASE + "/index.html") if BASE else "/index.html"
        self.path = target_index
        return super().do_GET()

    def log_message(self, fmt, *args):
        sys.stderr.write("%s - %s\n" % (self.address_string(), fmt % args))


if __name__ == "__main__":
    HTTPServer.allow_reuse_address = True
    httpd = HTTPServer(("", PORT), SpaHandler)
    base_disp = (BASE + "/") if BASE else "/"
    print(f"SPA server: dir={SERVE_DIR} base={base_disp} port={PORT}")
    print(f"Open http://localhost:{PORT}{base_disp}")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        httpd.server_close()
