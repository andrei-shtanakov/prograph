-- prograph schema v5 — adds the search_fts virtual table for MCP `search` tool.
-- M2 spec §5.1 specified search_fts but earlier milestones deferred it. M7 finally lands.

CREATE VIRTUAL TABLE IF NOT EXISTS search_fts USING fts5(
    entity_kind,    -- 'project' | 'contract'
    entity_id UNINDEXED,
    snapshot_id UNINDEXED,
    name,
    body,
    tokenize = 'porter unicode61'
);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (5, datetime('now'));
