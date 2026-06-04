# Plan 0004a — Insight Platform M0 + M1 (implementation spec)

> **Status:** Implementation-ready. Companion to Plan 0004 (§7 M0–M1). Scoped to be a single `/sprint`.
> **Goal:** Ship the artifact data model (M0) and one end-to-end vertical slice — the per-session insight card (M1) — exercising Ingest → Derive → Structure → Surface on a single session.
> **Grounding facts (verified 2026-06-04):**
> - `ApiState` (`api/v1/mod.rs`) already holds `pool: SqlitePool`, `session_service: Arc<SessionService>`, `key_provider: Option<Arc<dyn KeyProvider>>`, `anthropic_base_url: String`. **No state plumbing needed.**
> - Migrations run at boot: `sqlx::migrate!("./migrations")` (`lib.rs:135`); existing `migrations/0001_chat_conversations.sql`.
> - `anthropic::complete_messages(client, base_url, model, messages, …)` → full text + `Usage` (non-streaming). Inference call for the job.
> - `SessionService::get_session_detail(project, session, page) -> SessionDetailResponse`; reshape types `ContentBlock`/`SessionMessage`/`SessionConversation` in `session_service.rs`.
> - Route modules: `pub fn router() -> Router<ApiState>`, nested in `mod.rs`. Frontend features register via `@/features/registry`.

---

## M0 — Artifact data model + provenance

### Migration: `migrations/0002_insight_platform.sql`

```sql
-- Job ledger. input_hash = hash(session content + analyzer version) → cache/skip.
CREATE TABLE analysis_runs (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  kind        TEXT NOT NULL,                 -- 'session_insight' (more in M3+)
  target_ref  TEXT NOT NULL,                 -- '{project}/{session}'
  model       TEXT NOT NULL,
  input_hash  TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'pending',-- pending|running|done|error
  error       TEXT,
  input_tokens  INTEGER,
  output_tokens INTEGER,
  citation_error    INTEGER NOT NULL DEFAULT 0,  -- artifacts dropped: locator resolved to no real entry
  groundedness_error INTEGER NOT NULL DEFAULT 0, -- artifacts dropped: quote not found in cited entry
  started_at  TEXT NOT NULL DEFAULT (datetime('now')),
  finished_at TEXT
);
CREATE INDEX idx_runs_target ON analysis_runs(kind, target_ref);
CREATE UNIQUE INDEX idx_runs_hash ON analysis_runs(kind, input_hash);

-- Evidence anchor. Every artifact FKs here → clickable provenance.
CREATE TABLE sources (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  type        TEXT NOT NULL,                 -- session|journal|inbox|memory|git_commit
  ref         TEXT NOT NULL,                 -- '{project}/{session}' | file path
  locator     TEXT,                          -- entry uuid | byte_offset | git sha
  created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_sources_ref ON sources(type, ref);

CREATE TABLE insights (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      INTEGER NOT NULL REFERENCES analysis_runs(id) ON DELETE CASCADE,
  source_id   INTEGER REFERENCES sources(id),
  quote       TEXT,                          -- verbatim span from the cited entry (grounding evidence)
  title       TEXT NOT NULL,
  body        TEXT NOT NULL,
  severity    TEXT NOT NULL DEFAULT 'info',  -- info|notable|risk
  status      TEXT NOT NULL DEFAULT 'open',  -- open|dismissed
  created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE judgment_calls (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      INTEGER NOT NULL REFERENCES analysis_runs(id) ON DELETE CASCADE,
  source_id   INTEGER REFERENCES sources(id),
  quote       TEXT,                          -- verbatim span from the cited entry (grounding evidence)
  summary     TEXT NOT NULL,
  context     TEXT,
  options_json TEXT,                          -- JSON array of option strings
  chosen      TEXT,
  rationale   TEXT,
  dedup_group TEXT,                           -- NULL in M1; populated in M2 (sqlite-vec, see Forward note)
  status      TEXT NOT NULL DEFAULT 'open',   -- open|accepted|dismissed|superseded
  created_at  TEXT NOT NULL DEFAULT (datetime('now')),
  resolved_at TEXT
);

CREATE TABLE proposals (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      INTEGER NOT NULL REFERENCES analysis_runs(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL,                  -- skill|strategy
  title       TEXT NOT NULL,
  rationale   TEXT NOT NULL,
  evidence_source_ids_json TEXT,
  scaffold_target TEXT,
  status      TEXT NOT NULL DEFAULT 'proposed',-- proposed|accepted|promoted|rejected
  created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
```

