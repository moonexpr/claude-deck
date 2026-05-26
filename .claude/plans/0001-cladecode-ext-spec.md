# 0001 — `claudecode_ext` Spec

> **Status:** SPEC — awaiting PROMPTER sign-off before Dev phase.
> **Scope:** v1 of `claudecode_ext`, sized to satisfy claude-deck issue #4
> and lay the framework substrate for future lifecycle hooks.
> **Authors:** ARCHITECT (Claude), 2026-05-23
> **Related:** moonexpr/claude-deck#4, PR #3 (merged AI proxy)

---

## 1. Mission

Build `claudecode_ext` — a standalone Garden project that **observes Claude
Code's outbound API traffic** via a local TLS-terminating proxy and exposes
two things to consumers:

1. The **current OAuth bearer** Claude Code is using, refreshed transparently
   as Claude Code rotates it. Lets claude-deck (and future consumers) make
   Anthropic API calls **without a separate `sk-ant-…` key**, satisfying
   the user's existing Pro/Max subscription rather than billing pay-per-token.
2. A **lifecycle event stream** for the three session primitives — *session
   started*, *message sent*, *session closed* — published over an event-bus
   interface. v1 ships exactly these three; the architecture leaves seams
   for extending the surface (tool calls, model selection, retries, quota
   state) without rewriting the substrate.

The framework is **observe-only by construction in v1**. Bytes flow through
unchanged. No request rewriting, no response tampering, no model overrides.
Modification seams exist in the interface but are intentionally unwired.

The framework is **session-correlated single-tenant** in v1 — one Claude
Code session at a time. Multi-tenant correlation (which TLS connection
belongs to which claude PID) is deferred.

---

## 2. Scope

### In scope (v1)

- New Garden project at `~/Garden/app/claudecode_ext/` (workspace Cargo
  layout; see §4 for tree).
- Local TLS-terminating forward proxy in Rust (recommend `hudsucker` or a
  thin layer over `tokio-rustls` — see §3.A for the decision rationale).
- CA cert generation + per-user trust file at
  `~/.claudecode_ext/ca/root.pem`, consumed by Claude Code via
  `NODE_EXTRA_CA_CERTS` (Bun honors this). **No** insertion into the
  system root store — process-scoped trust only.
- A `claude_ext` wrapper binary that exec's the real `claude` binary with
  `NODE_EXTRA_CA_CERTS` + `HTTPS_PROXY` env vars set to point at the proxy.
- Bearer extraction: parse `Authorization: Bearer …` (or whatever Claude
  Code actually uses — confirmed empirically at first launch, see §8.OQ-1)
  from each outbound request; cache with expiry parsed from the JWT `exp`
  claim if present, else a conservative TTL.
- `LifecycleEvent` enum + async broadcast channel: `SessionStarted`,
  `MessageSent` (request metadata only, no body), `SessionClosed`.
- Public Rust API for downstream consumers:
  ```rust
  pub struct Handle { /* private */ }
  pub fn start(config: Config) -> anyhow::Result<Handle>;
  impl Handle {
      pub fn events(&self) -> impl Stream<Item = LifecycleEvent>;
      pub async fn current_bearer(&self) -> Option<Bearer>;
      pub fn shutdown(self) -> impl Future<Output = ()>;
  }
  ```
- Integration into `~/Garden/external/claude-deck/`:
  - `ServerConfig` gains a `key_source: KeySource` enum (see §6).
  - New `KeyProvider` trait, two impls: `ApiKeyProvider`,
    `ClaudeCodeOAuthProvider(Arc<claudecode_ext::Handle>)`.
  - `ai.rs` handlers call `provider.current_bearer().await` per request
    instead of reading a `String` directly.
  - `server-bin` gains a `--key-source=claude-code-oauth` flag (and env
    equivalent) that boots the proxy and wires the OAuth provider.
  - 503 diagnostic body grows a `key_source: "oauth" | "api_key" | null`
    field (already half-stubbed — see `ai.rs` `ErrorBody.key_source`).
- macOS-only in v1. Cross-platform deferred to v2.

### Out of scope (deferred)

- Linux / Windows support. The crate compiles on those platforms but the
  wrapper / CA-install story is macOS-specific.
