-- 0002_insight_platform.sql — Insight Platform M0 artifact data model (Plan 0004a).
--
-- Derived store (Decision A): every row here is a rebuildable projection of the
-- session files. `DELETE FROM` any table + re-run analysis reproduces it. The
-- canonical source of truth stays the JSONL transcripts; nothing here is written
-- back to files in M0/M1.

-- Job ledger. input_hash = hash(session content + analyzer version) → cache/skip.
-- citation_error / groundedness_error count artifacts the provenance gate dropped.
CREATE TABLE analysis_runs (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  kind        TEXT NOT NULL,                  -- 'session_insight' (more kinds in M3+)
  target_ref  TEXT NOT NULL,                  -- '{project}/{session}'
  model       TEXT NOT NULL,
  input_hash  TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'pending',-- pending|running|done|error
  error       TEXT,
  input_tokens       INTEGER,
  output_tokens      INTEGER,
  citation_error     INTEGER NOT NULL DEFAULT 0,  -- dropped: locator resolved to no real entry
  groundedness_error INTEGER NOT NULL DEFAULT 0,  -- dropped: quote not found in cited entry
  started_at  TEXT NOT NULL DEFAULT (datetime('now')),
  finished_at TEXT
);
CREATE INDEX idx_runs_target ON analysis_runs(kind, target_ref);
CREATE UNIQUE INDEX idx_runs_hash ON analysis_runs(kind, input_hash);

-- Evidence anchor. Every artifact FKs here → clickable provenance.
CREATE TABLE sources (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  type        TEXT NOT NULL,                  -- session|journal|inbox|memory|git_commit
  ref         TEXT NOT NULL,                  -- '{project}/{session}' | file path
  locator     TEXT,                           -- entry uuid | byte_offset | git sha
  created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_sources_ref ON sources(type, ref);

CREATE TABLE insights (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      INTEGER NOT NULL REFERENCES analysis_runs(id) ON DELETE CASCADE,
  source_id   INTEGER REFERENCES sources(id),
  quote       TEXT,                           -- verbatim span from the cited entry (grounding evidence)
  title       TEXT NOT NULL,
  body        TEXT NOT NULL,
  severity    TEXT NOT NULL DEFAULT 'info',    -- info|notable|risk
  status      TEXT NOT NULL DEFAULT 'open',    -- open|dismissed
  created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE judgment_calls (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      INTEGER NOT NULL REFERENCES analysis_runs(id) ON DELETE CASCADE,
  source_id   INTEGER REFERENCES sources(id),
  quote       TEXT,                           -- verbatim span from the cited entry (grounding evidence)
  summary     TEXT NOT NULL,
  context     TEXT,
  options_json TEXT,                           -- JSON array of option strings
  chosen      TEXT,
  rationale   TEXT,
  dedup_group TEXT,                            -- NULL in M1; populated in M2 (sqlite-vec)
  status      TEXT NOT NULL DEFAULT 'open',    -- open|accepted|dismissed|superseded
  created_at  TEXT NOT NULL DEFAULT (datetime('now')),
  resolved_at TEXT
);

CREATE TABLE proposals (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      INTEGER NOT NULL REFERENCES analysis_runs(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL,                   -- skill|strategy
  title       TEXT NOT NULL,
  rationale   TEXT NOT NULL,
  evidence_source_ids_json TEXT,
  scaffold_target TEXT,
  status      TEXT NOT NULL DEFAULT 'proposed',-- proposed|accepted|promoted|rejected
  created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
