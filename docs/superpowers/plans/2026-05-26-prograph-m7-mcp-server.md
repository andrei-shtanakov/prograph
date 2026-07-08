# prograph M7 — MCP stdio Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After M7, `prograph mcp` is a working MCP stdio server that AI agents (Claude Code, custom skills) consume to ground their reasoning about the monorepo. Eight tools are exposed: `monorepo_overview`, `list_projects`, `describe_project`, `find_edges`, `edge_evidence`, `changelog`, `search`, `snapshot_info`. Tool calls answer in <100ms against a 10-project monorepo. The server is configured via Claude Code's `mcp_servers` settings or invoked directly. Additionally, M7 makes MCP detection patterns user-configurable so arbiter-style custom idioms can be picked up by dropping a `.scm` file into `.prograph/mcp_patterns/`.

**Architecture:**
- **One Python entry point** — `prograph/mcp_server.py` registers tools using the `mcp` Python SDK. Every tool is a 5-20 line wrapper that reads `_core.<query>(...)`, converts via pydantic `from_core`, returns the model as a dict.
- **Three new Rust queries**: `Store::project_by_name`, `Store::snapshot_by_id`, `Store::find_edges_filtered` — needed by the MCP tools that aren't already covered by M5's `describe_*` / `monorepo_overview` aggregations.
- **Schema v5** adds: (a) the `search_fts` virtual table (FTS5) — populated each snapshot from project + contract names/descriptions; (b) actual data population of `edge_evidence` for MCP edges. The `edge_evidence` table itself exists since M2 — M7 finally populates it for `mcp_call` edges via the indexer's call-site → evidence pipeline.
- **`EdgeCandidate` gains an `evidence: Vec<(rel_path, line)>` field**; the MCP detector populates it from `McpClientUse` facts. Indexer persists rows via a new `SnapshotWriter::insert_edge_evidence`. Other edge kinds leave evidence empty in M7; backfill is a later milestone.
- **Configurable MCP patterns**: `parsers/python.rs` and `parsers/rust.rs` read `<monorepo_root>/.prograph/mcp_patterns/python.scm` and `rust.scm` if present and append them to the bundled query at runtime. Users can extend without forking the crate.
- **All tools return JSON-compatible dicts.** Pydantic models from M5 are reused — every tool's response is a `model_dump(mode="json")` of an existing or newly-introduced pydantic type.

**Tech Stack additions (M7 only):**
- `mcp` (Python SDK, ≥1.0) — official Anthropic MCP server bindings
- `pytest-asyncio` (dev-dep) — async test runner for MCP stdio integration tests

No new Rust deps. FTS5 ships with SQLite via `rusqlite`'s `bundled` feature — no extension wrangling needed.

**Spec reference:** `docs/superpowers/specs/2026-05-25-prograph-design.md` §7.2 — MCP tool surface (the 8 tools shipped here). §5.1 schema (search_fts virtual table; edge_evidence column shape).

**Baseline:** Branch off `main` at the M5 close commit `67dc674`. 119 cargo + 89 pytest + 1 realmonorepo passing; CI green; M5 produced the `prograph.export` + `Store::describe_*` foundations the MCP server reuses heavily.

**M7 explicitly out of scope (deferred to later):**
- **Browser UI** — M6 (which we deliberately deferred to ship the AI-facing path first).
- **Edge evidence for package_dep / contract_link** — only mcp_call evidence is persisted in M7. Backfill of the other kinds is a future polish.
- **HTTP / SSE MCP transport** — only stdio in M7. Remote AI access can land later.
- **Module-level facts** (public Python symbols, internal imports) — orthogonal parser-expansion milestone.
- **JS MCP detection** — no driver in the target monorepo (per M4 design call).

---

## File Structure (created/modified in M7)

```
prograph/
├── Cargo.toml                                  # unchanged
├── prograph-core/
│   ├── src/
│   │   ├── lib.rs                              # MODIFY — register new PyO3 wrappers + exports
│   │   ├── models.rs                           # MODIFY — EdgeEvidence pyclass, SearchHit pyclass,
│   │   │                                       #   widen / extend Edge return for find_edges
│   │   ├── store.rs                            # MODIFY — project_by_name, snapshot_by_id,
│   │   │                                       #   find_edges_filtered, edge_evidence_for,
│   │   │                                       #   search_fts query helpers; populate_search_fts
│   │   ├── indexer.rs                          # MODIFY — populate search_fts + edge_evidence
│   │   │                                       #   for MCP edges
│   │   ├── detectors/
│   │   │   ├── mod.rs                          # MODIFY — EdgeCandidate.evidence field
│   │   │   └── mcp.rs                          # MODIFY — populate evidence from McpClientUse
│   │   ├── parsers/
│   │   │   ├── python.rs                       # MODIFY — read mcp_patterns/python.scm override
│   │   │   └── rust.rs                         # MODIFY — read mcp_patterns/rust.scm override
│   │   └── migrations/
│   │       └── v5.sql                          # NEW — search_fts virtual table
├── prograph/
│   ├── _core.pyi                               # MODIFY — stubs for new pyclasses + functions
│   ├── __init__.py                             # MODIFY — re-export new pydantic types
│   ├── models.py                               # MODIFY — pydantic mirrors for new types
│   ├── paths.py                                # MODIFY — mcp_patterns_dir
│   ├── pyproject.toml                          # MODIFY — add mcp + pytest-asyncio deps
│   ├── cli.py                                  # MODIFY — prograph mcp command
│   └── mcp_server.py                           # NEW — MCP stdio server entry + tool registrations
├── tests/
│   ├── integration/
│   │   ├── test_cli_mcp.py                     # NEW — async MCP integration tests
│   │   └── test_smoke_real.py                  # MODIFY — smoke against real all_ai_orchestrators
│   └── unit/
│       └── test_mcp_tools.py                   # NEW — direct tool function tests (no async)
```

---

## Task 1: Workspace deps — `mcp` Python SDK + `pytest-asyncio`

**Files:**
- Modify: `prograph/pyproject.toml`

- [ ] **Step 1: Add the two new deps**

In `prograph/pyproject.toml`, append to `[project.dependencies]`:
```toml
mcp = ">=1.0"
```

And to `[dependency-groups.dev]` (or wherever `pytest` is declared):
```toml
pytest-asyncio = ">=0.23"
```

- [ ] **Step 2: Verify sync**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv sync --reinstall-package prograph
uv run python -c "from mcp.server import Server; from mcp.server.stdio import stdio_server; print('mcp OK')"
uv run python -c "import pytest_asyncio; print('pytest-asyncio OK')"
```

Both prints should succeed.

- [ ] **Step 3: Commit**

```sh
git add prograph/pyproject.toml prograph/uv.lock
git commit -m "prograph: M7 add mcp + pytest-asyncio dependencies"
```

---

## Task 2: Schema v5 — `search_fts` virtual table

**Files:**
- Create: `prograph-core/src/migrations/v5.sql`
- Modify: `prograph-core/src/store.rs`

FTS5 virtual table indexed on project name + kind + root_path + manifest snippet + contract id + content_hash prefix. Populated by the indexer (Task 7); for now just create the table.

- [ ] **Step 1: Write `v5.sql`**

`prograph-core/src/migrations/v5.sql`:
```sql
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
```

- [ ] **Step 2: Register the migration**

In `prograph-core/src/store.rs`, append to `MIGRATIONS`:
```rust
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/v1.sql")),
    (2, include_str!("migrations/v2.sql")),
    (3, include_str!("migrations/v3.sql")),
    (4, include_str!("migrations/v4.sql")),
    (5, include_str!("migrations/v5.sql")),
];
```

- [ ] **Step 3: Add a test**

Append to `store.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn schema_v5_creates_search_fts() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        // FTS5 tables appear in sqlite_master with type='table'; the helper tables
        // (search_fts_data, search_fts_idx, etc.) also exist.
        let names: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE name LIKE 'search_fts%' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"search_fts".to_string()));
        assert_eq!(store.schema_version().unwrap(), 5);
    }

    #[test]
    fn search_fts_accepts_inserts_and_returns_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        store.connection().execute(
            "INSERT INTO search_fts (entity_kind, entity_id, snapshot_id, name, body)
             VALUES ('project', 1, 1, 'Maestro', 'DAG orchestrator and runtime')",
            [],
        ).unwrap();
        let n: i64 = store.connection().query_row(
            "SELECT COUNT(*) FROM search_fts WHERE search_fts MATCH 'orchestrator'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert_eq!(n, 1);
    }