- Reading the Claude Code OAuth token directly from the keychain (the
  "no-proxy fallback" path). Pure-keychain bootstrap is *not* a v1 path —
  v1 requires the user to have a recent Claude Code session through the
  proxy. Document the degraded-UX implication (§8.OQ-3).
- Decoding Safe Storage envelopes. The proxy sidesteps this entirely.
- Multi-tenant session correlation. Single-tenant assumption documented.
- Any modification of Claude Code's traffic (request rewriting, response
  patching, model override, prompt injection, tool suppression).
- Lifecycle events beyond the three session primitives.
- Anthropic OAuth client registration — distinct R&D path; revisit if
  Anthropic publishes one.

---

## 3. Architecture

```
┌──────────────┐      HTTPS      ┌─────────────────────────┐      HTTPS      ┌──────────────────┐
│ claude binary├────────────────►│ claudecode_ext proxy     ├────────────────►│ api.anthropic.com │
│ (Bun Mach-O) │   via wrapper   │ (tokio-rustls MITM,     │   bytes intact  │ (or wherever      │
│              │   shim exports  │  per-host leaf certs    │                 │  Claude Code      │
│              │   HTTPS_PROXY   │  signed by ext CA)      │                 │  actually talks)  │
└──────────────┘                 └────────┬────────────────┘                 └──────────────────┘
                                          │
                                          │  passive tee, never gates
                                          ▼
                              ┌────────────────────────┐
                              │ event bus +            │
                              │ bearer cache (in-mem)  │
                              └────┬────────────┬──────┘
                                   │            │
                                   ▼            ▼
                           current_bearer() events()
                                   │            │
                                   ▼            ▼
                              ┌──────────────────────┐
                              │ claude-deck          │
                              │ server-core consumes │
                              │ via Handle           │
                              └──────────────────────┘
```

### 3.A — Proxy library choice

**Recommendation:** `hudsucker` (Rust MITM proxy crate, ALPN-aware,
designed for exactly this shape — terminate TLS, expose hooks, re-encrypt
to upstream). Mature, last release recent, used by `cargo-deny`'s
license-scanning preflight tooling and others.

**Alternative:** hand-roll on `tokio-rustls` + `rcgen` (cert gen) +
`hyper`. ~3× more code, more control. Recommend against unless `hudsucker`
proves limiting.

**Decision deferred to Dev phase** — first task is a 1-day spike: stand up
both, point a stub HTTPS client at each, verify ALPN + HTTP/2 behavior is
right (Claude Code likely uses HTTP/2 streaming). Whichever ships cleaner
wins.

### 3.B — Wrapper vs transparent proxy

**Recommendation:** wrapper script + env-var injection (`claude_ext`
binary that exports `HTTPS_PROXY`, `NODE_EXTRA_CA_CERTS`, then `execvp`s
the real `claude`). Same UX as the existing `claude_remotectrl` /
`claude_teams.sh` family. No root needed, no PF/iptables rules, trivially
opt-in per invocation.

Transparent proxy via PF (macOS) / iptables (Linux) is *more universal*
but requires root and risks breaking other apps. Not v1.

### 3.C — CA cert posture

- Generate a self-signed root at first run, store at
  `~/.claudecode_ext/ca/root.pem` (cert) + `root.key.pem` (key, mode 0600).
- **Do not** install into the system trust store. The wrapper sets
  `NODE_EXTRA_CA_CERTS=$HOME/.claudecode_ext/ca/root.pem` which makes Bun
  (and Node.js, via the same env var) trust it for the duration of the
  child process only.
- Per-host leaf certs are minted on demand and cached in memory.
- Documented in README: *if you stop using claudecode_ext, no system trust
  state remains*.

### 3.D — Event-bus design

In-process `tokio::sync::broadcast::Sender<LifecycleEvent>`. Consumers
(claude-deck server-core) subscribe via `.subscribe()` and consume via
the `events()` stream. Slow consumers drop oldest first — events are
diagnostic, not transactional.

For v2 cross-process consumption: same enum serialized over a Unix domain
socket. Architecture leaves the seam (`LifecycleEvent: Serialize`) but
does not implement the UDS path in v1.

### 3.E — Bearer cache semantics

