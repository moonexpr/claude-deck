# Architecture

Claude Deck is a full-stack application with a Rust backend and React frontend.

## Overview

```
┌─────────────────────────┐     ┌─────────────────────────┐
│   Frontend (React 19)   │────▶│   Backend (axum)         │
│   Port 5173 (dev)       │     │   Rust + Tokio           │
│   Vite + TypeScript     │     │   unix socket or :8000   │
│   shadcn/ui + Tailwind  │     │   sqlx + SQLite          │
└─────────────────────────┘     └──────────┬──────────────┘
                                           │
                                    ┌──────▼──────┐
                                    │  ~/.claude/  │
                                    │  Claude Code │
                                    │  config files│
                                    └─────────────┘
```

## Backend

**Stack:** Rust + axum + Tokio + sqlx (SQLite) + tower-http + tracing

The backend is a Cargo workspace at `app/server/` with two crates:

```
app/server/
├── core/                # server-core: the library (all logic + routes)
│   ├── src/
│   │   ├── lib.rs       # router assembly, app state
│   │   ├── api/v1/      # route modules (one file per feature)
│   │   ├── services/    # business logic (incl. ai/ proxy to Anthropic)
│   │   ├── models.rs    # serde request/response types
│   │   ├── paths.rs     # ~/.claude path resolution
│   │   ├── fileio.rs    # config file read/write
│   │   └── error.rs     # error types → HTTP responses
│   └── tests/           # integration tests (routes, cc_bridge, ai_proxy, …)
└── bin/                 # server-bin: the executable entrypoint
    └── src/main.rs      # binds socket/port, serves API + frontend bundle
```

### API Design

All routes live under `/api/v1/`, defined per-feature in `app/server/core/src/api/v1/`.
In development the Vite dev server proxies `/api` to the backend; by default the
backend listens on a **unix socket** (`SOCKET_PATH`, e.g. `/tmp/claude-deck.sock`)
and Vite proxies to it, so the whole stack can sit behind an nginx upstream. Pass
`--tcp` to `scripts/dev.sh` for legacy TCP on `localhost:8000`.

Route modules: `status` (health), `config`, `projects`, `cli`, `mcp`, `commands`,
`plugins`, `hooks`, `permissions`, `agents`, `backup`, `output-styles`,
`statusline`, `sessions`, `cc-bridge`, `usage`, `memory`, `chat`, `plans`,
`context`, `presence`, `ai`.

### Serving the frontend

In production a single `server-bin` process serves both the REST API and the built
web bundle. It reads `FRONTEND_DIST` (e.g. `app/web/dist`) and serves the static
assets via `tower-http`'s file service, so no separate web server is required.

### Database

SQLite at `claude_registry.db`, opened by sqlx on startup (`?mode=rwc`, auto-created
on first run). Schema is managed in code — there is no migration system, so schema
changes require recreating the database.

## Frontend

**Stack:** React 19 + Vite 7 + TypeScript + TailwindCSS + shadcn/ui

```
app/web/src/
├── App.tsx              # Routes
├── features/            # Feature modules (one directory per feature)
│   └── <feature>/
│       ├── *Page.tsx    # Main page component
│       ├── components/  # Feature-specific components
│       ├── api.ts       # API calls
│       └── types.ts     # TypeScript types
├── components/
│   ├── layout/          # Sidebar, header
│   ├── shared/          # Reusable components
│   └── ui/              # shadcn/ui primitives
├── hooks/               # Custom React hooks
├── contexts/            # React contexts (Project, Theme, …)
├── types/               # Shared TypeScript types
└── lib/                 # API client, constants, utilities
```

### Feature Modules

Each feature is self-contained in `app/web/src/features/<name>/` with its own page
component, sub-components, API functions, and types.

### State Management

- **ProjectContext** — tracks the active project, persists across navigation
- **ThemeContext** — dark/light mode
- **React Router** — client-side routing with sidebar navigation

## Key Decisions

| Decision | Rationale |
|----------|-----------|
| Rust + axum backend | Single static binary, low footprint, strong typing end-to-end |
| Unix socket by default | Sits cleanly behind an nginx upstream; TCP available via `--tcp` |
| sqlx + SQLite | Compile-checked queries, simple deployment, no external database |
| No `.env` file | Sensible defaults, zero-config startup |
| Feature modules | Isolate each feature's code for maintainability |
| shadcn/ui | Copy-paste components, full control over styling |
| Single-binary serving | `server-bin` serves the API and the built web bundle together |
