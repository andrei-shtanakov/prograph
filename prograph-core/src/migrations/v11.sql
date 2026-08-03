-- prograph schema v11 — per-snapshot per-project git provenance
-- (conformance-report versioned evidence, spec 2026-08-03 D3).
CREATE TABLE IF NOT EXISTS project_git_states (
    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id),
    project_id  INTEGER NOT NULL REFERENCES projects(id),
    git_commit  TEXT,               -- HEAD sha at index time; NULL when not a git repo
    git_dirty   INTEGER,            -- 0/1; NULL when not a git repo
    PRIMARY KEY (snapshot_id, project_id)
);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (11, datetime('now'));