```

- [ ] **Step 4: Run tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core store
```
Expected: 20 store tests (18 prior + 2 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 121 tests.

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/migrations/v5.sql prograph/prograph-core/src/store.rs
git commit -m "prograph: M7 schema v5 — search_fts virtual table"
```

---

## Task 3: `EdgeCandidate.evidence` field + MCP detector populates it

**Files:**
- Modify: `prograph-core/src/detectors/mod.rs`
- Modify: `prograph-core/src/detectors/mcp.rs`
- Modify: `prograph-core/src/detectors/deps.rs` + `contracts.rs` (compile-fix)

`EdgeCandidate` gains `evidence: Vec<EvidenceLocation>` so the indexer can persist per-edge call sites. M7 only fills this for `mcp_call`; package_dep and contract_link emit empty vecs.

- [ ] **Step 1: Add `EvidenceLocation` + extend `EdgeCandidate`**

In `prograph-core/src/detectors/mod.rs`, add:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceLocation {
    /// Index into the `Vec<ProjectFacts>` — resolved to a project_id at persist time.
    pub project_idx: usize,
    pub rel_path: String,
    pub line: i64,
    pub snippet: Option<String>,
}
```

And modify `EdgeCandidate`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeCandidate {
    pub kind: EdgeKind,
    pub from_kind: NodeKind,
    pub from_idx: usize,
    pub to_kind: NodeKind,
    pub to_idx: usize,
    pub attrs_json: String,
    pub attrs_hash: String,
    /// Source-line locations that justify this edge. Empty for kinds where we
    /// don't yet track evidence (package_dep, contract_link in M7).
    pub evidence: Vec<EvidenceLocation>,
}
```

- [ ] **Step 2: Compile-fix `deps.rs` and `contracts.rs`**

In each existing `EdgeCandidate { ... }` construction in `detectors/deps.rs` and `detectors/contracts.rs`, append `evidence: Vec::new()` to the struct literal. The compiler will flag every site.

- [ ] **Step 3: Populate evidence in `mcp.rs`**

In `prograph-core/src/detectors/mcp.rs`, locate the place where `mcp_call` `EdgeCandidate`s are constructed (inside the loop over `consumer.mcp_uses`). Currently it dedupes call sites into one edge per `(consumer, server, tool)`. Change it to accumulate evidence:

Replace the inner loop with:
```rust
    let mut seen: HashMap<(usize, usize, String), EdgeCandidate> = HashMap::new();

    for (consumer_idx, consumer) in facts.iter().enumerate() {
        for use_site in &consumer.mcp_uses {
            let Some(&server_idx) = servers.get(use_site.tool_name.as_str()) else {
                continue;
            };
            if server_idx == consumer_idx {
                continue;
            }

            let key = (consumer_idx, server_idx, use_site.tool_name.clone());
            let evidence = super::EvidenceLocation {
                project_idx: consumer_idx,
                rel_path: use_site.rel_path.clone(),
                line: use_site.line as i64,
                snippet: None,
            };

            match seen.entry(key) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    e.get_mut().evidence.push(evidence);
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    let attrs = serde_json::json!({"tool": use_site.tool_name});
                    let attrs_json = serde_json::to_string(&attrs).unwrap();
                    let mut hasher = Sha256::new();
                    hasher.update(b"mcp_call|");
                    hasher.update(use_site.tool_name.as_bytes());
                    let attrs_hash = format!("{:x}", hasher.finalize());

                    v.insert(EdgeCandidate {
                        kind: EdgeKind::McpCall,
                        from_kind: NodeKind::Project,
                        from_idx: consumer_idx,
                        to_kind: NodeKind::Project,
                        to_idx: server_idx,
                        attrs_json,
                        attrs_hash,
                        evidence: vec![evidence],
                    });
                }
            }
        }
    }
```

Also, sort each candidate's evidence deterministically before returning (so persisted rows are byte-stable):
```rust
    let mut out: Vec<EdgeCandidate> = seen.into_values().collect();
    for cand in out.iter_mut() {
        cand.evidence.sort_by(|a, b| {
            (a.rel_path.as_str(), a.line).cmp(&(b.rel_path.as_str(), b.line))
        });
    }
    out.sort_by(|a, b| (a.from_idx, a.to_idx, &a.attrs_hash).cmp(&(b.from_idx, b.to_idx, &b.attrs_hash)));
    out
```

- [ ] **Step 4: Update mcp_detector tests**

In the existing `#[cfg(test)] mod tests` block of `detectors/mcp.rs`, the `dedupes_multiple_call_sites_in_same_client` test now needs to also verify that ALL three call sites land in `evidence`. Update its assertions:

```rust
    #[test]
    fn dedupes_multiple_call_sites_into_evidence() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        // Construct a client with three call sites on the same tool.
        let mut consumer = fact("client", &[], &["decide"]);
        consumer.mcp_uses = vec![
            crate::facts::McpClientUse { tool_name: "decide".into(), rel_path: "a.py".into(), line: 10 },
            crate::facts::McpClientUse { tool_name: "decide".into(), rel_path: "a.py".into(), line: 20 },
            crate::facts::McpClientUse { tool_name: "decide".into(), rel_path: "b.py".into(), line: 5 },
        ];
        let facts = vec![consumer, fact("server", &["decide"], &[])];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1, "three call sites → one edge");
        assert_eq!(edges[0].evidence.len(), 3, "all three sites should land in evidence");
        // Verify sort order: rel_path then line.
        assert_eq!(edges[0].evidence[0].rel_path, "a.py");
        assert_eq!(edges[0].evidence[0].line, 10);
        assert_eq!(edges[0].evidence[1].line, 20);
        assert_eq!(edges[0].evidence[2].rel_path, "b.py");
    }
```

Delete or rename the old `dedupes_multiple_call_sites_in_same_client` test if its assertions conflict.

- [ ] **Step 5: Run tests**

```sh
cargo test --package prograph-core detectors
```
Expected: ≥18 detectors tests still pass with the updated assertions.

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 121 still + however many new asserts. Verify no regression.

- [ ] **Step 6: Commit**

```sh
git add prograph/prograph-core/src/detectors/mod.rs prograph/prograph-core/src/detectors/mcp.rs \
        prograph/prograph-core/src/detectors/deps.rs prograph/prograph-core/src/detectors/contracts.rs
git commit -m "prograph: M7 EdgeCandidate.evidence + mcp_detector populates per-call-site evidence"
```

---

## Task 4: Indexer — persist edge_evidence + populate search_fts

**Files:**
- Modify: `prograph-core/src/indexer.rs`
- Modify: `prograph-core/src/store.rs`

The indexer now:
1. Persists `edge_evidence` rows for each `EdgeCandidate.evidence` entry (only mcp_call in M7).
2. Populates `search_fts` after the snapshot is committed — clears entries for the latest snapshot first, then inserts new ones.

- [ ] **Step 1: Add `SnapshotWriter::insert_edge_evidence`**

Append to `impl<'a> SnapshotWriter<'a>` in `prograph-core/src/store.rs`:
```rust
    pub fn insert_edge_evidence(
        &self,
        edge_id: i64,
        project_id: i64,
        rel_path: &str,
        line: i64,
        snippet: Option<&str>,
        snapshot_id: i64,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO edge_evidence
             (edge_id, project_id, rel_path, line, snippet, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?,
                     COALESCE((SELECT first_seen FROM edge_evidence
                               WHERE edge_id=? AND project_id=? AND rel_path=? AND line=?), ?),
                     ?)",
            rusqlite::params![
                edge_id, project_id, rel_path, line, snippet,
                edge_id, project_id, rel_path, line, snapshot_id,
                snapshot_id
            ],
        )?;
        Ok(())
    }
```

Same COALESCE-on-first_seen pattern as M5's `insert_mcp_tool_decl`.

- [ ] **Step 2: Add `SnapshotWriter::clear_and_repopulate_search_fts`**

```rust
    /// Clear the FTS index for the given snapshot id and repopulate from current state.
    /// Called at the end of the persist phase, after all projects/contracts are written.
    pub fn rebuild_search_fts(&self, snapshot_id: i64) -> Result<()> {
        self.tx.execute(
            "DELETE FROM search_fts WHERE snapshot_id = ?",
            rusqlite::params![snapshot_id],
        )?;

        // Project rows.
        self.tx.execute(
            "INSERT INTO search_fts (entity_kind, entity_id, snapshot_id, name, body)
             SELECT 'project', id, ?, name,
                    COALESCE(name, '') || ' ' || COALESCE(kind, '') || ' ' ||
                    COALESCE(root_path, '') || ' ' || COALESCE(attrs_json, '')
             FROM projects WHERE last_seen = ?",
            rusqlite::params![snapshot_id, snapshot_id],
        )?;

        // Contract rows.
        self.tx.execute(
            "INSERT INTO search_fts (entity_kind, entity_id, snapshot_id, name, body)
             SELECT 'contract', id, ?, COALESCE(declared_id, content_hash),
                    COALESCE(declared_id, '') || ' ' || COALESCE(kind, '') || ' ' ||
                    SUBSTR(COALESCE(content_hash, ''), 1, 16)
             FROM contracts WHERE last_seen = ?",
            rusqlite::params![snapshot_id, snapshot_id],
        )?;

        Ok(())
    }
```

- [ ] **Step 3: Wire edge_evidence + rebuild_search_fts in `indexer.rs`**

In `prograph-core/src/indexer.rs`, locate the edge persist loop. For each `Added` mcp_call edge that gets persisted, also write its evidence rows. Insert immediately after each `writer.insert_edge(...)` that returns an `eid`:

Inside the existing `Added` arm of the edge persist match:
```rust
            DiffChange::Added => {
                // ... existing endpoint resolution + writer.insert_edge → eid ...

                // Persist evidence (M7).
                // Only mcp_call has populated evidence in M7; others have empty vecs.
                let candidate_ev = edge_candidates.iter().find(|c| c.attrs_hash == attrs_hash)
                    .map(|c| c.evidence.clone()).unwrap_or_default();
                for ev in &candidate_ev {
                    let proj_root = &facts[ev.project_idx].project_root;
                    if let Some(&pid) = new_project_ids.get(proj_root) {
                        writer.insert_edge_evidence(
                            eid, pid, &ev.rel_path, ev.line,
                            ev.snippet.as_deref(), snap_id,
                        )?;
                    }
                }
                // ... existing change_log + n_changes += 1 ...
            }
```

