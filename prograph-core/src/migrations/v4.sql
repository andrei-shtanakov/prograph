-- prograph schema v4 — adds mcp_tool_decls for "MCP tools exposed" MD section.
-- Sub-data of projects; no change_log entries are emitted for these rows directly —
-- a removed tool decl surfaces via the corresponding mcp_call edge becoming Removed.

CREATE TABLE IF NOT EXISTS mcp_tool_decls (
    project_id  INTEGER NOT NULL REFERENCES projects(id),
    tool_name   TEXT NOT NULL,
    rel_path    TEXT NOT NULL,
    line        INTEGER NOT NULL,
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
    PRIMARY KEY(project_id, tool_name)
);

CREATE INDEX IF NOT EXISTS idx_mcp_tool_decls_last_seen ON mcp_tool_decls(last_seen);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (4, datetime('now'));
