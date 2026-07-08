-- prograph schema v9 — drift findings (declared intent vs detected reality).
-- Temporal like every other table. NOT a new edge kind — drift is auxiliary
-- analytical data, not a structural relationship between entities.

CREATE TABLE IF NOT EXISTS drift_findings (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL REFERENCES projects(id),
    -- 'missing' | 'extra' | 'stale_todo'.
    kind            TEXT NOT NULL CHECK(kind IN ('missing','extra','stale_todo')),
    -- 'public_symbol' | 'mcp_tool' | 'contract' | 'todo'.
    entity_kind     TEXT NOT NULL CHECK(entity_kind IN ('public_symbol','mcp_tool','contract','todo')),
    entity_name     TEXT NOT NULL,
    source_path     TEXT NOT NULL,
    source_line     INTEGER NOT NULL DEFAULT 0,
    confidence      TEXT NOT NULL CHECK(confidence IN ('high','low')),
    detail          TEXT,
    first_seen      INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen       INTEGER NOT NULL REFERENCES snapshots(id),
    UNIQUE(project_id, kind, entity_kind, entity_name, source_path, source_line)
);

CREATE INDEX IF NOT EXISTS idx_drift_last_seen   ON drift_findings(last_seen);
CREATE INDEX IF NOT EXISTS idx_drift_project     ON drift_findings(project_id);
CREATE INDEX IF NOT EXISTS idx_drift_kind        ON drift_findings(kind);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (9, datetime('now'));