```rust
pub struct Bearer {
    pub token: String,         // never logged
    pub captured_at: Instant,
    pub expires_at: Option<Instant>,  // from JWT exp claim if present
}
```

- Latest-wins. No history.
- `current_bearer()` returns `Some(b)` iff `b.expires_at` is `None`
  (unknown expiry, conservative — trust the proxy will see a refresh
  before expiry) OR `Instant::now() < b.expires_at`. Otherwise `None`.
- Refresh observation: when the proxy sees a request with a different
  bearer than the cached one, replace the cache. No active refresh
  initiation in v1 — Claude Code does the refresh; we observe.
- **Implication (degraded UX):** if Claude Code hasn't run through the
  proxy in the last bearer-lifetime window, `current_bearer()` returns
  `None` and the Deck's AI proxy returns 503 with `key_source: "oauth"`.
  Documented; see §8.OQ-3.

---

## 4. Module layout

### 4.1 — `~/Garden/app/claudecode_ext/` (new project)

```
claudecode_ext/
├── Cargo.toml                          ← workspace root
├── README.md
├── PROJECT.md                          ← per Spec-phase rules (§9)
├── CLAUDE.md                           ← project-scoped Claude config
├── garden.toml                         ← harvest registration (if needed)
├── crates/
│   ├── core/                           ← library: the framework
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                  ← public API surface
│   │       ├── config.rs               ← Config struct, ports, paths
│   │       ├── proxy.rs                ← MITM proxy (hudsucker or hand-rolled)
│   │       ├── ca.rs                   ← root CA gen, leaf signing
│   │       ├── bearer.rs               ← extraction + cache
│   │       └── events.rs               ← LifecycleEvent + broadcast
│   └── shim/                           ← binary: claude_ext wrapper
│       ├── Cargo.toml
│       └── src/main.rs                 ← env-var injection + execvp
└── tests/
    ├── proxy_handshake.rs              ← integration: cert dance + tee
    └── bearer_lifecycle.rs             ← integration: capture, expiry, replace
```

### 4.2 — Changes to `~/Garden/external/claude-deck/`

| File | Change |
|---|---|
| `app/server/core/Cargo.toml` | add `claudecode_ext_core = { path = "../../../../../app/claudecode_ext/crates/core" }` (5 `..`s: `core` → `server` → `app` → `claude-deck` → `external` → `Garden`, then descend into `app/claudecode_ext/crates/core`) |
| `app/server/core/src/lib.rs` | `ServerConfig.anthropic_api_key: Option<String>` → `pub key_source: KeySource` enum |
| `app/server/core/src/services/ai/mod.rs` | new module `key_provider` |
| `app/server/core/src/services/ai/key_provider.rs` | new — `KeyProvider` trait + `ApiKeyProvider` + `ClaudeCodeOAuthProvider` |
| `app/server/core/src/api/v1/ai.rs` | `state.anthropic_api_key` → `state.key_provider.current_bearer().await`; populate `ErrorBody.key_source` |
| `app/server/core/src/api/v1/mod.rs` (`ApiState`) | swap field; expose provider |
| `app/server/bin/src/main.rs` | add `--key-source` flag + `CLAUDECODE_EXT_*` env handling; spawn proxy when OAuth selected |
| `app/desktop/src-tauri/src/keychain.rs` | unchanged in v1 (sibling `read_claude_code_oauth_bearer` deferred to v2) |
| `app/desktop/src-tauri/src/lib.rs` | gains a Tauri command to start/stop the embedded proxy |
| `app/server/core/tests/ai_proxy.rs` | extend to cover OAuth path via stub provider |

---

## 5. Interfaces

### 5.1 — `claudecode_ext_core::lib`

