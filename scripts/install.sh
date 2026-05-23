#!/bin/bash
# Initial setup script — installs dependencies for the app/ tree.
#
# Prerequisites: Rust 1.85+ (edition 2024) via rustup, Node.js 18+.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "Setting up Claude Deck..."

# Check Rust
if ! command -v cargo &> /dev/null; then
    echo "Error: Rust/Cargo not found. Please install Rust (https://rustup.rs/)."
    exit 1
fi
echo "Found Rust: $(cargo --version)"

# Check Node.js
if ! command -v npm &> /dev/null; then
    echo "Error: Node.js/npm not found. Please install Node 18+."
    exit 1
fi
echo "Found Node: $(node --version)"

# Compile server-core + server-bin (the embedded backend + standalone binary)
echo ""
echo "Building server (this can take a few minutes on first run)..."
cd "$PROJECT_ROOT/app/server"
cargo build -p server-core -p server-bin

# Install web dependencies
echo ""
echo "Installing web dependencies..."
cd "$PROJECT_ROOT/app/web"
npm install

# Database is created on first server-bin run via sqlx::migrate! — no init step.

echo ""
echo "Setup complete. Run ./scripts/dev.sh to start both servers."
echo ""
echo "Optional: install Tauri for the desktop app:"
echo "  cargo install tauri-cli --version '^2.0'"
echo "  cd app/desktop && cargo tauri dev"
