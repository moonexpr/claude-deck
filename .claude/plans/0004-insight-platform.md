# Plan 0004 — Insight Platform (meta-cognition layer)

> **Status:** Design/Spec only. No implementation. North-star spec; M0–M1 are the buildable near term.
> **Relation:** Builds on #7/#9 (session grouping), Plan 0003 (live tail — supplies incremental ingestion), and the existing AI proxy (`app/server/core/src/services/ai/*`). Does **not** depend on the Tauri desktop shell.
> **Authors:** Sprint session 2026-06-04.

---

## 1. Goal

Turn claude-deck from a **config manager** (CRUD over static files) into a **meta-cognition layer over the agentic framework**: the website itself reads the byproducts of work and produces

- **Insights / analysis** — what happened across sessions, recurring errors, time sinks, patterns;
- **A registry of all pending judgment calls** — every flagged decision, in one reviewable place with state;
- **Skill / strategy proposals** — repeated patterns surfaced as candidate skills, wired to the framework's promotion lifecycle.

Each artifact is **evidence-linked** (clickable back to its source) and has a **lifecycle** (open → accepted/dismissed/applied).

## 2. The reframe (why this is tractable)

The framework already *manufactures* this content as exhaust. A single disciplined session emits — into markdown — flagged judgment calls (`## Judgment calls` in journals), validators, saved memories, "key lessons," issue triage, and reusable patterns. The intelligence already exists; it is **trapped and unaggregated** in `admin/wiki/journals/`, `INBOX.md`, `MEMORY.md`, and session JSONL.

So this is **ETL + structuring + a thin synthesis layer + write-back**, not "AI invents insight from nothing." That framing is what keeps it grounded and trustworthy.