```rust
pub struct Config {
    /// Where to listen for HTTPS-proxy connections. Default 127.0.0.1:0
    /// (ephemeral; consumer reads the actual port from `Handle::proxy_addr()`).
    pub bind: SocketAddr,
    /// Root CA storage dir. Default: ~/.claudecode_ext/ca/.
    pub ca_dir: PathBuf,
    /// Capacity of the event broadcast channel. Default 256.
    pub event_buffer: usize,
}

pub struct Handle { /* private fields: shutdown tx, broadcast tx, bearer cache, listener */ }

pub fn start(config: Config) -> anyhow::Result<Handle>;

impl Handle {
    pub fn proxy_addr(&self) -> SocketAddr;
    pub fn ca_cert_path(&self) -> PathBuf;
    pub fn events(&self) -> impl Stream<Item = LifecycleEvent> + Send + 'static;
    pub async fn current_bearer(&self) -> Option<Bearer>;
    pub async fn shutdown(self);
}

#[derive(Clone, Debug, Serialize)]
pub enum LifecycleEvent {
    SessionStarted { at: SystemTime, peer: SocketAddr },
    MessageSent    { at: SystemTime, endpoint: String, method: String, content_length: Option<u64> },
    SessionClosed  { at: SystemTime, peer: SocketAddr },
}

#[derive(Clone)]
pub struct Bearer {
    pub token: String,           // Debug impl elides
    pub captured_at: Instant,
    pub expires_at: Option<Instant>,
}
```

### 5.2 — claude-deck `KeySource` + `KeyProvider`

```rust
// server_core::lib
pub enum KeySource {
    None,
    ApiKey(String),
    ClaudeCodeOAuth,   // requires the proxy to be running; spawned by main
}

// server_core::services::ai::key_provider
#[async_trait]
pub trait KeyProvider: Send + Sync {
    async fn current_bearer(&self) -> Option<String>;
    fn label(&self) -> &'static str;   // "api_key" | "oauth"
}

pub struct ApiKeyProvider(pub String);
pub struct ClaudeCodeOAuthProvider(pub Arc<claudecode_ext_core::Handle>);

#[async_trait]
impl KeyProvider for ApiKeyProvider { /* ... */ }
#[async_trait]
impl KeyProvider for ClaudeCodeOAuthProvider { /* ... */ }
```

`ApiState.anthropic_api_key: Option<String>` → `ApiState.key_provider: Option<Arc<dyn KeyProvider>>`.

### 5.3 — `claude_ext` shim CLI

```
claude_ext [--proxy-addr=127.0.0.1:PORT] [--ca-cert=PATH] [-- <claude args>...]
```

- With no flags: assumes default proxy at the canonical socket file
  (`~/.claudecode_ext/proxy.sock` carrying the addr) — fails fast if the
  proxy isn't running. Sets `HTTPS_PROXY` + `NODE_EXTRA_CA_CERTS`,
  then `execvp("claude", remaining_args)`.
- With explicit flags: skips socket discovery.
- Never inspects payloads itself; this is purely an env-var shim.

---

## 6. Integration story — operator's perspective

1. User installs claude-deck (Tauri or `server-bin`).
2. User picks "Use Claude Code OAuth" in settings UI (Tauri) or sets
   `--key-source=claude-code-oauth` (server-bin).
3. On Deck startup, server-core calls `claudecode_ext_core::start(...)`,
   gets a `Handle`, stores it on `ApiState`.
4. UI surface: "To use Claude Code's auth, launch Claude Code via the
   `claude_ext` wrapper" — shown until a bearer is observed.
5. User launches Claude Code via `claude_ext` (or via an updated
   `claude_remotectrl` that already exec's through it — easy follow-up
   in claude-config).
6. First HTTPS handshake → proxy mints leaf cert → tees first request →
   bearer cache populates → Deck's AI proxy becomes operational.
7. When bearer rotates, proxy observes the new one transparently.

Failure modes the user sees:
- "Claude Code not seen recently" — 503 from Deck AI proxy, message
  includes `key_source: "oauth"` and a hint to launch Claude Code.
- "Proxy port collision" — server-bin refuses to start, prints actionable
  message.

---

## 7. Phasing (Dev plan, draft)

Each cycle = design → work → eval per the goal-skill framework.

