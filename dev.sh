#!/usr/bin/env bash
set -e

MOUNTPOINT="${1:-/tmp/mntfuse}"
ADDR="${2:-127.0.0.1:19310}"
SERVER_ROOT="${3:-/tmp/azpfsroot}"

cargo build || exit 1

mkdir -p "$MOUNTPOINT" "$SERVER_ROOT"

# Open a new tmux window; capture the initial (left) pane ID
LEFT=$(tmux new-window -P -F '#{pane_id}')

# Split right → server pane
RIGHT=$(tmux split-window -h -P -F '#{pane_id}' -t "$LEFT")

# Split the left pane down → bottom-left (mountpoint shell)
BOTTOM=$(tmux split-window -v -P -F '#{pane_id}' -t "$LEFT")

# Split the right pane down → bottom-right (server root shell)
BOTTOM_RIGHT=$(tmux split-window -v -P -F '#{pane_id}' -t "$RIGHT")

# Right pane: run server
tmux send-keys -t "$RIGHT" "cargo run --bin azpfs-server -- $ADDR $(printf '%q' "$SERVER_ROOT")" Enter

# Top-left pane: wait for server port, then run client
HOST="${ADDR%:*}"
PORT="${ADDR##*:}"
tmux send-keys -t "$LEFT" \
    "until nc -z $HOST $PORT 2>/dev/null; do sleep 0.2; done; cargo run --bin azpfsd -- $(printf '%q' "$MOUNTPOINT") $ADDR" Enter

# Bottom-left pane: wait for FUSE mount, then cd into mountpoint
tmux send-keys -t "$BOTTOM" \
    "until mountpoint -q $(printf '%q' "$MOUNTPOINT") 2>/dev/null; do sleep 0.2; done; cd $(printf '%q' "$MOUNTPOINT")" Enter

# Bottom-right pane: cd into server root
tmux send-keys -t "$BOTTOM_RIGHT" "cd $(printf '%q' "$SERVER_ROOT")" Enter

# Focus the mountpoint pane
tmux select-pane -t "$BOTTOM"