> **Provenance is a write-time invariant — a claim-level grounding gate, not a string match** *(HF-refined: JSONSchemaBench 2501.10868; CiteAudit 2602.23452; WebCiteS 2403.01774)*. Forced tool-use guarantees schema-validity but **nothing** about truth — schema-validity and groundedness are orthogonal, so the floor lives **post-decode**. Each artifact carries a `source_ref` (locator) **and** a `quote` (verbatim span it claims support from). The service drops an artifact unless **both** hold:
> 1. **Citation check** — the locator resolves to a real session entry. (Fail ⇒ *citation error*.)
> 2. **Groundedness check** — the `quote` is found (normalized substring) in that entry's text. (Fail ⇒ *groundedness error*.)
>
> Drops are counted on the run by failure class (the two are distinct signals; don't collapse them). M1 uses the cheap deterministic substring check; an NLI/entailment verifier (per 2305.06311) is the M3+ hardening, not an M1 dependency. `proposals` are M4; the table ships in M0 so the schema is stable, but nothing writes it in M1.

> **Decision A (derived store):** these tables live in the same `claude_registry.db`. Nothing here is canonical — a `DELETE FROM` + re-run reproduces every row from the session files. No two-way file sync in M0/M1.

**M0 acceptance:** `cargo test --workspace` green (migration applies at boot in the test harness, which already constructs the pool); a trivial `#[sqlx::test]` inserts a `source` + `insight` and round-trips.

---

## M1 — Per-session insight card

### Backend

**New: `services/insight_service.rs`** — `pub struct InsightService { pool, session_service, anthropic_base_url }` (or take them as args; mirror `SessionService` construction).

```
pub async fn analyze_session(
    &self,
    key_provider: &Arc<dyn KeyProvider>,
    project: &str, session: &str,
) -> anyhow::Result<SessionInsight>
```

Flow:
1. Load **all pages** of `session_service.get_session_detail(project, session, page)` → flatten to the conversation list. Capture each entry's stable `locator` (the JSONL entry `uuid`; fall back to `"{conv_index}.{msg_index}"`).
2. `input_hash = blake3(model + analyzer_version + concat(entry uuids/text))`. If an `analysis_runs` row with `(kind='session_insight', input_hash)` is `done`, **return its cached artifacts** (no inference).
3. Insert `analysis_runs(status='running')`. Build the structured-output request (below). Call inference. On success, transactionally insert `sources` + `insights` + `judgment_calls`, set run `done` + token counts. On error, set run `error` and return it.

**Inference = forced tool-use (reliable structured output).** Extend `anthropic.rs` with:

```
pub async fn complete_structured(
    client, base_url, model, messages,
    tool_name: &str, input_schema: serde_json::Value,
) -> anyhow::Result<(serde_json::Value /*tool input*/, Usage)>
```

— adds `tools: [{name, input_schema}]` + `tool_choice: {type:"tool", name}` to `build_request_body`, and returns the `tool_use` block's `input`. (Falls back to a strict-JSON system prompt + `serde_json::from_str` with one repair retry if tool-use is undesired.)

Tool `record_session_insight`, `input_schema` (JSON Schema). Every grounded item carries **both** `source_ref` (locator) **and** `quote` (verbatim span copied from that entry) — the quote is what makes the post-decode groundedness check possible:
```jsonc
{ "type":"object","required":["summary","judgment_calls","follow_ups"], "properties":{
  "summary": {"type":"string"},
  "decisions": {"type":"array","items":{"type":"object","required":["text","source_ref","quote"],
      "properties":{"text":{"type":"string"},"source_ref":{"type":"string"},"quote":{"type":"string"}}}},
  "judgment_calls": {"type":"array","items":{"type":"object",
      "required":["summary","source_ref","quote"],"properties":{   // one ref PER judgment call (decision granularity)
        "summary":{"type":"string"},"options":{"type":"array","items":{"type":"string"}},
        "chosen":{"type":"string"},"rationale":{"type":"string"},
        "source_ref":{"type":"string"},"quote":{"type":"string"}}}},
  "errors_hit": {"type":"array","items":{"type":"object","required":["text","source_ref","quote"],
      "properties":{"text":{"type":"string"},"source_ref":{"type":"string"},"quote":{"type":"string"}}}},
  "follow_ups": {"type":"array","items":{"type":"string"}}   // no source_ref: forward-looking, not a grounded claim
}}
```
System prompt instructs: *cite `source_ref` as the entry uuid/index, and `quote` an exact span from that entry; emit nothing you cannot quote.* For each item the service runs the **two-check gate** (above): resolve `source_ref` → a `sources` row (`type='session'`, `ref='{project}/{session}'`, `locator=source_ref`), then verify `quote` is a normalized substring of that entry. **Both pass → persist** (with `quote` stored for UI highlight); either fails → **drop**, incrementing the run's `citation_error` / `groundedness_error` counters separately.

**New route: `api/v1/insights.rs`** → `pub fn router() -> Router<ApiState>`, nested as `.nest("/insights", insights::router())` in `mod.rs`:
- `GET  /insights/session/{project}/{session}` → cached `SessionInsight` (404/empty if never analyzed).
- `POST /insights/session/{project}/{session}/analyze` → run (or return cache); reads `state.key_provider` (503 `{key_source:null}` when `None`, mirroring `ai::post_chat`).
- `PATCH /insights/judgment-call/{id}` `{status}` and `PATCH /insights/{id}` `{status}` → lifecycle write-back.

`SessionInsight` response = the persisted rows joined to their `sources` (so the FE gets `locator` for back-linking).

### Frontend

- **`features/sessions/InsightCard.tsx`** — rendered at the top of `SessionViewPage.tsx`. States: *not analyzed* (button → POST analyze), *running* (spinner), *done* (sections: Summary, Judgment calls, Decisions, Errors, Follow-ups). 503 → reuse the chat banner's `key_source` copy (PR #6 pattern).
- Each judgment-call / decision / error row shows its evidence and, on click, **scrolls the transcript to the cited entry** via `locator` (the viewer from #8 keys rows; add a `data-locator`/uuid anchor + `scrollIntoView`).
- Judgment-call rows get accept/dismiss → `PATCH` → optimistic status update.
- **`useSessionsApi.ts`**: add `getSessionInsight`, `analyzeSession`, `setJudgmentCallStatus`. No new feature-registry entry needed (lives inside the existing sessions route).

### Validators (mechanical — per PROJECT.md testing floor)

1. **`tests/insights.rs`** (mirrors `tests/ai_proxy.rs` wiremock pattern): stand up the router with a wiremock `anthropic_base_url` returning a canned `tool_use` block. The canned response mixes three items against a fixture session: (a) a **grounded** judgment_call — real `source_ref` + a `quote` that is verbatim in that entry; (b) a **citation error** — `source_ref` to no real entry; (c) a **groundedness error** — real `source_ref` but a `quote` absent from that entry. Assert: `POST …/analyze` → 200; `analysis_runs` row `done` with token counts **and** `citation_error=1`, `groundedness_error=1`; **only (a)** persisted, with non-NULL `source_id` and its `quote` stored; (b) and (c) **not stored**.
2. Cache test: second `POST …/analyze` with unchanged input does **no** upstream call (wiremock `.expect(1)`).
3. 503 test: `key_provider=None` → `POST …/analyze` returns 503 `{key_source:null}`.
4. `routes.rs` smoke: `/insights/session/x/y` returns a non-5xx status.
5. Frontend: `npm run build` (tsc) + `npm run lint` clean; `e2e/sessions.spec.ts` still mounts without console errors (extend it to assert the InsightCard renders its idle state). **Run BOTH toolchains** (the #10→#11 lesson: a cross-cutting change must pass `cargo` *and* `npm run build`).

### Build order (for `/sprint`)
1. `0002_insight_platform.sql` + round-trip test → green. (M0 done.)
2. `complete_structured` in `anthropic.rs` + unit test against a canned tool_use payload.
3. `insight_service.rs::analyze_session` (load → hash/cache → infer → persist with provenance gate).
4. `api/v1/insights.rs` + nest in `mod.rs` + `tests/insights.rs` (validators 1–4).
5. `InsightCard.tsx` + `useSessionsApi` wiring + transcript back-link + e2e extension (validator 5).
6. Integration build on `main` after merge (both toolchains).

## Forward note (M2 dedup embedding) — *HF-refined, not built in M1*

When M2 adds semantic dedup of recurring judgment calls: embed `judgment_calls.summary` and store an `embedding float[384]` vec0 column in **sqlite-vec**, dedup by cosine distance, populate `dedup_group`. Model: **`BAAI/bge-small-en-v1.5`** (33.4M params, 384-dim, MIT, ships ONNX + native Rust/`candle` weights — keeps inference in-process, no extra service). Drop-in fallback: `sentence-transformers/all-MiniLM-L6-v2` (384-dim, Apache-2.0, `rust`/candle tag). Near-zero-cost option if latency ever dominates: `minishlab/potion-base-8M` (model2vec, 256-dim, MIT). *(hf_used: hub_repo_search/hub_repo_details.)* Recorded now so M0's `sources`/`dedup_group` shape is forward-compatible; **no embedding dependency in M1.**

## Open decisions inherited from 0004
- **A — derived store:** assumed. If you later want authoritative state, `status` columns already model lifecycle; only the file write-back direction changes.
- **B — corpus scope:** M1 ingests **sessions** only. `sources.type` enum already admits journal/inbox/memory/git for M2–M3 without a migration.

## Sign-off
- [ ] `0002` schema reviewed (incl. `quote` columns + run error counters).
- [x] Inference path: **forced tool-use** (Decision C).
- [ ] Claim-level two-check gate (citation + groundedness via `quote` substring) accepted as the M1 anti-hallucination floor; NLI/entailment verifier deferred to M3+.
- [ ] `BAAI/bge-small-en-v1.5` accepted as the M2 dedup embedding (forward note).

---

### Refinement provenance
HF refinement workflow `hf-refine-insight-spec` (2026-06-04): 3/4 agents landed with real Hugging Face evidence (paper_search, hf_doc_search, hub_repo_search/details); the cross-model critique agent hit a session limit and did not complete (its absence does not affect the deltas above). Synthesized by the orchestrator. Findings → deltas: claim-level grounding gate (`quote`+locator, citation vs groundedness split) and the M2 embedding choice.