For `Unchanged`/`AttrsChanged` mcp_call edges, also refresh evidence — call sites may have moved line numbers:
```rust
            DiffChange::Unchanged | DiffChange::AttrsChanged => {
                let (eid, _) = &alive_edges_by_root[&entry.identity_key];
                // ... existing touch_edge ...

                // M7: re-persist evidence so line numbers stay current.
                let candidate_ev = edge_candidates.iter().find(|c| c.attrs_hash == attrs_hash)
                    .map(|c| c.evidence.clone()).unwrap_or_default();
                for ev in &candidate_ev {
                    let proj_root = &facts[ev.project_idx].project_root;
                    if let Some(&pid) = new_project_ids.get(proj_root) {
                        writer.insert_edge_evidence(
                            *eid, pid, &ev.rel_path, ev.line,
                            ev.snippet.as_deref(), snap_id,
                        )?;
                    }
                }
                // ... existing change_log if AttrsChanged ...
            }
```

(The actual code currently has these arms separate — keep them separate but add the evidence-refresh block to each.)

Then, at the very end of the indexer (just before `writer.commit()?`), add:
```rust
    writer.rebuild_search_fts(snap_id)?;
```

- [ ] **Step 4: Tests**

Append to `indexer.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn edge_evidence_persisted_for_mcp_call() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();

        fs::create_dir_all(dir.path().join("server")).unwrap();
        fs::write(dir.path().join("server/pyproject.toml"),
            r#"[project]
name = "srv"
"#,
        ).unwrap();
        fs::write(dir.path().join("server/server.py"),
            r#"@server.tool()
def decide():
    return 1
"#,
        ).unwrap();

        fs::create_dir_all(dir.path().join("client")).unwrap();
        fs::write(dir.path().join("client/pyproject.toml"),
            r#"[project]
name = "cli"
"#,
        ).unwrap();
        fs::write(dir.path().join("client/client.py"),
            r#"async def run(session):
    a = await session.call_tool("decide", {})
    b = await session.call_tool("decide", {})
"#,
        ).unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        let n_evidence: i64 = store.connection().query_row(
            "SELECT COUNT(*) FROM edge_evidence
             WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
            [],
            |r| r.get(0),
        ).unwrap();
        assert!(n_evidence >= 2, "expected ≥2 evidence rows for two call sites, got {}", n_evidence);
    }

    #[test]
    fn search_fts_populated_after_index() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();
        fs::create_dir_all(dir.path().join("maestro")).unwrap();
        fs::write(
            dir.path().join("maestro/pyproject.toml"),
            r#"[project]
name = "maestro"
"#,
        ).unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        let n: i64 = store.connection().query_row(
            "SELECT COUNT(*) FROM search_fts WHERE search_fts MATCH 'maestro'",
            [],
            |r| r.get(0),
        ).unwrap();
        assert!(n >= 1, "expected ≥1 FTS hit on 'maestro', got {}", n);
    }
```

- [ ] **Step 5: Run cargo tests**

```sh
cargo test --package prograph-core
```
Expected: 123 tests (121 + 2 new).

Verify clean (cargo fmt + clippy).

- [ ] **Step 6: Commit**

```sh
git add prograph/prograph-core/src/indexer.rs prograph/prograph-core/src/store.rs
git commit -m "prograph: M7 indexer — persist edge_evidence for mcp_call + rebuild search_fts each snapshot"
```

---

## Task 5: New Rust query helpers + PyO3 wrappers

**Files:**
- Modify: `prograph-core/src/store.rs`
- Modify: `prograph-core/src/lib.rs`
- Modify: `prograph-core/src/models.rs`
- Modify: `prograph/_core.pyi`

Three new methods + PyO3 wrappers covering the MCP tool needs not addressed by M5 aggregations:

- `Store::project_by_name(name)` → `Option<i64>` (project_id). Used by `describe_project` MCP tool which takes name not id.
- `Store::snapshot_by_id(id)` → `Option<SnapshotInfo>`. M5 already has `latest_snapshot_info`; need id-keyed variant.
- `Store::find_edges_filtered(from_name?, to_name?, kind?, since_snapshot?)` → `Vec<EdgeRow>` where `EdgeRow` is a pyclass with project/contract names denormalised.
- `Store::edge_evidence_for(edge_id)` → `Vec<EdgeEvidenceRow>` for the `edge_evidence` MCP tool.
- `Store::search_fts(q, kinds?, limit?)` → `Vec<SearchHit>`.
- `Store::changelog_paginated(since_snapshot?, entity_kind?, limit)` → `Vec<ChangeEvent>`.

That's 6 new methods. Let's group them as M7's "query helpers" task.

- [ ] **Step 1: Add new pyclasses to `models.rs`**

Append:
```rust
/// One edge row as returned by `find_edges_filtered` — denormalised with target/source names.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct EdgeRow {
    pub id: i64,
    pub kind: String,
    pub from_kind: String,
    pub from_id: i64,
    pub from_name: String,
    pub to_kind: String,
    pub to_id: i64,
    pub to_name: String,
    pub attrs_json: String,
    pub first_seen: i64,
    pub last_seen: i64,
}

#[pymethods]
impl EdgeRow {
    fn __repr__(&self) -> String {
        format!("EdgeRow({}: {} → {})", self.kind, self.from_name, self.to_name)
    }
}

/// One edge_evidence row.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct EdgeEvidenceRow {
    pub edge_id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub rel_path: String,
    pub line: i64,
    pub snippet: Option<String>,
}

/// One search FTS hit.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct SearchHit {
    pub entity_kind: String,    // 'project' | 'contract'
    pub entity_id: i64,
    pub name: String,
    pub snippet: String,         // FTS snippet with markers around matched terms
    pub rank: f64,
}
```

Extend `pub use models::{...}` in `lib.rs`:
```rust
pub use models::{
    ..., EdgeRow, EdgeEvidenceRow, SearchHit,
};
```

And register in `#[pymodule]`:
```rust
    m.add_class::<EdgeRow>()?;
    m.add_class::<EdgeEvidenceRow>()?;
    m.add_class::<SearchHit>()?;
```

- [ ] **Step 2: Add `project_by_name`**

Append to `impl Store`:
```rust
    pub fn project_by_name(&self, name: &str) -> Result<Option<i64>> {
        let row = self.conn.query_row(
            "SELECT id FROM projects
             WHERE name = ? AND last_seen = (SELECT MAX(id) FROM snapshots)",
            rusqlite::params![name],
            |r| r.get::<_, i64>(0),
        );
        match row {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
```

- [ ] **Step 3: Add `snapshot_by_id`**

```rust
    pub fn snapshot_by_id(&self, id: i64) -> Result<Option<crate::models::SnapshotInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, monorepo_root, git_commit, prograph_version,
                    (SELECT COUNT(*) FROM projects WHERE last_seen = s.id) AS n_projects,
                    (SELECT COUNT(*) FROM edges    WHERE last_seen = s.id) AS n_edges,
                    (SELECT COUNT(*) FROM change_log WHERE snapshot_id = s.id) AS n_changes
             FROM snapshots s
             WHERE s.id = ?",
        )?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        if let Some(r) = rows.next()? {
            Ok(Some(crate::models::SnapshotInfo {
                id: r.get(0)?,
                ts: r.get(1)?,
                monorepo_root: r.get(2)?,
                git_commit: r.get(3)?,
                prograph_version: r.get(4)?,
                n_projects: r.get(5)?,
                n_edges: r.get(6)?,
                n_changes: r.get(7)?,
            }))
        } else {
            Ok(None)
        }
    }
```

- [ ] **Step 4: Add `find_edges_filtered`**

```rust
    pub fn find_edges_filtered(
        &self,
        from_name: Option<&str>,
        to_name: Option<&str>,
        kind: Option<&str>,
        since_snapshot: Option<i64>,
    ) -> Result<Vec<crate::models::EdgeRow>> {
        let mut sql = String::from(
            "SELECT e.id, e.kind, e.from_kind, e.from_id,
                    CASE e.from_kind
                        WHEN 'project' THEN (SELECT name FROM projects WHERE id = e.from_id)
                        WHEN 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.from_id)
                    END AS from_name,
                    e.to_kind, e.to_id,
                    CASE e.to_kind
                        WHEN 'project' THEN (SELECT name FROM projects WHERE id = e.to_id)
                        WHEN 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.to_id)
                    END AS to_name,
                    e.attrs_json, e.first_seen, e.last_seen
             FROM edges e
             WHERE e.last_seen = (SELECT MAX(id) FROM snapshots)",
        );
        if kind.is_some() {
            sql.push_str(" AND e.kind = ?");
        }
        if from_name.is_some() {
            sql.push_str(" AND from_name = ?");
        }
        if to_name.is_some() {
            sql.push_str(" AND to_name = ?");
        }
        if since_snapshot.is_some() {
            sql.push_str(" AND e.first_seen >= ?");
        }
        sql.push_str(" ORDER BY e.kind, from_name, to_name");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(k) = kind { params.push(Box::new(k.to_string())); }
        if let Some(f) = from_name { params.push(Box::new(f.to_string())); }
        if let Some(t) = to_name { params.push(Box::new(t.to_string())); }
        if let Some(s) = since_snapshot { params.push(Box::new(s)); }

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |r| {
            Ok(crate::models::EdgeRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                from_kind: r.get(2)?,
                from_id: r.get(3)?,
                from_name: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                to_kind: r.get(5)?,
                to_id: r.get(6)?,
                to_name: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                attrs_json: r.get(8)?,
                first_seen: r.get(9)?,
                last_seen: r.get(10)?,
            })
        })?;

        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }
```

