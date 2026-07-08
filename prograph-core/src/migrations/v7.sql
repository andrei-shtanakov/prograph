-- prograph schema v7 — module-level facts (public symbols + internal imports).
-- Sub-data of projects; no change_log entries are emitted for these rows directly.

CREATE TABLE IF NOT EXISTS modules (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id),
    rel_path    TEXT NOT NULL,
    language    TEXT NOT NULL CHECK (language IN ('python', 'rust', 'js')),
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
    UNIQUE(project_id, rel_path)
);

CREATE INDEX IF NOT EXISTS idx_modules_last_seen ON modules(last_seen);
CREATE INDEX IF NOT EXISTS idx_modules_project ON modules(project_id);

CREATE TABLE IF NOT EXISTS public_symbols (
    module_id   INTEGER NOT NULL REFERENCES modules(id),
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,
    line        INTEGER NOT NULL,
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
    PRIMARY KEY(module_id, name)
);

CREATE INDEX IF NOT EXISTS idx_public_symbols_last_seen ON public_symbols(last_seen);

CREATE TABLE IF NOT EXISTS internal_imports (
    module_id   INTEGER NOT NULL REFERENCES modules(id),
    target_path TEXT NOT NULL,
    line        INTEGER NOT NULL,
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
    PRIMARY KEY(module_id, target_path, line)
);

CREATE INDEX IF NOT EXISTS idx_internal_imports_last_seen ON internal_imports(last_seen);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (7, datetime('now'));