The two genuinely hard pieces — **ingestion** and **inference** — already ship: session JSONL ingestion (#7/#9) and the AI proxy (OAuth-bearer or API-key inference). The missing middle is a **data model** and an **analysis job runner**.

## 3. The pipeline spine

```
Ingest ──▶ Derive ──▶ Structure ──▶ Surface ──▶ Act
 (have)    (have*)     (missing)     (partial)   (missing)
```

| Stage | Definition | Current asset |
|---|---|---|
| Ingest | read sessions, journals, memory, git, deferrals | Sessions JSONL grouping (#7/#9); Plan 0003 `notify` tail for deltas |
| Derive | LLM analysis over the corpus | AI proxy (`ai.rs`/`anthropic.rs`/`key_provider`) |
| Structure | first-class artifacts with provenance + lifecycle | **gap** |
| Surface | feeds, judgment-call registry, proposal board | Usage / Memory / Sessions viewer (#8) are precedents |
| Act | dispatch a sprint, scaffold a skill, file an issue | `claude_remotectrl` + GitHub issues API to wire to |

## 4. Data model

Derived store (see §8 Open decision A: **derived, rebuildable** — files stay canonical). All artifacts FK to `sources` for clickable provenance.

```
analysis_runs(id, kind, model, input_hash, status, token_cost, started_at, finished_at)
  -- job ledger; input_hash enables incremental skip + cache.

sources(id, type, ref, byte_offset, git_sha, indexed_at)
  -- type ∈ {session, journal, inbox, memory, git_commit}
  -- ref = session_id | file path; (byte_offset / git_sha) pin the exact evidence.

insights(id, run_id, title, body, severity, source_id, embedding, status, created_at)

judgment_calls(id, run_id, summary, context, options_json, chosen, rationale,
               source_id, dedup_group, status, created_at, resolved_at)
  -- status ∈ {open, accepted, dismissed, superseded}

proposals(id, run_id, kind, title, rationale, evidence_source_ids_json,
          scaffold_target, status, created_at)
  -- kind ∈ {skill, strategy}; status ∈ {proposed, accepted, promoted, rejected}
```

Migrations via `sqlx-migrate` (already adopted in the lift). `embedding` backed by **sqlite-vec** for similarity dedup; FTS5 for keyword (the context-mode pattern).

## 5. Architecture

- **Ingestion.** Incremental, offset-tracked. Reuse Plan 0003's `notify` watcher on `~/.claude/projects/*.jsonl`; add parsers for journal frontmatter + `## Judgment calls`, the `INBOX.md` entry shape, `MEMORY.md`, and `git log`. Only deltas are re-analyzed (keyed by `input_hash`).
- **Job runner.** A `tokio` task pool + the `analysis_runs` ledger table (claim/lease/complete). Triggered on ingest, on demand, or scheduled. Map-reduce: per-source summarizers → cross-source synthesizer.
- **Inference.** Through the existing AI proxy. **Structured output via JSON-schema / tool-calling** (mirror the Workflow `StructuredOutput` pattern) so every artifact is parseable, not regex-scraped. Cheap-model triage → expensive synthesis.
- **Provenance.** Every artifact row carries a `source_id`; the UI links straight into the transcript viewer (#8) at the cited entry. No citation ⇒ artifact is rejected at write time.
- **Write-back.** Status transitions live in the DB; "accept memory" / "promote skill" actions optionally emit back to the canonical files (memory file, skill scaffold) so the loop closes without the DB becoming the silent source of truth.

## 6. The hard problems (the real engineering)

1. **Grounding / anti-hallucination — #1 risk.** Artifacts must be evidence-linked and **adversarially verified** (independent verifier stage, "default to refuted"). An uncited insight is noise, or the *manufactured-FAIL* failure mode the global config warns about. Mitigation: schema-constrained generation + mandatory `source_id` + a verify pass before an artifact becomes visible.
2. **Extraction from semi-structured exhaust.** Lean on existing conventions; tighten **one**: a machine-readable judgment-call marker in journals so extraction is deterministic, not LLM-guessed.
3. **State / identity / dedup.** Stable identity + status transitions + similarity dedup (sqlite-vec embeddings + FTS5). Recurring judgment calls collapse into a `dedup_group`.
4. **Cost / latency at scale.** Incremental deltas only, caching by `input_hash`, triage-then-synthesize.
5. **Agency boundary (M5).** "Act" re-opens the autonomy-grounding concern: proposals stay **human-gated** until explicitly promoted, never auto-applied. No detached, open-mandate action.

## 7. Milestones (vertical slices, not horizontal layers)

- **M0 — Artifact data model + provenance.** Migrations for the §4 tables; provenance link into the transcript viewer. *Foundation; no UI yet.*
- **M1 — Per-session insight card** *(highest-leverage first slice).* Open a session → background job → decisions / judgment calls / errors / artifacts / follow-ups, each linking back into the transcript. Exercises the **whole pipeline on one unit**.
- **M2 — Judgment-call registry.** Aggregate every flagged decision into one board with accept/dismiss write-back. Directly the "collection of all pending judgment calls."
- **M3 — Cross-session synthesis (insights feed).** Map-reduce over sessions → recurring errors, repeated fixes, time sinks.
- **M4 — Proposal engine.** Mine repeated patterns → propose skills/strategies; "promote" scaffolds the skill folder (framework's staging → promotion lifecycle).
- **M5 — Closed loop / agency.** Dispatch a sprint, scaffold a skill, or file an issue from an artifact — human-gated.

## 8. Decisions (resolved 2026-06-04)

- **A. Source of truth — RESOLVED: derived/rebuildable.** Files stay canonical; the DB is a re-buildable index + lifecycle store (`DELETE` + re-run reproduces every row). No two-way file sync in M0/M1.
- **B. Corpus scope — RESOLVED: multi-source, sessions-first.** Schema admits all sources (sessions, journals, INBOX, memory, git) with no migration; ingest **sessions** in M1, layer journals/memory/git in M2–M3. (claude-deck already reads the *global* `~/.claude/projects/`, so the corpus is cross-project from day one.)
- **C. Structured output — RESOLVED: forced tool-use.** Inference uses Anthropic `tools` + `tool_choice:{type:tool}` for schema-constrained artifacts (see 0004a), not JSON scraping.

## 9. Effort estimate & phasing

- **M0 + M1** (data model + one insight card): the proving phase — small, end-to-end, reuses ingestion + AI proxy + #8 viewer. Ship first.
- **M2 + M3**: the aggregation surfaces — where the platform starts feeling like a platform.
- **M4 + M5**: the agency layer — gated behind grounding (§6.1) being solid.

## 10. Risks

- Ungrounded artifacts erode trust faster than they add value → §6.1 is a gate, not a nice-to-have.
- Inference cost balloons without incremental + caching → §6.4.
- Scope sprawl: M3–M5 are tempting; M0–M1 must prove the pipeline first.

## Sign-off

- [ ] Open decisions A & B confirmed or overridden.
- [ ] M0 schema reviewed.
- [ ] M1 chosen as the first build.