- [ ] **Step 5: Add `edge_evidence_for`**

```rust
    pub fn edge_evidence_for(&self, edge_id: i64) -> Result<Vec<crate::models::EdgeEvidenceRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT ev.edge_id, ev.project_id, p.name, ev.rel_path, ev.line, ev.snippet
             FROM edge_evidence ev
             JOIN projects p ON p.id = ev.project_id
             WHERE ev.edge_id = ? AND ev.last_seen = (SELECT MAX(id) FROM snapshots)
             ORDER BY ev.rel_path, ev.line",
        )?;
        let rows = stmt.query_map(rusqlite::params![edge_id], |r| {
            Ok(crate::models::EdgeEvidenceRow {
                edge_id: r.get(0)?,
                project_id: r.get(1)?,
                project_name: r.get(2)?,
                rel_path: r.get(3)?,
                line: r.get(4)?,
                snippet: r.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }
```

- [ ] **Step 6: Add `search_fts`**

```rust
    pub fn search_fts(
        &self,
        query: &str,
        kinds: Option<Vec<String>>,
        limit: i64,
    ) -> Result<Vec<crate::models::SearchHit>> {
        // Build the SQL. Use FTS5's snippet() for highlighted excerpt.
        let mut sql = String::from(
            "SELECT entity_kind, entity_id, name,
                    snippet(search_fts, 4, '[', ']', '…', 16) AS hit,
                    bm25(search_fts) AS rank
             FROM search_fts
             WHERE search_fts MATCH ? AND snapshot_id = (SELECT MAX(id) FROM snapshots)",
        );
        if kinds.is_some() {
            sql.push_str(" AND entity_kind IN (");
            // Will splice placeholders below.
            sql.push_str(&vec!["?"; kinds.as_ref().unwrap().len()].join(","));
            sql.push_str(")");
        }
        sql.push_str(" ORDER BY rank LIMIT ?");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(query.to_string())];
        if let Some(ks) = kinds {
            for k in ks {
                params.push(Box::new(k));
            }
        }
        params.push(Box::new(limit));

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |r| {
            Ok(crate::models::SearchHit {
                entity_kind: r.get(0)?,
                entity_id: r.get(1)?,
                name: r.get(2)?,
                snippet: r.get(3)?,
                rank: r.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }
```

- [ ] **Step 7: Add `changelog_paginated`**

```rust
    pub fn changelog_paginated(
        &self,
        since_snapshot: Option<i64>,
        entity_kind: Option<&str>,
        limit: i64,
    ) -> Result<Vec<crate::models::ChangeEvent>> {
        // Build query.
        let mut sql = String::from(
            "SELECT id, snapshot_id, ts, entity_kind, entity_id, change, before_json, after_json
             FROM change_log WHERE 1=1",
        );
        if since_snapshot.is_some() {
            sql.push_str(" AND snapshot_id >= ?");
        }
        if entity_kind.is_some() {
            sql.push_str(" AND entity_kind = ?");
        }
        sql.push_str(" ORDER BY snapshot_id DESC, id DESC LIMIT ?");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(s) = since_snapshot { params.push(Box::new(s)); }
        if let Some(k) = entity_kind { params.push(Box::new(k.to_string())); }
        params.push(Box::new(limit));

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |r| {
            // Map change_log row → ChangeEvent pyclass.
            let entity_kind_str: String = r.get(3)?;
            let change_str: String = r.get(5)?;
            Ok(crate::models::ChangeEvent {
                id: r.get(0)?,
                snapshot_id: r.get(1)?,
                ts: r.get(2)?,
                entity_kind: match entity_kind_str.as_str() {
                    "project" => crate::models::EntityKind::Project,
                    "edge" => crate::models::EntityKind::Edge,
                    "contract" => {
                        // ChangeEvent's EntityKind enum is M5-Project/Edge only; for M7 we
                        // extend EntityKind to include Contract (already done in M4 schema
                        // but enum still has 2 variants — verify Task adds Contract).
                        // For now, fall through to Edge to keep compilation; the MCP tool
                        // returns the raw string via entity_kind_str in the SQL.
                        // ACTUAL FIX: add EntityKind::Contract variant in Task 5 step 0.
                        crate::models::EntityKind::Edge
                    }
                    _ => crate::models::EntityKind::Edge,
                },
                entity_id: r.get(4)?,
                change: match change_str.as_str() {
                    "added" => crate::models::ChangeKind::Added,
                    "removed" => crate::models::ChangeKind::Removed,
                    "attrs_changed" => crate::models::ChangeKind::AttrsChanged,
                    _ => crate::models::ChangeKind::Added,
                },
                before_json: r.get(6)?,
                after_json: r.get(7)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }
```

**Sub-step 7a: Widen EntityKind to include Contract.**

In `prograph-core/src/models.rs`, modify the `EntityKind` enum:
```rust
pub enum EntityKind {
    Project,
    Edge,
    Contract,
}
```

And extend the `name()` method:
```rust
    fn name(&self) -> &'static str {
        match self {
            EntityKind::Project => "project",
            EntityKind::Edge => "edge",
            EntityKind::Contract => "contract",
        }
    }
```

Update `prograph/_core.pyi`:
```python
class EntityKind:
    Project: ClassVar[EntityKind]
    Edge: ClassVar[EntityKind]
    Contract: ClassVar[EntityKind]
    def name(self) -> str: ...
```

Update `prograph/models.py` pydantic enum:
```python
class EntityKind(str, Enum):
    PROJECT = "project"
    EDGE = "edge"
    CONTRACT = "contract"
```

Then go back into `changelog_paginated` and replace the placeholder `crate::models::EntityKind::Edge` for the `"contract"` case with `crate::models::EntityKind::Contract`.

- [ ] **Step 8: PyO3 wrappers**

In `prograph-core/src/lib.rs`, add (mirroring the existing `py_describe_project` pattern):
```rust
#[pyfunction]
#[pyo3(name = "project_by_name")]
fn py_project_by_name(db_path: &str, name: &str) -> PyResult<Option<i64>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.project_by_name(name)?)
}

#[pyfunction]
#[pyo3(name = "snapshot_by_id")]
fn py_snapshot_by_id(db_path: &str, id: i64) -> PyResult<Option<SnapshotInfo>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.snapshot_by_id(id)?)
}

#[pyfunction]
#[pyo3(name = "find_edges_filtered")]
fn py_find_edges_filtered(
    db_path: &str,
    from_name: Option<&str>,
    to_name: Option<&str>,
    kind: Option<&str>,
    since_snapshot: Option<i64>,
) -> PyResult<Vec<EdgeRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.find_edges_filtered(from_name, to_name, kind, since_snapshot)?)
}

#[pyfunction]
#[pyo3(name = "edge_evidence_for")]
fn py_edge_evidence_for(db_path: &str, edge_id: i64) -> PyResult<Vec<EdgeEvidenceRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.edge_evidence_for(edge_id)?)
}

#[pyfunction]
#[pyo3(name = "search_fts")]
fn py_search_fts(
    db_path: &str,
    query: &str,
    kinds: Option<Vec<String>>,
    limit: i64,
) -> PyResult<Vec<SearchHit>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.search_fts(query, kinds, limit)?)
}

#[pyfunction]
#[pyo3(name = "changelog_paginated")]
fn py_changelog_paginated(
    db_path: &str,
    since_snapshot: Option<i64>,
    entity_kind: Option<&str>,
    limit: i64,
) -> PyResult<Vec<ChangeEvent>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.changelog_paginated(since_snapshot, entity_kind, limit)?)
}
```

Register them inside `#[pymodule]`:
```rust
    m.add_function(wrap_pyfunction!(py_project_by_name, m)?)?;
    m.add_function(wrap_pyfunction!(py_snapshot_by_id, m)?)?;
    m.add_function(wrap_pyfunction!(py_find_edges_filtered, m)?)?;
    m.add_function(wrap_pyfunction!(py_edge_evidence_for, m)?)?;
    m.add_function(wrap_pyfunction!(py_search_fts, m)?)?;
    m.add_function(wrap_pyfunction!(py_changelog_paginated, m)?)?;
```

Extend `prograph/_core.pyi`:
```python
def project_by_name(db_path: str, name: str) -> int | None: ...
def snapshot_by_id(db_path: str, id: int) -> SnapshotInfo | None: ...
def find_edges_filtered(
    db_path: str,
    from_name: str | None = None,
    to_name: str | None = None,
    kind: str | None = None,
    since_snapshot: int | None = None,
) -> list[EdgeRow]: ...
def edge_evidence_for(db_path: str, edge_id: int) -> list[EdgeEvidenceRow]: ...
def search_fts(db_path: str, query: str, kinds: list[str] | None, limit: int) -> list[SearchHit]: ...
def changelog_paginated(
    db_path: str,
    since_snapshot: int | None,
    entity_kind: str | None,
    limit: int,
) -> list[ChangeEvent]: ...

class EdgeRow:
    id: int
    kind: str
    from_kind: str
    from_id: int
    from_name: str
    to_kind: str
    to_id: int
    to_name: str
    attrs_json: str
    first_seen: int
    last_seen: int

class EdgeEvidenceRow:
    edge_id: int
    project_id: int
    project_name: str
    rel_path: str
    line: int
    snippet: str | None

class SearchHit:
    entity_kind: str
    entity_id: int
    name: str
    snippet: str
    rank: float
```

