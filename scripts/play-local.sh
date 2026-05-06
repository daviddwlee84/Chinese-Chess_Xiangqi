#!/usr/bin/env bash
# Launch one chess-net-server + two chess-tui --connect clients in a
# single tmux session. The first window ("clients") holds two client panes
# in a horizontal split; the second window ("server") shows the server log.
#
# Windows are addressed by NAME, not index — this is robust against any
# `base-index` / `pane-base-index` you set in ~/.tmux.conf.
#
# Usage:
#   scripts/play-local.sh [SESSION] [PORT] [VARIANT] [SERVER_ARGS...]
#
# Examples:
#   scripts/play-local.sh                          # xiangqi casual on :7878
#   scripts/play-local.sh chess-local 9000 banqi --preset taiwan --seed 42
#   scripts/play-local.sh chess-strict 7878 xiangqi --strict
#
# Detach with Ctrl-b d. Kill with `make stop-local` or
# `tmux kill-session -t SESSION`.

set -euo pipefail

SESSION="${1:-chess-local}"
PORT="${2:-7878}"
VARIANT="${3:-xiangqi}"
shift 3 2>/dev/null || true
SERVER_EXTRA=("$@")

if ! command -v tmux >/dev/null 2>&1; then
  echo "tmux not found on PATH. Install tmux to use this launcher." >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# Pre-build so the panes don't all race-compile on first launch and the
# server bind doesn't lag the client connect.
echo "==> building chess-net-server + chess-tui (debug)..."
cargo build -p chess-net -p chess-tui

SERVER_BIN="$REPO_ROOT/target/debug/chess-net-server"
CLIENT_BIN="$REPO_ROOT/target/debug/chess-tui"

if [[ ! -x "$SERVER_BIN" || ! -x "$CLIENT_BIN" ]]; then
  echo "build did not produce expected binaries:" >&2
  echo "  $SERVER_BIN" >&2
  echo "  $CLIENT_BIN" >&2
  exit 1
fi

# Reset stale session, if any.
tmux kill-session -t "$SESSION" 2>/dev/null || true

CLIENT_CMD="$CLIENT_BIN --connect ws://127.0.0.1:$PORT"
SERVER_CMD="$SERVER_BIN --port $PORT $VARIANT ${SERVER_EXTRA[*]:-}"

# Two windows: "clients" (split into two client panes) and "server".
# Address windows by NAME — `tmux new-session -n` honors `base-index`, so
# letting tmux pick the index avoids "index 1 in use" when the user has
# `set -g base-index 1` in ~/.tmux.conf.
tmux new-session  -d -s "$SESSION" -n clients
tmux new-window   -t "$SESSION" -n server "$SERVER_CMD"

# Tiny grace period so the server has bound :$PORT before clients try
# to connect. The clients also retry on failure, so this is just to
# avoid the first try wasting a backoff.
sleep 0.5

tmux send-keys    -t "${SESSION}:clients" "$CLIENT_CMD" C-m
tmux split-window -h -t "${SESSION}:clients" "$CLIENT_CMD"
tmux select-layout -t "${SESSION}:clients" even-horizontal
tmux select-window -t "${SESSION}:clients"
tmux attach -t "$SESSION"
