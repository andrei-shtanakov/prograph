-- prograph schema v8 — cross-project symbol references.
-- Auxiliary data, NOT a new edge kind. One row per resolved import:line cite.
-- Indexed for both directions: refs FROM a project (outbound list) and refs TO
-- a project (inbound references — answers "who calls my symbol?").

CREATE TABLE IF NOT EXISTS cross_project_symbol_refs (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    from_project_id INTEGER NOT NULL REFERENCES projects(id),
    from_module_id  INTEGER NOT NULL REFERENCES modules(id),
    line            INTEGER NOT NULL,
    to_project_id   INTEGER NOT NULL REFERENCES projects(id),
    -- Module path inside the target project, e.g. "atp_platform.sdk" or "a::b".
    to_module_path  TEXT NOT NULL,
    -- Symbol name imported. NULL if the import was module-level only (e.g.
    -- `import atp_platform.sdk` rather than `from atp_platform.sdk import X`).
    to_symbol_name  TEXT,
    first_seen      INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen       INTEGER NOT NULL REFERENCES snapshots(id),
    UNIQUE(from_module_id, line, to_project_id, to_module_path, to_symbol_name)
);

CREATE INDEX IF NOT EXISTS idx_cpsr_last_seen  ON cross_project_symbol_refs(last_seen);
CREATE INDEX IF NOT EXISTS idx_cpsr_from_proj  ON cross_project_symbol_refs(from_project_id);
CREATE INDEX IF NOT EXISTS idx_cpsr_to_proj    ON cross_project_symbol_refs(to_project_id);
CREATE INDEX IF NOT EXISTS idx_cpsr_to_symbol  ON cross_project_symbol_refs(to_project_id, to_symbol_name);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (8, datetime('now'));
