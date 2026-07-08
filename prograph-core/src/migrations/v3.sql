-- prograph schema v3 — adds contracts + contract_files, widens edges.kind and
-- change_log.entity_kind CHECK constraints. Strict superset of v2.

-- 1. New contract entity (first-class graph node) + per-project file occurrences.
CREATE TABLE IF NOT EXISTS contracts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    declared_id   TEXT,
    content_hash  TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('json_schema', 'openapi', 'proto')),
    first_seen    INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen     INTEGER NOT NULL REFERENCES snapshots(id),
    UNIQUE(declared_id, content_hash)
);

CREATE INDEX IF NOT EXISTS idx_contracts_last_seen ON contracts(last_seen);

CREATE TABLE IF NOT EXISTS contract_files (
    contract_id INTEGER NOT NULL REFERENCES contracts(id),
    project_id  INTEGER NOT NULL REFERENCES projects(id),
    rel_path    TEXT NOT NULL,
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
    PRIMARY KEY(contract_id, project_id, rel_path)
);

CREATE INDEX IF NOT EXISTS idx_contract_files_last_seen ON contract_files(last_seen);

-- 2. Widen edges.kind CHECK. SQLite cannot ALTER CHECK in place; rename + recreate.
ALTER TABLE edges RENAME TO _edges_v2;

CREATE TABLE edges (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL CHECK (kind IN ('package_dep', 'mcp_call', 'contract_link')),
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

INSERT INTO edges SELECT * FROM _edges_v2;

DROP TABLE _edges_v2;

CREATE INDEX idx_edges_last_seen ON edges(last_seen);
CREATE INDEX idx_edges_from ON edges(from_kind, from_id);
CREATE INDEX idx_edges_to ON edges(to_kind, to_id);

-- 3. Widen change_log.entity_kind CHECK.
ALTER TABLE change_log RENAME TO _change_log_v2;

CREATE TABLE change_log (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_id  INTEGER NOT NULL REFERENCES snapshots(id),
    ts           TEXT NOT NULL,
    entity_kind  TEXT NOT NULL CHECK (entity_kind IN ('project', 'edge', 'contract')),
    entity_id    INTEGER NOT NULL,
    change       TEXT NOT NULL CHECK (change IN ('added', 'removed', 'attrs_changed')),
    before_json  TEXT,
    after_json   TEXT
);

INSERT INTO change_log SELECT * FROM _change_log_v2;

DROP TABLE _change_log_v2;

CREATE INDEX idx_change_log_snapshot ON change_log(snapshot_id);
CREATE INDEX idx_change_log_entity ON change_log(entity_kind, entity_id);

-- 4. Record schema version.
INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (3, datetime('now'));