- [ ] **Step 9: Test the queries**

Append to `store.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn project_by_name_finds_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid = writer.insert_project(snap, "Maestro", "./Maestro", "python", "{}").unwrap();
        writer.commit().unwrap();
        assert_eq!(store.project_by_name("Maestro").unwrap(), Some(pid));
        assert_eq!(store.project_by_name("nope").unwrap(), None);
    }

    #[test]
    fn snapshot_by_id_returns_with_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        writer.insert_project(snap, "x", "./x", "python", "{}").unwrap();
        writer.commit().unwrap();
        let info = store.snapshot_by_id(snap).unwrap().unwrap();
        assert_eq!(info.n_projects, 1);
        assert!(store.snapshot_by_id(9999).unwrap().is_none());
    }

    #[test]
    fn find_edges_filtered_by_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pa = writer.insert_project(snap, "a", "./a", "python", "{}").unwrap();
        let pb = writer.insert_project(snap, "b", "./b", "python", "{}").unwrap();
        writer.insert_edge(snap, "package_dep", "project", pa, "project", pb, "{}", "h1").unwrap();
        writer.insert_edge(snap, "mcp_call", "project", pa, "project", pb, r#"{"tool":"t"}"#, "h2").unwrap();
        writer.commit().unwrap();

        let only_mcp = store.find_edges_filtered(None, None, Some("mcp_call"), None).unwrap();
        assert_eq!(only_mcp.len(), 1);
        assert_eq!(only_mcp[0].kind, "mcp_call");

        let from_a = store.find_edges_filtered(Some("a"), None, None, None).unwrap();
        assert_eq!(from_a.len(), 2);
    }

    #[test]
    fn search_fts_returns_hits_with_snippet() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid = writer.insert_project(snap, "Maestro", "./Maestro", "python", r#"{"declared_name":"maestro orchestrator"}"#).unwrap();
        writer.rebuild_search_fts(snap).unwrap();
        writer.commit().unwrap();

        let hits = store.search_fts("orchestrator", None, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entity_id, pid);
        assert!(hits[0].snippet.contains("orchestrator"));
    }

    #[test]
    fn changelog_paginated_respects_limit_and_kind_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        writer.insert_change_log(snap, "ts", "project", 1, "added", None, Some("{}")).unwrap();
        writer.insert_change_log(snap, "ts", "edge", 1, "added", None, Some("{}")).unwrap();
        writer.insert_change_log(snap, "ts", "contract", 1, "added", None, Some("{}")).unwrap();
        writer.commit().unwrap();

        let all = store.changelog_paginated(None, None, 100).unwrap();
        assert_eq!(all.len(), 3);

        let only_projects = store.changelog_paginated(None, Some("project"), 100).unwrap();
        assert_eq!(only_projects.len(), 1);

        let limited = store.changelog_paginated(None, None, 2).unwrap();
        assert_eq!(limited.len(), 2);
    }
```

- [ ] **Step 10: Run + commit**

```sh
cargo test --package prograph-core
```
Expected: 128 (123 + 5 new).

Verify clean.

```sh
git add prograph/prograph-core/src/{models,store,lib}.rs prograph/prograph/_core.pyi prograph/prograph/models.py
git commit -m "prograph: M7 query helpers — project_by_name / snapshot_by_id / find_edges_filtered / edge_evidence_for / search_fts / changelog_paginated"
```

---

## Task 6: Pydantic mirrors for new types

**Files:**
- Modify: `prograph/models.py`
- Modify: `prograph/__init__.py`

Add `EdgeRow`, `EdgeEvidenceRow`, `SearchHit` pydantic mirrors with `from_core` classmethods.

- [ ] **Step 1: Append mirrors**

In `prograph/models.py`, append:
```python
class EdgeRow(BaseModel):
    model_config = ConfigDict(frozen=True)

    id: int
    kind: str
    from_kind: str
    from_id: int
    from_name: str
    to_kind: str
    to_id: int
    to_name: str
    attrs: dict[str, object]
    first_seen: int
    last_seen: int

    @classmethod
    def from_core(cls, value: _core.EdgeRow) -> EdgeRow:
        import json
        return cls(
            id=value.id,
            kind=value.kind,
            from_kind=value.from_kind,
            from_id=value.from_id,
            from_name=value.from_name,
            to_kind=value.to_kind,
            to_id=value.to_id,
            to_name=value.to_name,
            attrs=json.loads(value.attrs_json) if value.attrs_json else {},
            first_seen=value.first_seen,
            last_seen=value.last_seen,
        )


class EdgeEvidenceRow(BaseModel):
    model_config = ConfigDict(frozen=True)

    edge_id: int
    project_id: int
    project_name: str
    rel_path: str
    line: int
    snippet: str | None

    @classmethod
    def from_core(cls, value: _core.EdgeEvidenceRow) -> EdgeEvidenceRow:
        return cls(
            edge_id=value.edge_id,
            project_id=value.project_id,
            project_name=value.project_name,
            rel_path=value.rel_path,
            line=value.line,
            snippet=value.snippet,
        )


class SearchHit(BaseModel):
    model_config = ConfigDict(frozen=True)

    entity_kind: str
    entity_id: int
    name: str
    snippet: str
    rank: float

    @classmethod
    def from_core(cls, value: _core.SearchHit) -> SearchHit:
        return cls(
            entity_kind=value.entity_kind,
            entity_id=value.entity_id,
            name=value.name,
            snippet=value.snippet,
            rank=value.rank,
        )
```

Extend `prograph/__init__.py` re-exports and `__all__` to include `EdgeRow`, `EdgeEvidenceRow`, `SearchHit` alphabetically.

- [ ] **Step 2: Smoke test**

Append to `tests/unit/test_models.py`:
```python
def test_search_hit_round_trip():
    raw = _core.SearchHit(  # type: ignore[call-arg]
        entity_kind="project",
        entity_id=1,
        name="Maestro",
        snippet="DAG [orchestrator]",
        rank=-1.5,
    ) if hasattr(_core.SearchHit, "__init__") else None
    # _core.SearchHit doesn't have #[new] in our spec; just exercise the pydantic shape.
    from prograph.models import SearchHit
    h = SearchHit(entity_kind="project", entity_id=1, name="x", snippet="x", rank=0.0)
    assert h.entity_kind == "project"
```

(Since none of the new pyclasses have `#[new]` constructors, we can't construct them from Python. The full round-trip is exercised via `Store::search_fts` in Task 11's integration tests.)

- [ ] **Step 3: Run + commit**

```sh
uv sync --reinstall-package prograph
uv run pytest tests/unit/test_models.py -v
```
Expected: 11 tests (10 prior + 1 new).

```sh
git add prograph/prograph/models.py prograph/prograph/__init__.py prograph/tests/unit/test_models.py
git commit -m "prograph: M7 pydantic mirrors — EdgeRow / EdgeEvidenceRow / SearchHit"
```

---

## Task 7: `prograph mcp` CLI command + server scaffolding

**Files:**
- Create: `prograph/mcp_server.py`
- Modify: `prograph/cli.py`

The `prograph mcp` command spawns the MCP stdio server. Tools are registered in `mcp_server.py` — each one a thin wrapper over `_core` queries. M7 Task 7 lands the scaffold + one trivial tool (`monorepo_overview` as a smoke test); subsequent tasks add the remaining 7 tools.

- [ ] **Step 1: Write the server entry point**

`prograph/mcp_server.py`:
```python
"""prograph MCP stdio server — exposes the snapshot graph to AI agents."""

from __future__ import annotations

import asyncio
import json
import sys
from pathlib import Path

from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import TextContent, Tool

from prograph import _core
from prograph.models import (
    ContractDescription,
    EdgeEvidenceRow,
    EdgeRow,
    MonorepoOverview,
    ProjectDescription,
    SearchHit,
    SnapshotInfo,
)
from prograph.paths import PrographPaths


def build_server(monorepo_root: Path) -> Server:
    """Construct an MCP server bound to the given monorepo's .prograph/graph.db."""
    paths = PrographPaths(monorepo_root=monorepo_root)
    db_path = str(paths.db_path)

    server = Server("prograph")

    @server.list_tools()
    async def _list_tools() -> list[Tool]:
        return _tool_definitions()

    @server.call_tool()
    async def _call(name: str, arguments: dict | None) -> list[TextContent]:
        args = arguments or {}
        try:
            result = await _dispatch(name, args, db_path)
        except Exception as exc:
            err = {"error": str(exc), "tool": name}
            return [TextContent(type="text", text=json.dumps(err))]
        return [TextContent(type="text", text=json.dumps(result, indent=2))]

    return server


async def _dispatch(name: str, args: dict, db_path: str) -> object:
    """Route MCP tool name → corresponding _core query, return JSON-friendly dict.

    Each branch is a 1-5 line wrapper. Tools are listed in priority order — most
    used at top.
    """
    if name == "monorepo_overview":
        raw = _core.monorepo_overview(db_path)
        if raw is None:
            return {"error": "no snapshot yet — run `prograph index` first"}
        return MonorepoOverview.from_core(raw).model_dump(mode="json")

    raise ValueError(f"unknown tool: {name}")


def _tool_definitions() -> list[Tool]:
    """Return the MCP tool definitions exposed by the server."""
    return [
        Tool(
            name="monorepo_overview",
            description=(
                "High-level summary of the monorepo: list of projects, list of contracts, "
                "edge counts, last 10 changelog entries. The 'hello world' tool for "
                "an AI agent entering a new monorepo."
            ),
            inputSchema={"type": "object", "properties": {}, "required": []},
        ),
        # M7 Tasks 8-13 add the remaining tools.
    ]


async def serve(monorepo_root: Path) -> None:
    """Run the MCP stdio server until the client disconnects."""
    server = build_server(monorepo_root)
    async with stdio_server() as (read_stream, write_stream):
        await server.run(read_stream, write_stream, server.create_initialization_options())


def main(monorepo_root: Path) -> None:
    """Entry point called from the `prograph mcp` CLI command."""
    asyncio.run(serve(monorepo_root))


if __name__ == "__main__":
    if len(sys.argv) > 1:
        main(Path(sys.argv[1]).resolve())
    else:
        main(Path.cwd())
```

