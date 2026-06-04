# Installation

## Docker

::: warning
The Docker image is being reworked for the Rust backend
([#2](https://github.com/moonexpr/claude-deck/issues/2)). Until that lands, use
the manual installation below.
:::

```bash
git clone https://github.com/moonexpr/claude-deck.git
cd claude-deck
docker compose up
```

This is intended to build and start Claude Deck at `http://localhost:8000`,
mounting your `~/.claude` and `~/.claude.json` configuration files.

## Manual Installation

### Prerequisites

- **Rust 1.85+** (edition 2024) via [rustup](https://rustup.rs/)
- **Node.js 18+**

### Steps

1. Clone the repository:

```bash
git clone https://github.com/moonexpr/claude-deck.git
cd claude-deck
```

2. Run the install script:

```bash
./scripts/install.sh
```

This script:
- Compiles the Rust server crates (`server-core` + `server-bin`) under `app/server/`
- Installs Node.js dependencies in `app/web/`
- The SQLite database is created on first run (no init step)

3. Verify the installation:

```bash
# Check backend
cd app/server && cargo build -p server-bin

# Check frontend
cd app/web && npm run build
```

## Configuration

Claude Deck requires no configuration files — all settings have sensible defaults.
The SQLite database (`claude_registry.db`) is created automatically on first run.
By default the server listens on a unix socket; pass `--tcp` to `scripts/dev.sh`
for TCP on `http://localhost:8000`.

## What Gets Read

Claude Deck reads these Claude Code configuration files:

| File/Directory | Scope | Description |
|----------------|-------|-------------|
| `~/.claude.json` | User | OAuth, caches, MCP servers |
| `~/.claude/settings.json` | User | User settings, permissions |
| `~/.claude/settings.local.json` | User | Local overrides |
| `~/.claude/commands/` | User | User slash commands |
| `~/.claude/agents/` | User | User agents |
| `~/.claude/skills/` | User | User skills |
| `~/.claude/projects/` | User | Session transcripts & usage |
| `.claude/settings.json` | Project | Project settings |
| `.claude/commands/` | Project | Project commands |
| `.mcp.json` | Project | Project MCP servers |
| `CLAUDE.md` | Project | Project instructions |
