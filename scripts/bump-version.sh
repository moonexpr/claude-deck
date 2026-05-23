#!/bin/bash
# bump-version.sh - Sync version across all project files
#
# Usage: ./scripts/bump-version.sh [version]
#   If version is provided, updates VERSION file and syncs to all files
#   If no version provided, reads from VERSION file and syncs to all files

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

VERSION_FILE="$PROJECT_ROOT/VERSION"

# Legacy stack (still shipping from main until the cleanup PR)
FRONTEND_PACKAGE_JSON="$PROJECT_ROOT/frontend/package.json"

# Lift stack (app/ — Tauri 2 + Rust sidecar + Vite/React)
APP_WEB_PACKAGE_JSON="$PROJECT_ROOT/app/web/package.json"
APP_SERVER_CORE_CARGO="$PROJECT_ROOT/app/server/core/Cargo.toml"
APP_SERVER_BIN_CARGO="$PROJECT_ROOT/app/server/bin/Cargo.toml"
APP_DESKTOP_CARGO="$PROJECT_ROOT/app/desktop/src-tauri/Cargo.toml"
APP_TAURI_CONF="$PROJECT_ROOT/app/desktop/src-tauri/tauri.conf.json"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

validate_semver() {
    local version="$1"
    if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?(\+[a-zA-Z0-9.]+)?$ ]]; then
        log_error "Invalid semantic version: $version"
        log_error "Expected format: MAJOR.MINOR.PATCH (e.g., 1.0.0, 2.1.3-beta)"
        exit 1
    fi
}

# Get version from argument or VERSION file
if [ -n "$1" ]; then
    VERSION="$1"
    validate_semver "$VERSION"
    echo "$VERSION" > "$VERSION_FILE"
    log_info "Updated VERSION file to $VERSION"
else
    if [ ! -f "$VERSION_FILE" ]; then
        log_error "VERSION file not found at $VERSION_FILE"
        log_error "Usage: $0 <version>"
        exit 1
    fi
    VERSION=$(cat "$VERSION_FILE" | tr -d '[:space:]')
    validate_semver "$VERSION"
    log_info "Reading version from VERSION file: $VERSION"
fi

# ---------- JSON: package.json files (jq if available, else sed) ----------

update_package_json() {
    local file="$1"
    if [ ! -f "$file" ]; then
        log_warn "File not found: $file"
        return
    fi
    if command -v jq &> /dev/null; then
        jq --arg v "$VERSION" '.version = $v' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
    else
        sed -i.bak "s/\"version\": \"[^\"]*\"/\"version\": \"$VERSION\"/" "$file" && rm -f "$file.bak"
    fi
    log_info "Updated $file to version $VERSION"
}

update_package_json "$FRONTEND_PACKAGE_JSON"
update_package_json "$APP_WEB_PACKAGE_JSON"

# ---------- Cargo.toml: only the top-level [package] version, not [dependencies] ----------
#
# `^version = "..."` anchored to a line start; matches the package version, not
# crate version strings under [dependencies].

update_cargo_toml() {
    local file="$1"
    if [ ! -f "$file" ]; then
        log_warn "File not found: $file"
        return
    fi
    sed -i.bak -E "0,/^version = \"[^\"]*\"/{s/^version = \"[^\"]*\"/version = \"$VERSION\"/}" "$file" && rm -f "$file.bak"
    log_info "Updated $file to version $VERSION"
}

update_cargo_toml "$APP_SERVER_CORE_CARGO"
update_cargo_toml "$APP_SERVER_BIN_CARGO"
update_cargo_toml "$APP_DESKTOP_CARGO"

# ---------- tauri.conf.json: top-level "version" field ----------

update_tauri_conf() {
    local file="$1"
    if [ ! -f "$file" ]; then
        log_warn "File not found: $file"
        return
    fi
    if command -v jq &> /dev/null; then
        jq --arg v "$VERSION" '.version = $v' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
    else
        sed -i.bak "s/\"version\": \"[^\"]*\"/\"version\": \"$VERSION\"/" "$file" && rm -f "$file.bak"
    fi
    log_info "Updated $file to version $VERSION"
}

update_tauri_conf "$APP_TAURI_CONF"

log_info "Version sync complete: v$VERSION"

# Display current versions for verification
echo ""
echo "Current versions:"
echo "  VERSION file:                $(cat "$VERSION_FILE" | tr -d '[:space:]')"
if [ -f "$FRONTEND_PACKAGE_JSON" ]; then
    echo "  frontend/package.json:       $(grep '"version"' "$FRONTEND_PACKAGE_JSON" | head -1 | sed 's/.*"version": "\([^"]*\)".*/\1/')"
fi
if [ -f "$APP_WEB_PACKAGE_JSON" ]; then
    echo "  app/web/package.json:        $(grep '"version"' "$APP_WEB_PACKAGE_JSON" | head -1 | sed 's/.*"version": "\([^"]*\)".*/\1/')"
fi
if [ -f "$APP_SERVER_CORE_CARGO" ]; then
    echo "  app/server/core/Cargo.toml:  $(grep '^version' "$APP_SERVER_CORE_CARGO" | head -1 | sed 's/version = "\([^"]*\)"/\1/')"
fi
if [ -f "$APP_SERVER_BIN_CARGO" ]; then
    echo "  app/server/bin/Cargo.toml:   $(grep '^version' "$APP_SERVER_BIN_CARGO" | head -1 | sed 's/version = "\([^"]*\)"/\1/')"
fi
if [ -f "$APP_DESKTOP_CARGO" ]; then
    echo "  app/desktop Cargo.toml:      $(grep '^version' "$APP_DESKTOP_CARGO" | head -1 | sed 's/version = "\([^"]*\)"/\1/')"
fi
if [ -f "$APP_TAURI_CONF" ]; then
    echo "  tauri.conf.json:             $(grep '"version"' "$APP_TAURI_CONF" | head -1 | sed 's/.*"version": "\([^"]*\)".*/\1/')"
fi
