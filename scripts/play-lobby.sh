#!/usr/bin/env bash
# Launch one chess-net-server + three chess-tui panes in a single tmux
# session, exercising the multi-room lobby flow:
#
#   - pane A and B: open the variant picker → pick "Connect to server…" →
#     enter ws://127.0.0.1:$PORT in the host prompt → land in the lobby.
#     One creates a room, the other picks it from the live list.
#   - pane C:       launches with --lobby ws://127.0.0.1:$PORT directly,
#     so it skips the picker and watches the room appear/fill in real time.
#
# Address windows by NAME (robust against `base-index` settings) and use
# `switch-client` instead of `attach` when invoked from inside tmux.
#
# Usage:
#   scripts/play-lobby.sh [SESSION] [PORT] [VARIANT] [SERVER_ARGS...]
#
# Examples:
#   scripts/play-lobby.sh                       # xiangqi casual on :7878
#   scripts/play-lobby.sh chess-lobby 9000 banqi --preset taiwan --seed 42

set -euo pipefail

SESSION="${1:-chess-lobby}"
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

tmux kill-session -t "$SESSION" 2>/dev/null || true

# Pane A & B drop the user into the variant picker (no flags). They go
# through the lobby flow manually — pick "Connect to server…", type the
# host, etc. Pane C uses --lobby to skip straight to the browser.
CLIENT_PICK="$CLIENT_BIN"
CLIENT_LOBBY="$CLIENT_BIN --lobby ws://127.0.0.1:$PORT"
SERVER_CMD="$SERVER_BIN --port $PORT $VARIANT ${SERVER_EXTRA[*]:-}"

tmux new-session  -d -s "$SESSION" -n clients
tmux new-window   -t "$SESSION" -n server "$SERVER_CMD"

sleep 0.5

# Three vertical-tile clients: A (picker), B (picker), C (--lobby watcher).
tmux send-keys    -t "${SESSION}:clients" "$CLIENT_PICK" C-m
tmux split-window -h -t "${SESSION}:clients" "$CLIENT_PICK"
tmux split-window -h -t "${SESSION}:clients" "$CLIENT_LOBBY"
tmux select-layout -t "${SESSION}:clients" even-horizontal
tmux select-window -t "${SESSION}:clients"

if [[ -n "${TMUX:-}" ]]; then
  echo
  echo "Already inside tmux — switching this client to '$SESSION'."
  echo "  - prefix + d  detach back to your shell"
  echo "  - prefix + s  pick another session (your previous one is still alive)"
  exec tmux switch-client -t "$SESSION"
else
  exec tmux attach -t "$SESSION"
fi