- [ ] **Step 2: Wire the CLI**

In `prograph/cli.py`, add (alongside the existing `init`, `index`, `status`, `export-md` commands):
```python
@app.command()
def mcp(
    monorepo: Path = typer.Option(  # noqa: B008
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
) -> None:
    """Run the MCP stdio server. Communicates with the AI client via stdin/stdout."""
    from prograph.mcp_server import main as mcp_main

    root = _resolve_monorepo(monorepo)
    paths = PrographPaths(monorepo_root=root)
    if not paths.is_initialized() or not paths.db_path.exists():
        err_console.print(
            f"[red]error:[/red] no snapshot at {paths.db_path}. "
            "Run `prograph init && prograph index` first."
        )
        raise typer.Exit(code=1)

    mcp_main(root)
```

- [ ] **Step 3: Smoke test via direct invocation**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run python -c "
from prograph.mcp_server import build_server
from pathlib import Path
import sys
# This will not actually serve — just confirm build_server doesn't crash.
build_server(Path('/tmp/nonexistent'))
print('OK')
"
```
Expected: prints OK without exception (build_server doesn't validate paths — that happens at serve time).

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph/mcp_server.py prograph/prograph/cli.py
git commit -m "prograph: M7 'prograph mcp' command + MCP stdio server scaffold (monorepo_overview tool)"
```

---

## Task 8: Tool — `list_projects`

**Files:**
- Modify: `prograph/mcp_server.py`

`list_projects` filters the `MonorepoOverview.projects` list by `kind`.

- [ ] **Step 1: Add the dispatch + definition**

In `prograph/mcp_server.py`, add to `_dispatch`:
```python
    if name == "list_projects":
        kind_filter = args.get("kind")
        raw = _core.monorepo_overview(db_path)
        if raw is None:
            return []
        ov = MonorepoOverview.from_core(raw)
        projects = ov.projects
        if kind_filter:
            projects = [p for p in projects if p.kind == kind_filter]
        return [p.model_dump(mode="json") for p in projects]
```

In `_tool_definitions`, append:
```python
        Tool(
            name="list_projects",
            description=(
                "List discovered projects. Optionally filter by kind. "
                "Returns minimal summaries (name, slug, kind)."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["python", "rust", "js", "docs", "mixed"],
                        "description": "Optional kind filter.",
                    }
                },
                "required": [],
            },
        ),
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/mcp_server.py
git commit -m "prograph: M7 MCP tool — list_projects"
```

---

## Task 9: Tool — `describe_project`

**Files:**
- Modify: `prograph/mcp_server.py`

`describe_project` takes a project name (not id — MCP clients have human-readable names). Resolve via `_core.project_by_name`, then call `_core.describe_project`.

- [ ] **Step 1: Add the dispatch branch**

```python
    if name == "describe_project":
        project_name = args.get("name")
        if not project_name or not isinstance(project_name, str):
            return {"error": "missing required arg 'name'"}
        pid = _core.project_by_name(db_path, project_name)
        if pid is None:
            return {"error": f"project not found: {project_name}"}
        raw = _core.describe_project(db_path, pid)
        if raw is None:
            return {"error": f"project_id {pid} not in latest snapshot"}
        return ProjectDescription.from_core(raw).model_dump(mode="json")
```

In `_tool_definitions`, append:
```python
        Tool(
            name="describe_project",
            description=(
                "Full description of one project: manifest, MCP tools exposed, contracts "
                "declared, outbound and inbound edges, last 5 recent changes. "
                "Use after `list_projects` to drill into a specific project."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Project name as shown by `list_projects`.",
                    }
                },
                "required": ["name"],
            },
        ),
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/mcp_server.py
git commit -m "prograph: M7 MCP tool — describe_project (by name)"
```

---

## Task 10: Tool — `find_edges`

**Files:**
- Modify: `prograph/mcp_server.py`

- [ ] **Step 1: Add the dispatch**

```python
    if name == "find_edges":
        rows = _core.find_edges_filtered(
            db_path,
            args.get("from"),
            args.get("to"),
            args.get("kind"),
            args.get("since"),
        )
        return [EdgeRow.from_core(r).model_dump(mode="json") for r in rows]
```

In `_tool_definitions`:
```python
        Tool(
            name="find_edges",
            description=(
                "Query the edge graph with optional filters. All four filters are AND'ed. "
                "Returns full edge rows with from/to names and attrs."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "from": {"type": "string", "description": "Source project name."},
                    "to": {"type": "string", "description": "Target project name OR contract declared_id."},
                    "kind": {
                        "type": "string",
                        "enum": ["package_dep", "mcp_call", "contract_link"],
                    },
                    "since": {
                        "type": "integer",
                        "description": "Only edges first seen at or after this snapshot id.",
                    },
                },
                "required": [],
            },
        ),
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/mcp_server.py
git commit -m "prograph: M7 MCP tool — find_edges (from/to/kind/since filters)"
```

---

## Task 11: Tool — `edge_evidence`

**Files:**
- Modify: `prograph/mcp_server.py`

- [ ] **Step 1: Add the dispatch**

```python
    if name == "edge_evidence":
        edge_id = args.get("edge_id")
        if edge_id is None or not isinstance(edge_id, int):
            return {"error": "missing required int arg 'edge_id'"}
        rows = _core.edge_evidence_for(db_path, edge_id)
        return [EdgeEvidenceRow.from_core(r).model_dump(mode="json") for r in rows]
```

In `_tool_definitions`:
```python
        Tool(
            name="edge_evidence",
            description=(
                "Source-line locations that justify an edge. M7 returns evidence "
                "only for mcp_call edges (file:line of every call site); other edge "
                "kinds return empty until evidence persistence lands in M8+."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "edge_id": {
                        "type": "integer",
                        "description": "Edge id from `find_edges`.",
                    }
                },
                "required": ["edge_id"],
            },
        ),
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/mcp_server.py
git commit -m "prograph: M7 MCP tool — edge_evidence"
```

---

## Task 12: Tool — `changelog`

**Files:**
- Modify: `prograph/mcp_server.py`

- [ ] **Step 1: Add the dispatch**

```python
    if name == "changelog":
        from prograph.models import ChangeEvent
        events = _core.changelog_paginated(
            db_path,
            args.get("since"),
            args.get("entity_kind"),
            args.get("limit", 50),
        )
        return [ChangeEvent.from_core(e).model_dump(mode="json") for e in events]
```

In `_tool_definitions`:
```python
        Tool(
            name="changelog",
            description=(
                "Paginated change history. Filter by `since` (snapshot id), "
                "`entity_kind` (project/edge/contract), and `limit` (default 50)."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "since": {
                        "type": "integer",
                        "description": "Only events at this snapshot id or later.",
                    },
                    "entity_kind": {
                        "type": "string",
                        "enum": ["project", "edge", "contract"],
                    },
                    "limit": {"type": "integer", "default": 50, "minimum": 1, "maximum": 500},
                },
                "required": [],
            },
        ),
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/mcp_server.py
git commit -m "prograph: M7 MCP tool — changelog"
```

---

## Task 13: Tool — `search` + `snapshot_info`

**Files:**
- Modify: `prograph/mcp_server.py`

- [ ] **Step 1: Add dispatch for both**

```python
    if name == "search":
        query = args.get("q")
        if not query or not isinstance(query, str):
            return {"error": "missing required string arg 'q'"}
        kinds = args.get("kinds")
        if kinds is not None and not isinstance(kinds, list):
            return {"error": "'kinds' must be a list of strings"}
        limit = args.get("limit", 20)
        hits = _core.search_fts(db_path, query, kinds, limit)
        return [SearchHit.from_core(h).model_dump(mode="json") for h in hits]

    if name == "snapshot_info":
        snap_id = args.get("id")
        if snap_id is None:
            raw = _core.latest_snapshot_info(db_path)
        else:
            raw = _core.snapshot_by_id(db_path, snap_id)
        if raw is None:
            return {"error": "no snapshot found"}
        return SnapshotInfo.from_core(raw).model_dump(mode="json")
```

In `_tool_definitions`:
```python
        Tool(
            name="search",
            description=(
                "Full-text search over project + contract names and attributes. "
                "Returns hits with FTS snippet (matched terms wrapped in []) and BM25 rank."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "q": {"type": "string", "description": "FTS query."},
                    "kinds": {
                        "type": "array",
                        "items": {"type": "string", "enum": ["project", "contract"]},
                    },
                    "limit": {"type": "integer", "default": 20, "minimum": 1, "maximum": 100},
                },
                "required": ["q"],
            },
        ),
        Tool(
            name="snapshot_info",
            description=(
                "Metadata for a snapshot: ts, monorepo_root, git_commit, prograph_version, "
                "counts of projects/edges/changes. Defaults to the latest snapshot when "
                "`id` is omitted."
            ),
            inputSchema={
                "type": "object",
                "properties": {"id": {"type": "integer", "description": "Snapshot id."}},
                "required": [],
            },
        ),
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/mcp_server.py
git commit -m "prograph: M7 MCP tools — search + snapshot_info"
```

