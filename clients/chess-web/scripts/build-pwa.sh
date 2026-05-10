#!/usr/bin/env bash
# Trunk post-build hook: generate `dist/sw.js` and
# `dist/manifest.webmanifest` from the templates in `public/`.
#
# Trunk passes:
#   TRUNK_STAGING_DIR  — absolute path to the dist directory
#   TRUNK_PUBLIC_URL   — public URL prefix from `--public-url`
#                        (defaults to "/" when not set; for GitHub
#                        Pages we run with --public-url "/<repo>/").
#
# Why a post-build step?
#   - Trunk hashes JS/WASM/CSS for cache busting; we want those exact
#     hashed filenames in the SW precache list, so we can only know
#     them after the build runs.
#   - The manifest needs `start_url` / `scope` baked at build-time to
#     match the deployment base path (server vs. GitHub Pages subpath).
#
# This script intentionally avoids any external deps beyond bash + sed
# + find + shasum — all default on macOS / Linux dev machines.

set -euo pipefail

STAGING="${TRUNK_STAGING_DIR:-clients/chess-web/dist}"
PUBLIC_URL="${TRUNK_PUBLIC_URL:-/}"

# Normalize PUBLIC_URL → BASE_PATH:
#   "/"                           -> ""
#   "/Chinese-Chess_Xiangqi/"     -> "/Chinese-Chess_Xiangqi"
#   "Chinese-Chess_Xiangqi/"      -> "/Chinese-Chess_Xiangqi"  (lenient on
#                                  WEB_BASE without leading slash)
#   "/sub/"                       -> "/sub"
BASE_PATH="${PUBLIC_URL%/}"
if [[ -n "${BASE_PATH}" && "${BASE_PATH}" != /* ]]; then
  BASE_PATH="/${BASE_PATH}"
fi

if [[ ! -d "${STAGING}" ]]; then
  echo "[build-pwa] staging dir missing: ${STAGING}" >&2
  exit 1
fi

# Locate the template sources. Trunk copies them into dist via the
# index.html `<link data-trunk rel="copy-file" …>` directives, so the
# real source-of-truth is the project tree.
HOOK_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${HOOK_DIR}/.." && pwd)"
SW_TMPL="${PROJECT_DIR}/public/sw.js.tmpl"
MF_TMPL="${PROJECT_DIR}/public/manifest.webmanifest.tmpl"

if [[ ! -f "${SW_TMPL}" || ! -f "${MF_TMPL}" ]]; then
  echo "[build-pwa] template files missing under ${PROJECT_DIR}/public/" >&2
  exit 1
fi

# ---- Build the precache URL list ------------------------------------
#
# Walk dist/ and collect every shipping asset. Skip anything that
# (a) the SW would refuse to fetch anyway (sw.js itself, sw.js.tmpl /
# manifest.webmanifest.tmpl leftovers from the copy-file step), or
# (b) is purely social-card metadata not needed offline (og-image.png,
# 404.html — GitHub Pages uses 404.html only on real not-founds, and
# the SW handles SPA fallback navigation independently).

cd "${STAGING}"

# Collect relative paths, prefix with BASE_PATH/, sort for determinism.
mapfile -t PRECACHE_REL < <(
  find . -type f \
    \( -name "*.js" -o -name "*.wasm" -o -name "*.css" \
       -o -name "*.html" -o -name "*.svg" -o -name "*.png" \
       -o -name "*.webp" -o -name "*.webmanifest" \) \
    ! -name "sw.js" \
    ! -name "sw.js.tmpl" \
    ! -name "manifest.webmanifest.tmpl" \
    ! -name "og-image.png" \
    ! -name "404.html" \
    | sed 's|^\./||' | sort
)

if [[ ${#PRECACHE_REL[@]} -eq 0 ]]; then
  echo "[build-pwa] no assets found under ${STAGING}" >&2
  exit 1
fi

# Build a JS array literal: ["${BASE}/index.html", "${BASE}/foo-abc.js", …]
PRECACHE_JSON="["
first=1
for rel in "${PRECACHE_REL[@]}"; do
  if [[ ${first} -eq 1 ]]; then
    first=0
  else
    PRECACHE_JSON+=","
  fi
  PRECACHE_JSON+="\"${BASE_PATH}/${rel}\""
done
PRECACHE_JSON+="]"

# Always force "/" (== BASE_PATH/) into the precache so navigation
# fallback can find a cached shell at the root URL too.
if ! printf '%s\n' "${PRECACHE_REL[@]}" | grep -q '^index\.html$'; then
  echo "[build-pwa] warning: index.html not produced by Trunk?" >&2
fi

# ---- Compute APP_VERSION (content hash) -----------------------------
#
# Concatenate per-file sha256s, then sha256 the result; keep the first
# 12 hex chars. Stable across runs as long as the dist contents are
# byte-identical, which is what we want for Cache-Storage versioning.

VERSION_RAW="$(
  for rel in "${PRECACHE_REL[@]}"; do
    shasum -a 256 "${rel}"
  done | shasum -a 256 | awk '{print $1}'
)"
APP_VERSION="${VERSION_RAW:0:12}"

cd "${PROJECT_DIR}"

# ---- Render templates -----------------------------------------------
#
# Use `awk` for the substitution because the precache JSON contains
# characters (slashes, quotes) that play badly with sed delimiters.

render_template() {
  local src=$1 dst=$2
  awk -v ver="${APP_VERSION}" -v base="${BASE_PATH}" -v list="${PRECACHE_JSON}" '
    {
      gsub(/__APP_VERSION__/, ver)
      gsub(/__BASE_PATH__/, base)
      gsub(/__PRECACHE_MANIFEST__/, list)
      print
    }
  ' "${src}" > "${dst}"
}

render_template "${SW_TMPL}" "${STAGING}/sw.js"
render_template "${MF_TMPL}" "${STAGING}/manifest.webmanifest"

# Drop the leftover *.tmpl files Trunk copy-file dropped into dist.
rm -f "${STAGING}/sw.js.tmpl" "${STAGING}/manifest.webmanifest.tmpl"

echo "[build-pwa] version=${APP_VERSION} base='${BASE_PATH}' files=${#PRECACHE_REL[@]}"
