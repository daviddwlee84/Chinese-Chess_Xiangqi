#!/usr/bin/env bash
# Launch one chess-net-server + two chess-tui players + one chess-tui
# spectator in a single tmux session, exercising the v3 spectator + chat
# additions:
#
#   - panes A and B: --connect ws://127.0.0.1:$PORT/ws/$ROOM as players.
#     Press 't' in either pane to open the chat input editor.
#   - pane C:        --connect ws://127.0.0.1:$PORT/ws/$ROOM?role=spectator
#     joins as a spectator. The board is read-only; 't' shows a "spectator"
#     hint and the chat log mirrors the players' conversation.
#   - server window: chess-net-server log.
#
# Address windows by NAME (robust against `base-index`) and use
# `switch-client` instead of `attach` when invoked from inside tmux.
#
# Usage:
#   scripts/play-spectator.sh [SESSION] [PORT] [ROOM] [VARIANT] [SERVER_ARGS...]
#
# Examples:
#   scripts/play-spectator.sh                                # main / xiangqi on :7878
#   scripts/play-spectator.sh chess-spec 7878 derby xiangqi --strict
#   scripts/play-spectator.sh chess-spec 9000 friends banqi --preset taiwan --seed 42

set -euo pipefail

SESSION="${1:-chess-spec}"
PORT="${2:-7878}"
ROOM="${3:-main}"
VARIANT="${4:-xiangqi}"
shift 4 2>/dev/null || true
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

PLAYER_CMD="$CLIENT_BIN --connect ws://127.0.0.1:$PORT/ws/$ROOM"
SPEC_CMD="$CLIENT_BIN --connect ws://127.0.0.1:$PORT/ws/$ROOM?role=spectator"
SERVER_CMD="$SERVER_BIN --port $PORT $VARIANT ${SERVER_EXTRA[*]:-}"

tmux new-session  -d -s "$SESSION" -n clients
tmux new-window   -t "$SESSION" -n server "$SERVER_CMD"

sleep 0.5

# Three vertical-tile clients: A (Red), B (Black), C (spectator).
tmux send-keys    -t "${SESSION}:clients" "$PLAYER_CMD" C-m
tmux split-window -h -t "${SESSION}:clients" "$PLAYER_CMD"
tmux split-window -h -t "${SESSION}:clients" "$SPEC_CMD"
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
