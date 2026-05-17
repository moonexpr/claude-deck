#!/bin/bash
# Production build script
# Builds the frontend for production deployment

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "Building Claude Deck for production..."

# Build frontend (the backend binary serves frontend/dist directly)
echo ""
echo "Building frontend..."
cd "$PROJECT_ROOT/frontend"
npm run build

# Build the Rust backend (release)
echo ""
echo "Building backend (release)..."
cd "$PROJECT_ROOT/backend"
cargo build --release

echo ""
echo "Build complete!"
echo "  Frontend assets: frontend/dist"
echo "  Backend binary:  backend/target/release/backend"
echo ""
echo "To deploy, run the single backend binary (it also serves frontend/dist):"
echo "  cd backend && PORT=8000 ./target/release/backend"
echo ""
echo "Override HOST / PORT / PRESENCE_PUBLIC_URL via environment variables."
