-- prograph schema v6 — repair edge_evidence's FK reference.
--
-- Background: pre-M7 versions of prograph applied migration v3 without
-- `PRAGMA legacy_alter_table = ON`. SQLite's default behaviour rewrote any
-- table whose FK pointed at the renamed `_edges_v2` to keep the dangling
-- reference — which dropped silently when v3 then `DROP TABLE _edges_v2`.
-- Databases created after the M7 fix have the correct FK; older ones do not.
--
-- This migration is a one-shot rebuild of edge_evidence that re-targets the FK
-- at `edges`. Any existing rows survive the rebuild.

ALTER TABLE edge_evidence RENAME TO _edge_evidence_v5;

CREATE TABLE edge_evidence (
    edge_id     INTEGER NOT NULL REFERENCES edges(id),
    project_id  INTEGER NOT NULL REFERENCES projects(id),
    rel_path    TEXT NOT NULL,
    line        INTEGER,
    snippet     TEXT,
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
    PRIMARY KEY(edge_id, project_id, rel_path, line)
);

CREATE INDEX IF NOT EXISTS idx_edge_evidence_last_seen ON edge_evidence(last_seen);

INSERT INTO edge_evidence SELECT * FROM _edge_evidence_v5;

DROP TABLE _edge_evidence_v5;

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (6, datetime('now'));
