-- prograph schema v2 — adds edges, edge_evidence, change_log.
-- Additive over v1 (snapshots + projects). M3+ may add contracts/contract_files/search_fts.

CREATE TABLE IF NOT EXISTS edges (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL CHECK (kind IN ('package_dep')),
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

CREATE INDEX IF NOT EXISTS idx_edges_last_seen ON edges(last_seen);
CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_kind, from_id);
CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_kind, to_id);

CREATE TABLE IF NOT EXISTS edge_evidence (
    edge_id     INTEGER NOT NULL REFERENCES edges(id),
    project_id  INTEGER NOT NULL REFERENCES projects(id),
    rel_path    TEXT NOT NULL,
    line        INTEGER,
    snippet     TEXT,
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
    PRIMARY KEY(edge_id, project_id, rel_path, line)
);

CREATE TABLE IF NOT EXISTS change_log (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_id  INTEGER NOT NULL REFERENCES snapshots(id),
    ts           TEXT NOT NULL,
    entity_kind  TEXT NOT NULL CHECK (entity_kind IN ('project', 'edge')),
    entity_id    INTEGER NOT NULL,
    change       TEXT NOT NULL CHECK (change IN ('added', 'removed', 'attrs_changed')),
    before_json  TEXT,
    after_json   TEXT
);

CREATE INDEX IF NOT EXISTS idx_change_log_snapshot ON change_log(snapshot_id);
CREATE INDEX IF NOT EXISTS idx_change_log_entity ON change_log(entity_kind, entity_id);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (2, datetime('now'));
