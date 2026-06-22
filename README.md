# Claude Deck

**Website**: [claudedeck.org](https://claudedeck.org)

A self-hosted application for visualizing and managing Claude Code configuration. Provides a unified interface for managing MCP servers, plugins, slash commands, hooks, agents, permissions, usage tracking, session transcripts, CC Bridge, AI chat, and other Claude Code extensions. Ships as a Tauri desktop app **and** a standalone Rust binary for LAN/mobile access.

> [!IMPORTANT]
> **This is a fork under heavy active development — not yet pinned to a stable release.** Expect breaking changes, in-flux APIs, and rough edges. If you need stability, pin to a specific commit rather than tracking `main`. This fork re-platforms the original onto a Tauri 2 + Rust + React stack.
>
> Full credit to the original authors, [Adrian Rubio](https://github.com/adrirubio) and [Juan Rubio](https://github.com/juanrubio), whose upstream project lives at [adrirubio/claude-deck](https://github.com/adrirubio/claude-deck).

## Why This Exists

Claude Code starts simple, then slowly sprawls across config files and directories: `~/.claude.json`, `~/.claude/settings.json`, `.mcp.json`, slash commands, agents, skills, project settings, transcripts, and usage data. That works fine at small scale, but once your setup gets serious it becomes hard to see the whole picture, change things confidently, or understand what is actually configured.

Claude Deck gives you one local interface for that sprawl.

## Best For

Claude Deck is best for people running multiple MCP servers, custom commands, hooks, agents, or tracking Claude Code usage across sessions.

If you only use Claude Code casually with mostly default config, Claude Deck may be overkill.

## Trust Model

- **Local only** — no cloud
- **No account** — nothing to sign up for
- **No telemetry** — no usage tracking sent anywhere
- **Works with your real files** — reads and writes existing Claude Code config files

> [!WARNING]
> Claude Deck reads and writes your real Claude Code configuration files. Changes made in the UI affect the files Claude Code actually uses. Review changes carefully, and create a backup before major edits.

## Features

- **Dashboard** — Overview of all Claude Code configurations with context window visualizer
- **Config Editor** — Browse, inspect, and edit configuration files across all scopes (CodeMirror 6 syntax highlighting on read-only JSON renders)
- **MCP Servers** — Add, edit, test, and manage MCP server connections with OAuth support. Browse and install servers from the [MCP Registry](https://registry.modelcontextprotocol.io). View tools, resources, and prompts. Supports stdio, HTTP, and SSE transports
- **Slash Commands** — Browse, create, and edit custom commands (user and project scope) with CodeMirror 6 markdown editing
- **Plugins** — Browse installed plugins with detail views and enable/disable toggles
- **Hooks** — Configure automation hooks by event type (PreToolUse, PostToolUse, etc.)
- **Permissions** — Visual allow/deny rule builder for tool access control
- **Agents** — Create and manage custom agent configurations; ✨ AI suggest writes a draft into the CodeMirror buffer (reusable pattern for Commands / Skills / Memory / Output Styles / Hooks)
- **Skills** — Browse installed skills and discover new ones from [skills.sh](https://skills.sh)
- **Memory** — View and edit Claude Code memory files
- **Output Styles** — Configure response output formats
- **Status Line** — Customize Claude Code status line display
- **CC Bridge** — Discover, spawn, and monitor Claude Code sessions. Hosts the child PTY directly via `portable-pty` over WebSocket binary frames. Optional AI panel suggests shell commands and renders them as `[Send] / [Edit] / [Discard]` preview cards — **the model never reaches the PTY without an explicit Send click**.
- **Chat panel** — Streaming chat against Anthropic via a server-side proxy. The API key stays on the server; conversations persist to SQLite via `sqlx::migrate!`.
- **Session Transcripts** — View conversation history with full message details and tool use
- **Usage Tracking** — Monitor token usage, costs, and billing blocks with daily/monthly charts
- **Plan History** — Browse and review Claude Code implementation plans
- **Backup & Restore** — Create and manage configuration backups with selective restore
- **Projects** — Discover and manage project directories
- **Mobile-friendly** — Responsive layout, scrollable sidebar drawer; usable from a phone or tablet over LAN/tailnet, not just the desktop app

## Screenshots

| Dashboard | MCP Servers |
|-----------|-------------|
| ![Dashboard](screenshots/dashboard.png) | ![MCP Servers](screenshots/mcp-servers.png) |
| High-level overview of your Claude Code setup | Manage MCP connections, status, and configuration |

| Usage Tracking | Session Transcripts |
|----------------|---------------------|
| ![Usage Tracking](screenshots/usage-tracking.png) | ![Session Transcripts](screenshots/sessions.png) |
| Cost visibility, charts, and billing blocks | Browse conversation history and tool usage details |

| CC Bridge | Skills |
|-----------|--------|
| ![CC Bridge](screenshots/cc-bridge.png) | ![Skills](screenshots/skills.png) |
| Spawn and interact with Claude Code sessions (PTY-hosted) | Browse installed skills and discover new ones |

## Tech Stack

| Layer | Technology |
|-------|------------|
| Desktop shell | Tauri 2 (Rust) |
| Server | Rust 2024 · axum 0.8 · sqlx (with `sqlx::migrate!`) · portable-pty |
| Web UI | React 19 · TypeScript · Vite 7 · Tailwind 4 (`@theme`) · shadcn/ui · Zustand · CodeMirror 6 · xterm.js · Vercel AI SDK v6 |
| Database | SQLite (async via sqlx) |
| AI | Anthropic Messages API via server-side proxy (Vercel Data Stream v1 to the browser); key resolved from OS keychain (desktop) or `ANTHROPIC_API_KEY` env (server-bin) |

## Layout

```
app/
├── server/     # Cargo workspace — core lib + bin (HTTP/WS backend)
├── desktop/   # Tauri 2 — embeds server/core in-process; OS keychain for AI key
└── web/        # React/Vite — UI bundle, served by both Tauri and server-bin
```

See [`app/README.md`](app/README.md) for layout detail, dev workflow, and migration notes.

## Manual Installation

**Prerequisites**: Rust 1.85+ (edition 2024, via [rustup](https://rustup.rs/)), Node.js 18+.

```bash
git clone https://github.com/moonexpr/claude-deck.git
cd claude-deck
./scripts/install.sh
```

Optional desktop app:

```bash
cargo install tauri-cli --version '^2.0'
cd app/desktop && cargo tauri dev
```

## Development

```bash
./scripts/dev.sh
```

This starts:
- Server (axum) at http://localhost:8000 (REST API under `/api/v1/`, health check at `/health`)
- Web dev server (Vite) at http://localhost:5173

The first `./scripts/dev.sh` run compiles the Rust server (`cargo run`), which can take a few minutes; subsequent runs are incremental.

### Hostname & remote access

To reach the dev environment from another machine on your LAN or tailnet, pass `--host`:

```bash
./scripts/dev.sh --host 0.0.0.0
```

The server also honors environment variables (useful for the production binary / launchd / systemd):

| Variable | Default | Purpose |
|----------|---------|---------|
| `HOST` | `0.0.0.0` | Address the server binds to |
| `PORT` | `8000` | Port the server listens on |
| `PRESENCE_PUBLIC_URL` | *(auto)* | Base URL embedded in the CC Bridge / Presence setup snippet |
| `ANTHROPIC_API_KEY` | *(unset)* | Enables the AI proxy (server-bin path; Tauri uses the OS keychain instead) |
| `ANTHROPIC_BASE_URL` | `https://api.anthropic.com` | Override for staging/test or self-hosted Anthropic-compatible endpoints |
| `FRONTEND_DIST` | *(unset)* | Path to the built web bundle; required when serving the UI from server-bin in production |

`PRESENCE_PUBLIC_URL` only needs to be set if autodetection is wrong. When it is unset, the public URL is **auto-derived from the incoming request** — the `Host` header plus `X-Forwarded-Proto`.

## Production Build

```bash
./scripts/build.sh
```

Produces `app/web/dist/` and `app/server/target/release/server-bin`. Run with:

```bash
FRONTEND_DIST=$PWD/app/web/dist HOST=0.0.0.0 PORT=8000 \
  app/server/target/release/server-bin
```

## Configuration Files

Claude Deck reads and writes these Claude Code configuration files:

| File/Directory | Scope | Description |
|---------------|-------|-------------|
| `~/.claude.json` | User | OAuth, caches, MCP servers |
| `~/.claude/settings.json` | User | User settings, permissions, disabled servers |
| `~/.claude/settings.local.json` | User | Local overrides (not committed) |
| `~/.claude/commands/` | User | User slash commands |
| `~/.claude/agents/` | User | User agents |
| `~/.claude/skills/` | User | User skills |
| `~/.claude/projects/` | User | Session transcripts & usage data |
| `.claude/settings.json` | Project | Project settings |
| `.claude/commands/` | Project | Project slash commands |
| `.mcp.json` | Project | Project MCP servers |
| `CLAUDE.md` | Project | Project instructions |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, style, and PR guidelines.

The server is a Rust/axum service; its REST API is served under `/api/v1/`. There is no auto-generated Swagger UI — the route modules in `app/server/core/src/api/v1/` are the API reference.

## Feedback

If you use Claude Code heavily, issues and feature requests are especially welcome.

## Built By

[Adrian](https://github.com/adrirubio) (13) and [Juan](https://github.com/juanrubio) during the 2025 Christmas break as a learning project — to explore open source, Claude Code, and full-stack development together.

## Acknowledgments

The session transcript viewer was inspired by and includes code adapted from [claude-code-transcripts](https://github.com/simonw/claude-code-transcripts) by [Simon Willison](https://simonwillison.net/).

The usage tracking feature ports algorithms from [ccusage](https://github.com/ryoppippi/ccusage) by [ryoppippi](https://github.com/ryoppippi), including session block identification, tiered pricing, and burn rate projections.

## Disclaimer

Claude Deck is a community project and is not affiliated with or endorsed by Anthropic.

## License

MIT License
