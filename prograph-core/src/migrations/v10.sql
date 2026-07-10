-- prograph schema v10 — declared edges (M12). Two CHECK constraints widen:
--   edges.kind          += 'declared'
--   drift_findings.kind += 'stale_declaration', entity_kind += 'declared_path'
-- SQLite cannot ALTER a CHECK -> rebuild both tables (pattern per v6).

ALTER TABLE edges RENAME TO _edges_v9;

CREATE TABLE edges (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL CHECK (kind IN ('package_dep', 'mcp_call', 'contract_link', 'declared')),
    from_kind   TEXT NOT NULL CHECK (from_kind IN ('project', 'contract')),
    from_id     INTEGER NOT NULL,
    to_kind     TEXT NOT NULL CHECK (to_kind IN ('project', 'contract')),
    to_id       INTEGER NOT NULL,
    attrs_json  TEXT NOT NULL DEFAULT '{}',
    attrs_hash  TEXT NOT NULL,
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
    UNIQUE(kind, from_kind, from_id, to_kind, to_id, attrs_hash)
);

INSERT INTO edges SELECT * FROM _edges_v9;

DROP TABLE _edges_v9;

CREATE INDEX idx_edges_last_seen ON edges(last_seen);
CREATE INDEX idx_edges_from ON edges(from_kind, from_id);
CREATE INDEX idx_edges_to ON edges(to_kind, to_id);

ALTER TABLE drift_findings RENAME TO _drift_findings_v9;

CREATE TABLE drift_findings (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL REFERENCES projects(id),
    -- 'missing' | 'extra' | 'stale_todo' | 'stale_declaration'.
    kind            TEXT NOT NULL CHECK(kind IN ('missing','extra','stale_todo','stale_declaration')),
    -- 'public_symbol' | 'mcp_tool' | 'contract' | 'todo' | 'declared_path'.
    entity_kind     TEXT NOT NULL CHECK(entity_kind IN ('public_symbol','mcp_tool','contract','todo','declared_path')),
    entity_name     TEXT NOT NULL,
    source_path     TEXT NOT NULL,
    source_line     INTEGER NOT NULL DEFAULT 0,
    confidence      TEXT NOT NULL CHECK(confidence IN ('high','low')),
    detail          TEXT,
    first_seen      INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen       INTEGER NOT NULL REFERENCES snapshots(id),
    UNIQUE(project_id, kind, entity_kind, entity_name, source_path, source_line)
);

INSERT INTO drift_findings SELECT * FROM _drift_findings_v9;

DROP TABLE _drift_findings_v9;

CREATE INDEX idx_drift_last_seen   ON drift_findings(last_seen);
CREATE INDEX idx_drift_project     ON drift_findings(project_id);
CREATE INDEX idx_drift_kind        ON drift_findings(kind);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (10, datetime('now'));
