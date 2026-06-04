#!/bin/bash
# Development server startup script — starts server-bin (axum) and the Vite
# dev server for the app/ tree.
#
# By default the server binds to a Unix socket and Vite proxies /api to it,
# so the whole stack can sit behind an nginx upstream for local networking.
#
# Usage:
#   ./scripts/dev.sh                            # unix socket at /tmp/claude-deck.sock
#   ./scripts/dev.sh --socket /path/to.sock     # unix socket at a custom path
#   ./scripts/dev.sh --tcp                      # legacy TCP on localhost:8000
#   ./scripts/dev.sh --tcp --host 0.0.0.0       # TCP, bound to all interfaces
#   ./scripts/dev.sh --web-host 0.0.0.0         # expose Vite on LAN; server still on socket

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

MODE="socket"            # socket | tcp
SOCKET_PATH_ARG=""
TCP_HOST=""
WEB_HOST=""

usage() {
    cat <<EOF
Usage: $0 [--socket <path>] [--tcp] [--host <host>] [--web-host <host>]

Options:
  --socket <path>   Bind server-bin to the given unix socket path
                    (default: \$SOCKET_PATH or /tmp/claude-deck.sock)
  --tcp             Use TCP instead of a unix socket (legacy mode)
  --host <host>     With --tcp: bind server-bin to this host (e.g. 0.0.0.0)
  --web-host <host> Bind Vite dev server to this host (e.g. 0.0.0.0)
  -h, --help        Show this help message
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --socket)
            if [ -z "${2:-}" ]; then
                echo "Error: --socket requires a value."
                usage
                exit 1
            fi
            MODE="socket"
            SOCKET_PATH_ARG="$2"
            shift 2
            ;;
        --socket=*)
            MODE="socket"
            SOCKET_PATH_ARG="${1#*=}"
            shift
            ;;
        --tcp)
            MODE="tcp"
            shift
            ;;
        --host)
            if [ -z "${2:-}" ]; then
                echo "Error: --host requires a value."
                usage
                exit 1
            fi
            TCP_HOST="$2"
            shift 2
            ;;
        --host=*)
            TCP_HOST="${1#*=}"
            shift
            ;;
        --web-host)
            if [ -z "${2:-}" ]; then
                echo "Error: --web-host requires a value."
                usage
                exit 1
            fi
            WEB_HOST="$2"
            shift 2
            ;;
        --web-host=*)
            WEB_HOST="${1#*=}"
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

SERVER_ENV=()
WEB_ENV=()
WEB_HOST_ARGS=()

if [ "$MODE" = "socket" ]; then
    SOCKET_PATH="${SOCKET_PATH_ARG:-${SOCKET_PATH:-/tmp/claude-deck.sock}}"
    SERVER_ENV=(SOCKET_PATH="$SOCKET_PATH")
    WEB_ENV=(SOCKET_PATH="$SOCKET_PATH")
    SERVER_LISTEN_DESC="unix:${SOCKET_PATH}"
else
    TCP_HOST="${TCP_HOST:-127.0.0.1}"
    SERVER_ENV=(HOST="$TCP_HOST" PORT=8000)
    WEB_ENV=(SERVER_URL="http://${TCP_HOST}:8000")
    SERVER_LISTEN_DESC="http://${TCP_HOST}:8000"
fi

if [ -n "$WEB_HOST" ]; then
    WEB_HOST_ARGS=(-- --host "$WEB_HOST")
    echo "Vite bound to host: $WEB_HOST"
fi
DISPLAY_WEB_HOST="${WEB_HOST:-localhost}"

# Start server-bin
echo "Starting server-bin on ${SERVER_LISTEN_DESC}..."
cd "$PROJECT_ROOT/app/server"
env "${SERVER_ENV[@]}" cargo run -p server-bin &
SERVER_PID=$!

# Start Vite
echo "Starting web dev server on http://${DISPLAY_WEB_HOST}:5173..."
cd "$PROJECT_ROOT/app/web"
env "${WEB_ENV[@]}" npm run dev "${WEB_HOST_ARGS[@]}" &
WEB_PID=$!

echo ""
echo "Development servers started!"
echo "  - Server: ${SERVER_LISTEN_DESC} (REST API under /api/v1, health at /health)"
echo "  - Web:    http://${DISPLAY_WEB_HOST}:5173"
echo ""
echo "Press Ctrl+C to stop all servers."

# Wait for either process to exit, then clean up the other
wait -n 2>/dev/null || true
cleanup