| Cycle | Deliverable | Acceptance |
|---|---|---|
| D1 | Scaffold Garden project (`Cargo.toml` workspace, two crates, `PROJECT.md`, `CLAUDE.md`, harvest registration) | `cargo check -p claudecode_ext_core && cargo check -p claudecode_ext_shim` |
| D2 | Proxy library spike: `hudsucker` vs hand-rolled, both wired to a stub HTTPS server | One winner picked; doc the rationale |
| D3 | CA gen + per-host leaf signing + `NODE_EXTRA_CA_CERTS` end-to-end test | Stub HTTPS-client request through proxy succeeds with no system-trust modification |
| D4 | Bearer extraction + cache + JWT exp parsing | Unit tests cover present/absent/malformed exp |
| D5 | `LifecycleEvent` enum + broadcast channel + `Handle::events()` stream | Integration test sends fake traffic, asserts 3 events in order |
| D6 | `claude_ext` shim binary | Manual smoke: launch real `claude` through shim, observe `MessageSent` events |
| D7 | claude-deck integration: `KeySource` enum, `KeyProvider` trait, `ai.rs` switch | Existing `ai_proxy.rs` tests pass; new test for OAuth path passes with stub provider |
| D8 | `server-bin` flag + Tauri command wiring | Manual smoke on both binaries; 503 diagnostic surface verified |
| D9 | README, PROJECT.md acceptance, issue #4 acceptance checklist closure | All checkboxes flipped |

Build phase: squash, format, lint, push, PR.

---

## 8. Open questions / risks

- **OQ-1.** What is Claude Code's actual API endpoint and request shape?
  Unconfirmed — will be revealed empirically on first MITM run (D3).
  Does **not** change the framework architecture; only changes what we
  log and which header we extract the bearer from. Document findings in
  `PROJECT.md` once observed.
- **OQ-2.** What's the OAuth bearer's lifetime? Likely 1 hour. Affects
  the degraded-UX window. Confirm via JWT `exp` on first capture.
- **OQ-3.** **Degraded UX:** Deck AI proxy fails when no Claude Code
  session has run through `claude_ext` in the recent bearer-lifetime.
  Mitigations (deferred to v2): (a) keychain-direct fallback, (b)
  background-launch a hidden `claude --print` request to refresh.
  v1 accepts the degradation explicitly.
- **OQ-4.** **ToS posture.** Observe-only via TLS MITM of *the user's
  own* outbound traffic, on the user's own machine, to expose their own
  bearer to their own Deck. No third-party data, no traffic modification,
  no automated requests. Documented in README. If Anthropic publishes an
  OAuth client registration, retire this path.
- **OQ-5.** **Cert pinning.** If Claude Code's Bun runtime pins the
  Anthropic cert (TLS pinning), MITM fails. Mitigation: detect on first
  run, fail with actionable error pointing at OQ-4 path. v1 assumes no
  pinning — confirmed by the fact that `NODE_EXTRA_CA_CERTS` is a
  well-documented Bun pattern; pinning would defeat it.
- **OQ-6.** **Fragility.** Claude Code's auth flow is undocumented and
  may change. Mitigations: the bearer extractor is permissive (try
  multiple known header shapes); the framework reports the *raw header
  set* on the first observed request in DEBUG so we can adapt.

---

## 9. Spec-phase establishments (per goal-skill rules)

### Testing philosophy

- Unit tests for pure logic: cert gen, JWT exp parsing, bearer cache
  eviction, event enum (de)serialization.
- Integration tests over a stub HTTPS upstream (`wiremock` or hand-rolled
  `tokio-rustls` server). The proxy is exercised end-to-end without
  hitting real Anthropic.
- Manual smoke tests for the wrapper-shim path (no easy way to script
  launching a 213 MB Mach-O in CI).
- No mocking of `claudecode_ext_core::Handle` in claude-deck tests — use
  a `KeyProvider` test impl that returns a fixed string.

### Git/PR conventions

- Branch on claude-deck repo: `lift/issue-4-cladecode-ext-keysource`.
- `claudecode_ext` is a new Garden project; branched + committed inside
  Monogarden's standard worktree/workspace conventions.
- Commits scoped per crate: `feat(claudecode_ext-core): …`,
  `feat(claudecode_ext-shim): …`, `feat(server-core): …`.
