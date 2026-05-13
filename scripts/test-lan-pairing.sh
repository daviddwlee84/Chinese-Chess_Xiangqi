#!/usr/bin/env bash
#
# scripts/test-lan-pairing.sh — end-to-end smoke test for LAN multiplayer
# via two playwright-cli browser tabs.
#
# Drives the offer/answer dance through textarea+paste (NOT the camera
# scanner — playwright-cli's headless Chrome doesn't have a usable
# webcam, so the camera path is left to manual real-device testing).
# The textarea path is the same `transport::webrtc` machinery as the
# camera path, so this catches:
#   * WebRTC SDP exchange + ICE gathering.
#   * The full transport layer (HostRoom, Session, Incoming queue,
#     PeerSink::Local + Remote, fanout).
#   * Phase 5.5 commits 1+2: QR encoding renders side-by-side with
#     textarea (verified by greppable QR SVG element).
#
# What it does NOT catch:
#   * Camera capture frame loop (jsQR + raf timing).
#   * QR decode round-trip (encode-then-decode, would need a
#     real camera or canvas trickery).
#   * Cross-device transport (only single-machine same-browser).
#
# Pre-reqs:
#   * trunk serve running on https://192.168.31.136:8080 (TLS)
#     (or pass URL as $1).
#   * playwright-cli installed (`npm install -g @playwright/cli`).
#   * Working `node` to JSON-decode SDP envelopes.
#
# Usage:
#   scripts/test-lan-pairing.sh [base-url]
#
# Exit code:
#   0 — both tabs reached the play view + chat sync confirmed.
#   1 — any step failed; logs left in /tmp/lan-test-*.log for debug.

set -euo pipefail

BASE_URL="${1:-https://192.168.31.136:8080}"
HOST_URL="$BASE_URL/lan/host"
JOIN_URL="$BASE_URL/lan/join"
LOG_DIR="/tmp"
TMP_DIR="$(mktemp -d)"
trap "rm -rf '$TMP_DIR'" EXIT

step() {
    printf '\n\033[1;36m▶ %s\033[0m\n' "$1"
}

ok() {
    printf '\033[1;32m  ✓ %s\033[0m\n' "$1"
}

fail() {
    printf '\033[1;31m  ✗ %s\033[0m\n' "$1" >&2
    exit 1
}

PWC="npx --no-install playwright-cli"

step "Open host tab + joiner tab"
$PWC open --browser=chrome "$HOST_URL" > /dev/null
$PWC tab-new "$JOIN_URL" > /dev/null
ok "two tabs open"

step "Click Open room on host"
$PWC tab-select 0 > /dev/null
$PWC click "button:has-text('Open room')" > /dev/null
sleep 4

OFFER_RAW=$($PWC --raw eval "() => document.querySelector('textarea').value")
echo "$OFFER_RAW" | node -e "console.log(JSON.parse(require('fs').readFileSync(0, 'utf8')))" \
    > "$TMP_DIR/offer.txt"
[ -s "$TMP_DIR/offer.txt" ] || fail "offer textarea empty"
ok "offer SDP captured ($(wc -c < "$TMP_DIR/offer.txt") bytes)"

# Verify QR is also rendered (Phase 5.5 commit 1).
QR_DIM=$($PWC --raw eval \
    "() => { const q = document.querySelector('.qr-svg svg'); \
       return q ? q.getAttribute('width') + 'x' + q.getAttribute('height') : 'no QR'; }")
# Strip surrounding double quotes that --raw JSON-encodes.
QR_DIM="${QR_DIM%\"}"
QR_DIM="${QR_DIM#\"}"
[[ "$QR_DIM" =~ ^[0-9]+x[0-9]+$ ]] || fail "host QR not rendered: $QR_DIM"
ok "host offer QR rendered: $QR_DIM"

step "Paste offer into joiner tab + click Generate answer"
$PWC tab-select 1 > /dev/null
OFFER_JSON=$(node -e "console.log(JSON.stringify(require('fs').readFileSync('$TMP_DIR/offer.txt','utf8')))")
$PWC run-code "async (page) => { \
    await page.locator('textarea').first().fill($OFFER_JSON); \
    await page.locator('textarea').first().dispatchEvent('input'); \
    await page.getByRole('button', { name: 'Generate answer' }).click(); \
}" > /dev/null
sleep 5

ANSWER_RAW=$($PWC --raw eval "() => document.querySelectorAll('textarea')[1].value")
echo "$ANSWER_RAW" | node -e "console.log(JSON.parse(require('fs').readFileSync(0, 'utf8')))" \
    > "$TMP_DIR/answer.txt"
[ -s "$TMP_DIR/answer.txt" ] || fail "answer textarea empty"
ok "answer SDP captured ($(wc -c < "$TMP_DIR/answer.txt") bytes)"

# Verify joiner answer QR rendered too.
QR_DIM=$($PWC --raw eval \
    "() => { const q = document.querySelector('.qr-svg svg'); \
       return q ? q.getAttribute('width') + 'x' + q.getAttribute('height') : 'no QR'; }")
QR_DIM="${QR_DIM%\"}"
QR_DIM="${QR_DIM#\"}"
[[ "$QR_DIM" =~ ^[0-9]+x[0-9]+$ ]] || fail "joiner QR not rendered: $QR_DIM"
ok "joiner answer QR rendered: $QR_DIM"

step "Paste answer into host tab + click Accept answer"
$PWC tab-select 0 > /dev/null
ANSWER_JSON=$(node -e "console.log(JSON.stringify(require('fs').readFileSync('$TMP_DIR/answer.txt','utf8')))")
$PWC run-code "async (page) => { \
    await page.locator('textarea').nth(1).fill($ANSWER_JSON); \
    await page.locator('textarea').nth(1).dispatchEvent('input'); \
    await page.getByRole('button', { name: 'Accept answer' }).click(); \
}" > /dev/null
sleep 12

step "Verify both tabs reached play view"
HOST_STATE=$($PWC --raw eval \
    "() => { const ps = document.querySelectorAll('p'); \
       for (const p of ps) if (p.textContent.includes('to move')) return p.textContent.trim(); \
       return 'no to-move text'; }")
[[ "$HOST_STATE" =~ "to move" ]] || fail "host not in game: $HOST_STATE"
ok "host: $HOST_STATE"

$PWC tab-select 1 > /dev/null
JOIN_STATE=$($PWC --raw eval \
    "() => { const ps = document.querySelectorAll('p'); \
       for (const p of ps) if (p.textContent.includes('to move')) return p.textContent.trim(); \
       return 'no to-move text'; }")
[[ "$JOIN_STATE" =~ "to move" ]] || fail "joiner not in game: $JOIN_STATE"
ok "joiner: $JOIN_STATE"

step "Send chat from host → verify on joiner"
$PWC tab-select 0 > /dev/null
$PWC run-code "async (page) => { \
    await page.locator('input[type=text]').fill('hello from e2e test'); \
    await page.getByRole('button', { name: 'Send' }).click(); \
}" > /dev/null
sleep 2

$PWC tab-select 1 > /dev/null
JOIN_CHAT=$($PWC --raw eval "() => document.querySelector('.chat-log').innerText")
[[ "$JOIN_CHAT" =~ "hello from e2e test" ]] || fail "joiner did not receive chat: $JOIN_CHAT"
ok "joiner received chat: $(echo "$JOIN_CHAT" | tr '\n' ' ')"

step "Cleanup"
$PWC close > /dev/null
ok "browser closed"

printf '\n\033[1;32m✓ All checks passed\033[0m\n'
