#!/bin/bash
# Production build script.
#   1. Builds app/web for production (output: app/web/dist).
#   2. Builds server-bin in release mode (output: app/server/target/release/server-bin).
#
# The server-bin reads FRONTEND_DIST env to locate the bundle.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "Building Claude Deck for production..."

echo ""
echo "Building web bundle..."
cd "$PROJECT_ROOT/app/web"
npm run build

echo ""
echo "Building server-bin (release)..."
cd "$PROJECT_ROOT/app/server"
cargo build --release -p server-bin

echo ""
echo "Build complete:"
echo "  Web bundle:   $PROJECT_ROOT/app/web/dist"
echo "  Server:       $PROJECT_ROOT/app/server/target/release/server-bin"
echo ""
echo "Production run:"
echo "  FRONTEND_DIST=$PROJECT_ROOT/app/web/dist HOST=0.0.0.0 PORT=8000 \\"
echo "    $PROJECT_ROOT/app/server/target/release/server-bin"
echo ""
echo "Override HOST / PORT / PRESENCE_PUBLIC_URL / ANTHROPIC_API_KEY via environment variables."
