-- prograph schema v1 — minimal, M1 scope.
-- M2+ adds: contracts, contract_files, edges, edge_evidence, change_log, search_fts.

PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS snapshots (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    ts               TEXT NOT NULL,
    monorepo_root    TEXT NOT NULL,
    git_commit       TEXT,
    prograph_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    root_path   TEXT NOT NULL UNIQUE,
    kind        TEXT NOT NULL CHECK (kind IN ('python', 'rust', 'js', 'docs', 'mixed')),
    attrs_json  TEXT NOT NULL DEFAULT '{}',
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id)
);

CREATE INDEX IF NOT EXISTS idx_projects_last_seen ON projects(last_seen);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (1, datetime('now'));
