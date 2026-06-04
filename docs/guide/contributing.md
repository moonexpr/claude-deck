# Contributing

Thanks for your interest in contributing to Claude Deck! We welcome pull requests and issues from everyone.

## Getting Started

1. Fork the repo and clone it:

```bash
git clone https://github.com/<your-username>/claude-deck.git
cd claude-deck
```

2. Run the install script (requires Rust 1.85+ and Node.js 18+):

```bash
./scripts/install.sh
```

3. Start the dev servers:

```bash
./scripts/dev.sh
```

Backend runs at `http://localhost:8000`, frontend at `http://localhost:5173`.

## Code Style

### Frontend

- TypeScript strict mode (`noUnusedLocals`, `noUnusedParameters`)
- ESLint — run `cd app/web && npm run lint` before submitting
- Tailwind CSS + shadcn/ui for styling
- Path alias `@/*` maps to `./src/*`

### Backend

- Rust 2024 edition — run `cargo fmt` and `cargo clippy` before submitting
- Async/await on Tokio; axum handlers return typed responses
- serde for request/response types

## Project Structure

Each feature lives in its own module under `app/web/src/features/`. When adding a new feature:

1. Create a directory in `app/web/src/features/<name>/`
2. Add a page component, sub-components, API functions, and types
3. Register the route in `app/web/src/App.tsx`
4. Add the backend route module in `app/server/core/src/api/v1/`
5. Wire it into the router in `app/server/core/src/api/v1/mod.rs`

## Submitting Changes

1. Create a branch for your change
2. Make your changes and test them locally
3. Run `cd frontend && npm run lint` to check for lint errors
4. Open a pull request with a clear description of what you changed and why

## Reporting Issues

Found a bug or have a feature idea? [Open an issue](https://github.com/moonexpr/claude-deck/issues) and include:

- What you expected to happen
- What actually happened
- Steps to reproduce (if applicable)

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
