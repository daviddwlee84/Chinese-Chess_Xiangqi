#!/usr/bin/env bash
# Re-rasterize the placeholder PWA icons from the committed SVG sources.
# Run this when you change `public/icons/icon.svg` or replace the
# placeholder design with your own.
#
# Requires `rsvg-convert` (preferred — sharper text antialias) or
# `magick` / `convert` from ImageMagick as a fallback.
#
# Output:
#   public/icons/icon-192.png            (any-purpose, 192x192)
#   public/icons/icon-512.png            (any-purpose, 512x512)
#   public/icons/icon-maskable-512.png   (maskable variant, 512x512)
#   public/icons/apple-touch-icon-180.png (iOS home-screen, 180x180)
#
# These are baked-in placeholders. The real .svg sources are the source
# of truth and are also referenced directly by the manifest (modern
# browsers handle SVG icons), so even without re-running this script
# Chrome / Edge will pick up SVG changes immediately.

set -euo pipefail

cd "$(dirname "$0")/.."

ICONS_DIR="public/icons"
ANY_SVG="${ICONS_DIR}/icon.svg"
MASK_SVG="${ICONS_DIR}/icon-maskable.svg"

render() {
  local src=$1 size=$2 dst=$3
  if command -v rsvg-convert >/dev/null 2>&1; then
    rsvg-convert -w "$size" -h "$size" "$src" -o "$dst"
  elif command -v magick >/dev/null 2>&1; then
    magick -background none -density 384 "$src" -resize "${size}x${size}" "$dst"
  elif command -v convert >/dev/null 2>&1; then
    convert -background none -density 384 "$src" -resize "${size}x${size}" "$dst"
  else
    echo "error: need rsvg-convert or ImageMagick to rasterize icons" >&2
    exit 1
  fi
  echo "wrote ${dst} (${size}x${size})"
}

render "$ANY_SVG"  192 "${ICONS_DIR}/icon-192.png"
render "$ANY_SVG"  512 "${ICONS_DIR}/icon-512.png"
render "$MASK_SVG" 512 "${ICONS_DIR}/icon-maskable-512.png"
render "$ANY_SVG"  180 "${ICONS_DIR}/apple-touch-icon-180.png"