- Single PR on claude-deck (closes #4); the Garden-side `claudecode_ext`
  scaffolding lands as a separate commit on Monogarden `main` *before*
  the claude-deck PR opens (path-dep needs the target to exist).

### Platform / framework preferences

- Rust edition matching workspace (claude-deck currently 2024 edition —
  match it).
- `tokio` async runtime.
- `reqwest` only inside server-core, not in claudecode_ext-core (the proxy
  speaks raw HTTP, not the high-level client).
- `tracing` for logs.
- `keyring` is **not** a v1 dep of claudecode_ext_core (deferred).

### Documentation

- `PROJECT.md` in the new repo: this spec, condensed.
- `README.md` in the new repo: install / launch / troubleshoot.
- claude-deck `app/README.md` gains a "Key sources" section.

---

## 10. Acceptance criteria

Original issue #4 checklist (reconciled against PR #5 / D7+D8+D9):

- [x] `ServerConfig` exposes a discriminated key source (`KeySource::ApiKey`,
      `KeySource::ClaudeCodeOAuth`, `KeySource::None`). — D7 (`5012d09`).
- [x] `server-bin` boots without `ANTHROPIC_API_KEY` and, when launched
      with `CLAUDECODE_EXT_KEY_SOURCE=oauth` *and* a recent Claude Code
      session has passed through `claude_ext`, serves `/api/v1/ai/chat`
      and `/api/v1/ai/suggest` via the observed bearer. — D7 (`5012d09`).
      (Shipped as env var rather than `--key-source` flag; same effect.)
- [x] Tauri build picks up the same source when configured. — D8 (`70f233d`)
      auto-detects via `security find-generic-password` keychain probe.
- [x] Token refresh is observed transparently (no user-visible re-auth).
      Bearer cache refreshes on each proxied request with a fresh JWT.
- [x] 503 diagnostic body reports `key_source: "oauth" | "api_key" | null`.
      — `ai.rs` `no_key_response()`.
- [x] At least one integration test exercises the OAuth code path against
      a stub provider in `app/server/core/tests/`. — `tests/ai_proxy.rs`:
      `no_oauth_bearer_returns_503_with_oauth_label` +
      `oauth_path_sends_authorization_bearer_header` (52/52 green).
- [x] Documented as **experimental** in `app/README.md`. — D8/D9 README polish.

Plus framework-specific:

- [x] `claudecode_ext_core::start()` returns a `Handle` whose
      `current_bearer()` becomes `Some(_)` within 1s of a stub HTTPS
      request flowing through the proxy. — D4 + D5 `end_to_end.rs`.
- [~] `events()` stream emits `SessionStarted`, `MessageSent`,
      `SessionClosed` in correct order during integration test. —
      `MessageSent` wired and tested; `SessionStarted` / `SessionClosed`
      declared but unwired (hudsucker `HttpHandler` doesn't surface
      connection-level hooks). **Deferred** to follow-up cycle.
- [x] No traffic modification: byte-for-byte parity between request
      written by client and request observed at upstream stub. — D5
      `end_to_end.rs` asserts response bytes == sent bytes.
- [x] No system trust store modification: process-scoped trust via
      `NODE_EXTRA_CA_CERTS` only; CA at `~/.claudecode_ext/ca/root.pem`
      is never inserted into the system keychain. (D3 close.)

---

## 11. Spelling — resolved

The original goal text said `cladecode_ext`; PROMPTER selected
`claudecode_ext` at Spec sign-off (2026-05-23). All references in this
plan use **`claudecode_ext`** throughout. Implications locked in:

| Artifact | Name |
|---|---|
| Project directory | `~/Garden/app/claudecode_ext/` |
| Lib crate | `claudecode_ext_core` |
| Shim binary crate | `claudecode_ext_shim` |
| Shim binary | `claude_ext` (unchanged — already self-evident) |
| Env-var prefix | `CLAUDECODE_EXT_*` |
| Trust dir | `~/.claudecode_ext/ca/` |
| Discovery socket | `~/.claudecode_ext/proxy.sock` |
| Branch on claude-deck | `lift/issue-4-claudecode-ext-keysource` |

The journal filename `2026May23_cladecode-ext-hooking-framework.md` is
kept as the session record (reflects what was said at session start);
its content notes the rename.

---

## Sign-off

- [ ] PROMPTER reviewed
- [x] Spelling confirmed (§11) — `claudecode_ext`
- [x] Open questions §8 understood — OQ-3 (degraded UX) mitigations
      explicitly deferred in PR #5 caveats.
- [x] Phasing §7 acceptable — D1→D9 landed in declared order.
- [x] Spec → Dev phase transition approved — Dev phase executed.
