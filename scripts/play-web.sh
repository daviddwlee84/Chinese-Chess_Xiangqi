#!/usr/bin/env bash
# Launch one chess-net-server + one Trunk dev server in a tmux session.
# Browser opens to http://127.0.0.1:8080 (Trunk serves the SPA, proxies WS
# at /ws and /lobby to chess-net at :7878).
#
# Development only. `trunk serve` builds a much larger debug WASM bundle and
# keeps a dev reload websocket open; for remote users run `make serve-web-prod`
# so chess-net serves `trunk build --release` output with compression.
#
# Usage:
#   scripts/play-web.sh [SESSION] [PORT] [VARIANT] [SERVER_ARGS...]
#
# Examples:
#   scripts/play-web.sh                          # xiangqi casual on :7878
#   scripts/play-web.sh chess-web 9000 banqi --preset taiwan --seed 42
#
# Detach with Ctrl-b d. Kill with `make stop-web`.

set -euo pipefail

SESSION="${1:-chess-web}"
PORT="${2:-7878}"
VARIANT="${3:-xiangqi}"
shift 3 2>/dev/null || true
SERVER_EXTRA=("$@")

if ! command -v tmux >/dev/null 2>&1; then
  echo "tmux not found on PATH. Install tmux to use this launcher." >&2
  exit 1
fi
if ! command -v trunk >/dev/null 2>&1; then
  echo "trunk not found on PATH. Install with:" >&2
  echo "  cargo install trunk" >&2
  echo "  rustup target add wasm32-unknown-unknown" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

echo "==> building chess-net-server (debug)..."
cargo build -p chess-net

SERVER_BIN="$REPO_ROOT/target/debug/chess-net-server"
if [[ ! -x "$SERVER_BIN" ]]; then
  echo "build did not produce $SERVER_BIN" >&2
  exit 1
fi

tmux kill-session -t "$SESSION" 2>/dev/null || true

SERVER_CMD="$SERVER_BIN --port $PORT $VARIANT ${SERVER_EXTRA[*]:-}"
TRUNK_CMD="cd $REPO_ROOT/clients/chess-web && trunk serve"

tmux new-session  -d -s "$SESSION" -n server "$SERVER_CMD"
tmux new-window   -t "$SESSION" -n web "$TRUNK_CMD"

# Keep panes visible after the command exits so panics stay readable.
# Without this, a fast crash (e.g. trunk proxy route conflict) auto-kills
# the pane before any log scrolls — see
# pitfalls/trunk-proxy-route-conflict.md. After a crash, attach to the
# pane and press Enter to dismiss; the rest of the session keeps running.
tmux set-option -w -t "${SESSION}:server" remain-on-exit on
tmux set-option -w -t "${SESSION}:web" remain-on-exit on

tmux select-window -t "${SESSION}:web"

cat <<EOF

  chess-web tmux session '$SESSION' is running.

    server : ws://127.0.0.1:$PORT     (window 'server')
    SPA    : http://127.0.0.1:8080    (window 'web')

  Trunk recompiles chess-web on save. The browser auto-reloads via WS
  to Trunk's port (8080). Switch windows with prefix + n / prefix + p.

EOF

if [[ -n "${TMUX:-}" ]]; then
  exec tmux switch-client -t "$SESSION"
else
  exec tmux attach -t "$SESSION"
fi
