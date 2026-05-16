#!/bin/bash
# Initial setup script
# Creates virtual environment, installs dependencies, initializes database

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "Setting up Claude Deck..."

# Check Rust
if ! command -v cargo &> /dev/null; then
    echo "Error: Rust/Cargo not found. Please install Rust (https://rustup.rs/)."
    exit 1
fi

RUST_VERSION=$(cargo --version)
echo "Found Rust $RUST_VERSION"

# Check Node.js
if ! command -v node &> /dev/null; then
    echo "Error: Node.js not found. Please install Node.js 18+."
    exit 1
fi

NODE_VERSION=$(node --version)
echo "Found Node.js $NODE_VERSION"

# Setup backend
echo ""
echo "Setting up backend..."
cd "$PROJECT_ROOT/backend"

echo "Building Rust backend..."
cargo build

# Initialize database (handled on first run by axum startup)
echo "Backend setup complete!"

# Setup frontend
echo ""
echo "Setting up frontend..."
cd "$PROJECT_ROOT/frontend"

echo "Installing Node.js dependencies..."
npm install

echo "Frontend setup complete!"

# Create required directories
echo ""
echo "Creating required directories..."
mkdir -p ~/.claude-registry/backups

echo ""
echo "Setup complete!"
echo ""
echo "To start development servers, run:"
echo "  ./scripts/dev.sh"
