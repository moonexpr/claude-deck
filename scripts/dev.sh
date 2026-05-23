#!/bin/bash
# Development server startup script — starts server-bin (axum) and the Vite
# dev server for the app/ tree.
#
# Usage:
#   ./scripts/dev.sh                 # bind to 127.0.0.1
#   ./scripts/dev.sh --host 0.0.0.0  # bind to all interfaces (LAN/tailnet)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

HOST=""

usage() {
    cat <<EOF
Usage: $0 [--host <host>]

Options:
  --host <host>   Bind both server and web dev to the given host (e.g. 0.0.0.0)
  -h, --help      Show this help message
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host)
            if [ -z "${2:-}" ]; then
                echo "Error: --host requires a value."
                usage
                exit 1
            fi
            HOST="$2"
            shift 2
            ;;
        --host=*)
            HOST="${1#*=}"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1"
            usage
            exit 1
            ;;
    esac
done

echo "Starting Claude Deck development servers..."

if [ ! -f "$PROJECT_ROOT/app/server/Cargo.toml" ]; then
    echo "Error: app/server/Cargo.toml not found."
    exit 1
fi
if [ ! -d "$PROJECT_ROOT/app/web/node_modules" ]; then
    echo "Error: Web dependencies not installed."
    echo "Run ./scripts/install.sh first."
    exit 1
fi

SERVER_PID=""
WEB_PID=""
CLEANED_UP=0

# Kill a process and its entire descendant tree.
kill_tree() {
    local pid="$1"
    [ -z "$pid" ] && return 0
    local pids="$pid"
    local frontier="$pid"
    while [ -n "$frontier" ]; do
        local next
        next="$(pgrep -P $frontier 2>/dev/null | tr '\n' ' ')"
        [ -z "$next" ] && break
        pids="$pids $next"
        frontier="$next"
    done
    kill -TERM $pids 2>/dev/null || true
    sleep 1
    kill -KILL $pids 2>/dev/null || true
}

cleanup() {
    [ "$CLEANED_UP" -eq 1 ] && return
    CLEANED_UP=1
    echo ""
    echo "Shutting down servers..."
    kill_tree "$SERVER_PID"
    kill_tree "$WEB_PID"
    wait 2>/dev/null || true
}

trap 'cleanup; exit 130' SIGINT
trap 'cleanup; exit 143' SIGTERM
trap cleanup EXIT

SERVER_HOST_ENV=()
WEB_HOST_ARGS=()
if [ -n "$HOST" ]; then
    SERVER_HOST_ENV=(HOST="$HOST")
    WEB_HOST_ARGS=(-- --host "$HOST")
    echo "Binding servers to host: $HOST"
fi

# Start server-bin
DISPLAY_HOST="${HOST:-localhost}"
echo "Starting server-bin on http://${DISPLAY_HOST}:8000..."
cd "$PROJECT_ROOT/app/server"
env "${SERVER_HOST_ENV[@]}" PORT=8000 cargo run -p server-bin &
SERVER_PID=$!

# Start Vite
echo "Starting web dev server on http://${DISPLAY_HOST}:5173..."
cd "$PROJECT_ROOT/app/web"
npm run dev "${WEB_HOST_ARGS[@]}" &
WEB_PID=$!

echo ""
echo "Development servers started!"
echo "  - Server: http://${DISPLAY_HOST}:8000 (REST API under /api/v1, health at /health)"
echo "  - Web:    http://${DISPLAY_HOST}:5173"
echo ""
echo "Press Ctrl+C to stop all servers."

# Wait for either process to exit, then clean up the other
wait -n 2>/dev/null || true
cleanup