---

## Task 14: MCP integration tests (async + stdio subprocess)

**Files:**
- Create: `tests/integration/test_cli_mcp.py`

Spawn `prograph mcp` as a subprocess, connect via the MCP SDK's stdio client, exercise each tool against the `monorepo_mcp` fixture.

- [ ] **Step 1: Mark async tests**

Append to `pyproject.toml`'s `[tool.pytest.ini_options]`:
```toml
asyncio_mode = "auto"
```

This lets `async def test_...` functions run without explicit `@pytest_asyncio.fixture` decoration.

- [ ] **Step 2: Write the test file**

`tests/integration/test_cli_mcp.py`:
```python
"""MCP stdio server integration tests."""

import json
import shutil
import sys
from pathlib import Path

import pytest
from mcp import ClientSession
from mcp.client.stdio import StdioServerParameters, stdio_client
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_mcp"


@pytest.fixture
def indexed_mcp_fixture(tmp_path: Path) -> Path:
    """Copy monorepo_mcp into tmp_path and run init + index."""
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


async def _open_session(monorepo: Path):
    """Spawn `prograph mcp` against the indexed monorepo and return a ClientSession."""
    params = StdioServerParameters(
        command=sys.executable,
        args=["-m", "prograph.mcp_server", str(monorepo)],
    )
    return stdio_client(params)


async def test_mcp_list_tools_returns_eight(indexed_mcp_fixture: Path):
    async with await _open_session(indexed_mcp_fixture) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            tools = await session.list_tools()
            names = {t.name for t in tools.tools}
            expected = {
                "monorepo_overview",
                "list_projects",
                "describe_project",
                "find_edges",
                "edge_evidence",
                "changelog",
                "search",
                "snapshot_info",
            }
            assert expected == names, f"expected {expected}, got {names}"


async def test_mcp_monorepo_overview_returns_projects(indexed_mcp_fixture: Path):
    async with await _open_session(indexed_mcp_fixture) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool("monorepo_overview", arguments={})
            payload = json.loads(result.content[0].text)
            assert payload["n_projects"] == 6
            project_names = {p["name"] for p in payload["projects"]}
            assert "py_server" in project_names


async def test_mcp_list_projects_filter_by_kind(indexed_mcp_fixture: Path):
    async with await _open_session(indexed_mcp_fixture) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool("list_projects", arguments={"kind": "rust"})
            payload = json.loads(result.content[0].text)
            assert len(payload) == 1
            assert payload[0]["name"] == "rust_server"


async def test_mcp_describe_project(indexed_mcp_fixture: Path):
    async with await _open_session(indexed_mcp_fixture) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool(
                "describe_project", arguments={"name": "py_client"}
            )
            payload = json.loads(result.content[0].text)
            assert payload["name"] == "py_client"
            assert len(payload["outbound"]) >= 1  # py_client → py_server via mcp_call


async def test_mcp_find_edges_kind_filter(indexed_mcp_fixture: Path):
    async with await _open_session(indexed_mcp_fixture) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool("find_edges", arguments={"kind": "mcp_call"})
            payload = json.loads(result.content[0].text)
            assert len(payload) == 3
            assert all(e["kind"] == "mcp_call" for e in payload)


async def test_mcp_edge_evidence_for_mcp_call(indexed_mcp_fixture: Path):
    async with await _open_session(indexed_mcp_fixture) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            edges = json.loads(
                (await session.call_tool("find_edges", arguments={"kind": "mcp_call"})).content[0].text
            )
            mcp_edge_id = edges[0]["id"]
            result = await session.call_tool(
                "edge_evidence", arguments={"edge_id": mcp_edge_id}
            )
            evidence = json.loads(result.content[0].text)
            assert len(evidence) >= 1
            assert "rel_path" in evidence[0]
            assert "line" in evidence[0]


async def test_mcp_changelog(indexed_mcp_fixture: Path):
    async with await _open_session(indexed_mcp_fixture) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool("changelog", arguments={"limit": 5})
            payload = json.loads(result.content[0].text)
            assert isinstance(payload, list)
            assert len(payload) <= 5


async def test_mcp_search_finds_project(indexed_mcp_fixture: Path):
    async with await _open_session(indexed_mcp_fixture) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool("search", arguments={"q": "py_server"})
            payload = json.loads(result.content[0].text)
            assert any(h["name"] == "py_server" for h in payload)


async def test_mcp_snapshot_info_latest(indexed_mcp_fixture: Path):
    async with await _open_session(indexed_mcp_fixture) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool("snapshot_info", arguments={})
            payload = json.loads(result.content[0].text)
            assert payload["id"] == 1
            assert payload["n_projects"] == 6


async def test_mcp_unknown_tool_returns_error(indexed_mcp_fixture: Path):
    async with await _open_session(indexed_mcp_fixture) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool("nonexistent_tool", arguments={})
            payload = json.loads(result.content[0].text)
            assert "error" in payload
            assert "unknown tool" in payload["error"]
```

- [ ] **Step 3: Run**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv sync --reinstall-package prograph
uv run pytest tests/integration/test_cli_mcp.py -v
```
Expected: 10 passed.

Full suite:
```sh
uv run pytest -v
```
Expected: 99+ tests pass.

- [ ] **Step 4: Commit**

```sh
git add prograph/tests/integration/test_cli_mcp.py prograph/pyproject.toml
git commit -m "prograph: M7 MCP integration tests — 10 tests covering all 8 tools via stdio client"
```

---

## Task 15: Configurable detection patterns

**Files:**
- Modify: `prograph-core/src/parsers/python.rs`
- Modify: `prograph-core/src/parsers/rust.rs`
- Modify: `prograph/paths.py`
- Modify: `prograph/cli.py` (init command — create the patterns dir)

Allow users to drop `.scm` files into `.prograph/mcp_patterns/` to extend the bundled tree-sitter queries. Particularly useful for arbiter-style custom MCP idioms that don't match our heuristic patterns.

- [ ] **Step 1: Add `mcp_patterns_dir` to `PrographPaths`**

In `prograph/paths.py`, add:
```python
    @property
    def mcp_patterns_dir(self) -> Path:
        return self.prograph_dir / "mcp_patterns"
```

And update `ensure_dirs` to also create it:
```python
    def ensure_dirs(self) -> None:
        # ... existing dirs ...
        self.mcp_patterns_dir.mkdir(parents=True, exist_ok=True)
```

- [ ] **Step 2: Read pattern overrides in Rust parsers**

In `prograph-core/src/parsers/python.rs`, modify `scan_python_source` to look for an additional query file:

Replace the line where `query_src` is built from `include_str!`:
```rust
    let bundled = include_str!("../ts_queries/python_mcp.scm");
    let override_path = monorepo_root_from_project(project_root)
        .join(".prograph/mcp_patterns/python.scm");
    let override_src = std::fs::read_to_string(&override_path).ok();
    let combined = match override_src {
        Some(extra) => format!("{}\n\n; --- user override ---\n{}", bundled, extra),
        None => bundled.to_string(),
    };

    let query = match Query::new(&language, &combined) {
        Ok(q) => q,
        Err(e) => {
            return (
                vec![], vec![],
                vec![crate::facts::ParseWarning {
                    rel_path: ".prograph/mcp_patterns/python.scm".into(),
                    message: format!("failed to compile combined tree-sitter query: {}", e),
                }],
            );
        }
    };
```

The helper `monorepo_root_from_project` walks up from the project root looking for `.prograph/`:
```rust
fn monorepo_root_from_project(project_root: &std::path::Path) -> std::path::PathBuf {
    let mut cur = project_root.to_path_buf();
    loop {
        if cur.join(".prograph").is_dir() {
            return cur;
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => return project_root.to_path_buf(),
        }
    }
}
```

Place `monorepo_root_from_project` near the top of `python.rs` so the override loader can use it.

- [ ] **Step 3: Mirror for Rust parser**

Apply the same change to `prograph-core/src/parsers/rust.rs`: replace `let query_src = include_str!("../ts_queries/rust_mcp.scm");` with the combined bundled+override loader, using `.prograph/mcp_patterns/rust.scm` as the override path. Add the same `monorepo_root_from_project` helper if not already shared.

(Consider moving `monorepo_root_from_project` to `parsers/mod.rs` for reuse — `pub(super) fn monorepo_root_from_project(...)`.)

- [ ] **Step 4: Update `prograph init` to create the dir + sample file**

In `prograph/cli.py`'s `init` command, after the existing `paths.ensure_dirs()` call, add:
```python
    # M7: create mcp_patterns dir with a sample (empty) override for documentation.
    sample = paths.mcp_patterns_dir / "README.md"
    if not sample.exists():
        sample.write_text(
            "# MCP detection pattern overrides\n\n"
            "Drop `python.scm` or `rust.scm` files here to extend the bundled\n"
            "tree-sitter queries used by `detectors/mcp`. They are appended to the\n"
            "built-in queries; queries are run with the same capture-name conventions\n"
            "(`tool_name`, `tool_name_literal`, `tool_use_call`, `tool_use_method`).\n"
        )
