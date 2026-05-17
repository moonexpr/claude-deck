# Rust Port Contract (read before porting any module)

The Python backend at `backend_python/` is the **behavioral source of truth**.
The React frontend is **unchanged** and was built against the Python API — so
**route paths, query/body field names, and response JSON shapes must match the
Python module exactly**. Do not "improve" paths or rename fields.

## Where things are

- Your Python source: `backend_python/app/api/v1/<mod>.py` (routes) and
  `backend_python/app/services/<svc>_service.py` (logic).
- Reference Rust module (copy this style): `backend/src/api/v1/config.rs`.
- You edit **only** your assigned `backend/src/api/v1/<mod>.rs` file(s).
  Do NOT touch `mod.rs`, `models.rs`, `main.rs`, `Cargo.toml`, or other
  modules' files — the architect owns integration.

## Foundation API (already built, use it)

- `crate::paths` — every Claude path helper, ported 1:1 from
  `utils/path_utils.py` (same function names). E.g.
  `paths::get_claude_user_settings_file()`, `paths::get_project_hooks_dir(pp)`.
- `crate::fileio` — `read_json_file(&Path) -> Option<Value>`,
  `write_json_file(&Path, &Value).await -> bool` (atomic),
  `read_text_file`, `write_text_file`, `file_exists`, `directory_exists`.
- `crate::patterns` — permission pattern validate/migrate/sanitize.
- `crate::error::{AppError, AppResult}` — `AppError` renders as
  `{"detail": "..."}` with a status code, **mirroring FastAPI's
  HTTPException** (the frontend already parses this). Constructors:
  `AppError::internal/bad_request/not_found/forbidden/conflict`.
  `?` auto-converts `anyhow`/`io`/`serde_json` errors → 500.

## Handler conventions

- Signature: `async fn h(...) -> AppResult<Json<Value>>` (or `Json<T>`).
- Router signature: `pub fn router() -> Router<ApiState>`.
- `ApiState` (in `mod.rs`) has `pool: SqlitePool` and
  `session_service: Arc<SessionService>`. Most config modules need neither —
  call `crate::paths` / `crate::fileio` free functions directly. Only
  DB-backed modules (presence) use `State(state): State<ApiState>` + `pool`.
- Translate Python `except Exception as e: raise HTTPException(500, str(e))`
  → return `Err(AppError::internal(e.to_string()))` (or just use `?`).
  `raise HTTPException(400/404, ...)` → `AppError::bad_request/not_found`.
- Query params: `Query(struct)`. Path params: `axum::extract::Path<...>`.
  JSON body: `Json(struct)` with `#[derive(Deserialize)]`,
  `#[serde(default)] Option<...>` for optional fields.
- Match Python JSON keys exactly. Prefer `serde_json::json!` + `Value` for
  response bodies so shapes are byte-for-byte what the frontend expects.
- Preserve Python semantics precisely: dict.update = shallow overwrite;
  deep-merge / null-removes-key (see `config.rs::deep_merge`); `rglob("*.md")`
  = recursive, `glob` = one level (see `config.rs::rglob_md`); sorting and
  ordering where the frontend relies on it.
- Security: keep every path-traversal / size guard the Python has. The commit
  that started this claimed "fix Issue #1 security vulnerabilities" — do not
  regress validation. `session_service.rs` shows the guard style.

## Definition of done for a module

1. Every Python endpoint exists at the **same method + path**.
2. Request and response shapes match Python (test mentally against the
   frontend `src/features/<feature>` API client if unsure).
3. Real filesystem/DB behavior — **no hardcoded empty stubs**.
4. `cargo check` clean for your file (architect runs the full build).
5. Leave a one-line `// PORTED: <python file>` at the top.