```

- [ ] **Step 5: Add a Rust test for the override mechanism**

Append to `parsers/python.rs`'s tests:
```rust
    #[test]
    fn loads_mcp_pattern_override_from_monorepo_root() {
        let monorepo = TempDir::new().unwrap();
        // Set up .prograph dir + an override.
        fs::create_dir_all(monorepo.path().join(".prograph/mcp_patterns")).unwrap();
        fs::write(
            monorepo.path().join(".prograph/mcp_patterns/python.scm"),
            // A custom pattern: any call to `.custom_tool("name", ...)` is a tool decl.
            r#"
(call
  function: (attribute attribute: (identifier) @method)
  arguments: (argument_list . (string) @tool_name_literal)
  (#eq? @method "custom_tool")) @tool_decl_custom
"#,
        ).unwrap();

        // Create a project with code that matches the custom pattern.
        let proj = monorepo.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            proj.join("pyproject.toml"),
            r#"[project]
name = "p"
"#,
        ).unwrap();
        fs::write(
            proj.join("server.py"),
            r#"server.custom_tool("decide_v2")"#,
        ).unwrap();

        let out = parse(&proj).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(
            names.contains(&"decide_v2"),
            "expected override pattern to fire, got: {:?}",
            names
        );
    }
```

- [ ] **Step 6: Run + commit**

```sh
cargo test --package prograph-core parsers
```
Expected: ≥33 parsers tests pass.

Full crate + Python:
```sh
cargo test --package prograph-core
uv run pytest -v
```

```sh
git add prograph/prograph-core/src/parsers/{python,rust,mod}.rs prograph/prograph/paths.py prograph/prograph/cli.py
git commit -m "prograph: M7 configurable MCP patterns — .prograph/mcp_patterns/{python,rust}.scm override"
```

---

## Task 16: README + CLAUDE.md + smoke + close

**Files:**
- Modify: `prograph/README.md`
- Modify: `prograph/CLAUDE.md`
- Modify: `tests/integration/test_smoke_real.py`
- Modify: this plan file

- [ ] **Step 1: Extend the real-monorepo smoke**

Append to `tests/integration/test_smoke_real.py`'s existing test body (after the M5 export-md block):
```python
    # M7: also confirm `prograph mcp` can boot against the real monorepo.
    # We don't run a full MCP session here (too slow + flaky) — just confirm the
    # build_server call succeeds.
    from prograph.mcp_server import build_server
    server = build_server(real)
    assert server is not None
```

- [ ] **Step 2: README**

Replace the Status line:
```markdown
**Status:** M7 — MCP stdio server. `prograph mcp` exposes 8 tools (`monorepo_overview`, `list_projects`, `describe_project`, `find_edges`, `edge_evidence`, `changelog`, `search`, `snapshot_info`) over MCP stdio. AI agents (Claude Code, custom skills) consume the graph directly. MCP detection patterns are user-extensible via `.prograph/mcp_patterns/{python,rust}.scm`. Browser UI: M6 (deliberately deferred to land the AI surface first).
```

Add a new "AI agent integration" section:
````markdown
## AI agent integration (MCP)

Configure Claude Code or another MCP client to spawn `prograph mcp` for your monorepo:

```json
{
  "mcpServers": {
    "prograph": {
      "command": "uv",
      "args": ["run", "prograph", "mcp", "--monorepo", "/path/to/monorepo"]
    }
  }
}
```

The 8 tools are:

| Tool | Purpose |
|---|---|
| `monorepo_overview` | Hello-world: list of projects + contracts + recent changes. |
| `list_projects` | Filter projects by kind. |
| `describe_project` | Full project card by name. |
| `find_edges` | Query edges with from/to/kind/since filters. |
| `edge_evidence` | File:line locations of MCP call sites for a given edge. |
| `changelog` | Paginated history of changes. |
| `search` | FTS over project + contract names. |
| `snapshot_info` | Snapshot metadata (latest or by id). |

### Extending MCP detection

Drop a `python.scm` or `rust.scm` into `.prograph/mcp_patterns/` to extend the bundled tree-sitter queries:

```scheme
; arbiter-style: tools registered via .mcp_tool("name", ...)
(call_expression
  function: (field_expression field: (field_identifier) @method)
  arguments: (arguments . (string_literal) @tool_name_literal)
  (#eq? @method "mcp_tool")) @tool_decl_arbiter
```

Required capture names: `tool_name` (identifier capture for decorator-style decls), `tool_name_literal` (string-literal capture for call-style decls and uses), `tool_use_call` (Python) / `tool_use_method` (Rust) marker captures to distinguish use sites from decl sites.
````

- [ ] **Step 3: CLAUDE.md**

Update "Architecture (M5 state)" → "Architecture (M7 state)". Append the MCP server + new query helpers to the components list:

```markdown
## Architecture (M7 state)

Two-layer build:

- **`prograph-core` (Rust crate via PyO3):**
  - `discovery`, `parsers/{python,rust,js,contracts}`, `detectors/{deps,contracts,mcp}`, `diff`, `lock`, `indexer` (M1-M5)
  - `store` — SQLite schema **v5** (adds `search_fts`); 12+ query methods incl. `describe_*`, `monorepo_overview`, `project_by_name`, `snapshot_by_id`, `find_edges_filtered`, `edge_evidence_for`, `search_fts`, `changelog_paginated`
  - `models` — 25+ pyclasses incl. M7 additions `EdgeRow`, `EdgeEvidenceRow`, `SearchHit`
  - `facts` — `Manifest`, `McpToolDecl`, `McpClientUse`, `ContractFile`, `ProjectFacts`
  - `migrations/v1.sql..v5.sql`
- **`prograph` (Python package):**
  - `cli.py` — `init`, `index`, `status`, `export-md`, `mcp`, `--version`
  - `mcp_server.py` — MCP stdio server with 8 tools (M7)
  - `export/` — Markdown rendering (M5)
  - `config.py`, `models.py`, `paths.py`

The Rust↔Python boundary remains data-only.

### MCP detection pattern overrides

`.prograph/mcp_patterns/{python,rust}.scm` files are appended to the bundled tree-sitter queries at parse time. Use them to recognise project-specific MCP idioms without forking the crate.
```

Replace "What is NOT in M5" with:
```markdown
## What is NOT in M7

- Browser UI / REST API — M6 (deferred behind the AI-facing path).
- `edge_evidence` for `package_dep` / `contract_link` — only `mcp_call` evidence is persisted in M7. Backfill is a follow-up.
- HTTP / SSE MCP transport — only stdio in M7.
- Module-level facts (public symbols, internal imports) — orthogonal parser-expansion milestone.
- JS MCP source scanning — no driver in the target monorepo.
```

Add to "Common commands":
```sh
uv run prograph mcp [--monorepo PATH]                # run MCP stdio server
```

- [ ] **Step 4: Run the full local gate**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo fmt --all -- --check && \
cargo clippy --all-targets -- -D warnings && \
cargo test --all-targets && \
uv run ruff check . && \
uv run ruff format --check . && \
uv run pyrefly check 'prograph/**/*.py' 'tests/**/*.py' && \
uv run pytest -v && \
uv run pytest -m realmonorepo -v
```

Expected: every command exits 0. Cargo ≥128; pytest ≥100; realmonorepo 1.

- [ ] **Step 5: Close the DoD boxes**

Mark every `- [ ]` in "Definition of Done (M7)" as `- [x]` with achieved counts.

- [ ] **Step 6: Final commit**

```sh
git add prograph/README.md prograph/CLAUDE.md \
        prograph/tests/integration/test_smoke_real.py \
        prograph/docs/superpowers/plans/2026-05-26-prograph-m7-mcp-server.md
git commit -m "prograph: M7 close — docs updated, full gate green, DoD checked"
```

---

## Definition of Done (M7)

- [x] `cargo test --all-targets` passes (129 tests).
- [x] `uv run pytest -v` passes (100 tests; 1 deselected).
- [x] `uv run pytest -m realmonorepo -v` passes; the real monorepo's MCP server boots without error.
- [x] Schema v5 (`search_fts`) applies cleanly over v4 and the FTS table is populated by the indexer.
- [x] `EdgeCandidate.evidence` carries per-call-site locations for `mcp_call` edges; the indexer persists them into `edge_evidence`.
- [x] `Store::project_by_name`, `Store::snapshot_by_id`, `Store::find_edges_filtered`, `Store::edge_evidence_for`, `Store::search_fts`, `Store::changelog_paginated` all expose via PyO3 and round-trip through pydantic.
- [x] `EntityKind` enum gains `Contract` variant (Rust + pydantic + .pyi).
- [x] `prograph mcp` boots an MCP stdio server and registers 8 tools.
- [x] Each tool runs successfully against the `monorepo_mcp` fixture and returns the expected payload (10 integration tests pass).
- [x] Unknown tool names return `{"error": "unknown tool: ..."}` rather than crashing.
- [x] `.prograph/mcp_patterns/{python,rust}.scm` overrides are read by parsers at scan time and appended to the bundled queries.
- [x] `prograph init` creates `.prograph/mcp_patterns/` with a README explaining the override mechanism.
- [x] CI workflow continues to pass with no changes required.
- [x] All commits follow the `prograph: M7 ...` prefix convention.

## What is NOT done in M7 (handled in subsequent milestones)

- **M6** — Browser UI (FastAPI + d3/cytoscape) + REST API.
- **M8** — `edge_evidence` backfill for `package_dep` (manifest:1) and `contract_link` (contract file:1); module-level facts (public symbols / internal imports); HTTP/SSE MCP transport; JS MCP detection; performance baselines; workspace auto-discovery.
