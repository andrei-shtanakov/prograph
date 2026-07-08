# prograph M5 — Markdown Exporter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After M5, every `prograph index` run produces (optionally) a tree of per-project, per-contract, and one root `index.md` file under `.prograph/`. The files are **byte-stable** — running `prograph export-md` twice on the same snapshot yields identical bytes — and **Obsidian-friendly** — YAML frontmatter + `[[wiki-links]]` + sorted sections. They become the human-facing artefact: when committed to git, a PR that adds a cross-project edge produces a readable MD diff in the same PR. AI agents read the MD instead of opening SQLite.

**Architecture:**
- **Rendering happens in Python**, not Rust. The Rust core owns data + queries; Python owns I/O + templates. This matches the spec §4.2 ("Python wrapper handles MD export") and keeps the Rust↔Python boundary clean: Python never touches SQLite directly — it consumes typed `Description` pyclasses returned by new `Store::describe_*` methods.
- **Three new Rust-side query helpers**: `Store::describe_project`, `Store::describe_contract`, `Store::monorepo_overview`. Each returns an aggregation pyclass that bundles everything the renderer needs for one MD file. The renderer makes one call per file — no chatty I/O.
- **Schema v4 — additive**: persist `McpToolDecl` facts in a new `mcp_tool_decls` table so the "MCP tools exposed" MD section is filled from the DB (not re-scanned at render time). Identity per `(project_id, tool_name)`; temporal first_seen/last_seen like `contract_files`. No `change_log` entries — sub-data of project, no CHECK widening needed.
- **`prograph` package gains a new sub-package `prograph.export`** with three focused renderers (`render_project`, `render_contract`, `render_index`) + an `intro` helper that extracts the first paragraph from `README.md` / `CLAUDE.md` / `TODO.md`.
- **`prograph index` gains `--export-md` flag** and respects a new `[output] auto_export = true` in `.prograph/config.toml`. A new standalone `prograph export-md` command re-renders from the current snapshot without re-indexing (useful when fixing renderer templates).
- **Determinism is enforced at three layers**: SQL `ORDER BY`, Rust sort before returning pyclass lists, Python sort before joining strings into the final MD. Frontmatter keys are emitted in alphabetical order; line endings are `\n`; single trailing newline; no trailing whitespace per line.
- **Golden tests are the M5 backbone**. Each of the three checked-in fixtures (`monorepo_full`, `monorepo_multilang`, `monorepo_mcp`) gains a `golden/` directory of expected MD output. The pytest helper compares byte-for-byte; `PROGRAPH_UPDATE_GOLDEN=1` env regenerates.

**Tech Stack additions (M5 only):**
- No new Rust deps. SQL aggregation uses rusqlite primitives already on hand.
- No new Python deps for rendering — only stdlib `pathlib`, `os`, `re` and existing `pydantic` + `rich` + `typer`.

**Spec reference:** `docs/superpowers/specs/2026-05-25-prograph-design.md` §5.3 — MD export structure (per-project frontmatter, sections, wiki-links). §6 phase 6 ("Optional outputs — if `--export-md` or `config.auto_export`, render projects/*.md, contracts/*.md, index.md").

**Baseline:** Branch off `main` at the M4 close commit `9f747e1`. 112 cargo + 45 pytest + 1 realmonorepo passing; CI green; `prograph index` produces three edge kinds (`package_dep`, `mcp_call`, `contract_link`) plus a `contracts` node table. Real `all_ai_orchestrators/` produces 2 `package_dep` + 2 `contract_link` edges; MCP patterns don't currently match arbiter's idioms (soft-warned).

**M5 explicitly out of scope (deferred to later):**
- **Module-level facts** (public Python symbols, internal imports, public Rust crate items) — they'd live in the "Public surface" MD section alongside MCP tools. Deferred to a later parser-expansion milestone.
- **Browser UI / REST** — M6.
- **MCP stdio server for AI agents** — M7.
- **Configurable detection patterns** for arbiter-style MCP idioms — M7.
- **Workspace auto-discovery, HTTP/REST edges, JS MCP** — M8+.
- **`--update-snapshots` for live golden refresh** is a pytest env var, not a CLI flag. Generating new golden requires running the test suite with `PROGRAPH_UPDATE_GOLDEN=1`, not invoking `prograph` directly.

---

## File Structure (created/modified in M5)

```
prograph/
├── prograph-core/
│   ├── src/
│   │   ├── lib.rs                                   # MODIFY — register new modules + exports
│   │   ├── models.rs                                # MODIFY — add 6 aggregation pyclasses
│   │   ├── store.rs                                 # MODIFY — describe_project / describe_contract /
│   │   │                                            #   monorepo_overview + alive_mcp_tool_decls +
│   │   │                                            #   SnapshotWriter mcp_tool_decl methods
│   │   ├── indexer.rs                               # MODIFY — persist McpToolDecls each snapshot
│   │   └── migrations/
│   │       └── v4.sql                               # NEW — mcp_tool_decls table
├── prograph/
│   ├── _core.pyi                                    # MODIFY — stubs for 6 new pyclasses + 3 fns
│   ├── __init__.py                                  # MODIFY — re-export new pydantic types
│   ├── models.py                                    # MODIFY — pydantic mirrors for aggregation
│   ├── paths.py                                     # MODIFY — index_md_path, config_output_section
│   ├── cli.py                                       # MODIFY — index --export-md, export-md cmd
│   └── export/                                      # NEW sub-package
│       ├── __init__.py
│       ├── intro.py                                 # NEW — first-paragraph extraction
│       ├── render.py                                # NEW — main render functions
│       └── slug.py                                  # NEW — filename slugification
├── tests/
│   ├── fixtures/
│   │   ├── monorepo_full/golden/                    # NEW — expected MD output
│   │   ├── monorepo_multilang/golden/               # NEW
│   │   └── monorepo_mcp/golden/                     # NEW
│   ├── conftest.py                                  # MODIFY — add assert_md_dir_matches helper
│   ├── unit/
│   │   ├── test_export_intro.py                     # NEW
│   │   ├── test_export_render.py                    # NEW
│   │   └── test_export_slug.py                      # NEW
│   └── integration/
│       ├── test_cli_export_md.py                    # NEW
│       └── test_smoke_real.py                       # MODIFY — also run export-md
```

---

## Task 1: Schema v4 — `mcp_tool_decls` table

**Files:**
- Create: `prograph-core/src/migrations/v4.sql`
- Modify: `prograph-core/src/store.rs`

The MCP tool declarations parser-side facts now persist alongside `contract_files`. Identity is `(project_id, tool_name)` — when a project removes a `@server.tool()` decorator, the row's `last_seen` stops advancing. No `change_log` entries are written (sub-data of project; the resulting `mcp_call` edge removal is what surfaces in the log).

- [ ] **Step 1: Write the v4 schema**

`prograph-core/src/migrations/v4.sql`:
```sql
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
```

- [ ] **Step 2: Register the migration**

In `prograph-core/src/store.rs`, find `MIGRATIONS` and append v4:
```rust
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/v1.sql")),
    (2, include_str!("migrations/v2.sql")),
    (3, include_str!("migrations/v3.sql")),
    (4, include_str!("migrations/v4.sql")),
];
```

- [ ] **Step 3: Add `alive_mcp_tool_decls` + writer methods**

Append to `impl Store`:
```rust
    /// Return alive MCP tool decls keyed by "{project_id}|{tool_name}" → (rel_path, line).
    pub fn alive_mcp_tool_decls(
        &self,
    ) -> Result<std::collections::HashMap<String, (String, i64)>> {
        let mut out = std::collections::HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT project_id, tool_name, rel_path, line FROM mcp_tool_decls
             WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        for row in rows {
            let (pid, name, path, line) = row?;
            let key = format!("{}|{}", pid, name);
            out.insert(key, (path, line));
        }
        Ok(out)
    }
```

Append to `impl<'a> SnapshotWriter<'a>`:
```rust
    pub fn insert_mcp_tool_decl(
        &self,
        project_id: i64,
        tool_name: &str,
        rel_path: &str,
        line: i64,
        snapshot_id: i64,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO mcp_tool_decls
             (project_id, tool_name, rel_path, line, first_seen, last_seen)
             VALUES (?, ?, ?, ?,
                     COALESCE((SELECT first_seen FROM mcp_tool_decls WHERE project_id=? AND tool_name=?), ?),
                     ?)",
            rusqlite::params![
                project_id, tool_name, rel_path, line,
                project_id, tool_name, snapshot_id,
                snapshot_id
            ],
        )?;
        Ok(())
    }

    pub fn touch_mcp_tool_decl(
        &self,
        project_id: i64,
        tool_name: &str,
        snapshot_id: i64,
    ) -> Result<()> {
        self.tx.execute(
            "UPDATE mcp_tool_decls SET last_seen = ?
             WHERE project_id = ? AND tool_name = ?",
            rusqlite::params![snapshot_id, project_id, tool_name],
        )?;
        Ok(())
    }
```

`insert_mcp_tool_decl` uses `INSERT OR REPLACE` with a COALESCE on `first_seen` so re-running on the same (project, tool) preserves the original `first_seen` timestamp while updating `last_seen` + position fields (line numbers may shift when files are edited).

- [ ] **Step 4: Add tests**

Append to `store.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn schema_v4_creates_mcp_tool_decls_table() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        let names: Vec<String> = store
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"mcp_tool_decls".to_string()));
        assert_eq!(store.schema_version().unwrap(), 4);
    }

    #[test]
    fn mcp_tool_decl_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid = writer.insert_project(snap, "srv", "./srv", "python", "{}").unwrap();
        writer
            .insert_mcp_tool_decl(pid, "decide", "src/server.py", 42, snap)
            .unwrap();
        writer.commit().unwrap();

        let alive = store.alive_mcp_tool_decls().unwrap();
        let key = format!("{}|decide", pid);
        assert!(alive.contains_key(&key));
        let (path, line) = alive[&key].clone();
        assert_eq!(path, "src/server.py");
        assert_eq!(line, 42);
    }

    #[test]
    fn touch_mcp_tool_decl_preserves_first_seen() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        // Snapshot 1: insert.
        let pid;
        {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts1", "/m", None, "0.1.0").unwrap();
            pid = writer.insert_project(snap, "srv", "./srv", "python", "{}").unwrap();
            writer
                .insert_mcp_tool_decl(pid, "decide", "src/server.py", 42, snap)
                .unwrap();
            writer.commit().unwrap();
        }

        // Snapshot 2: re-insert with same identity, different line.
        {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts2", "/m", None, "0.1.0").unwrap();
            writer
                .insert_mcp_tool_decl(pid, "decide", "src/server.py", 99, snap)
                .unwrap();
            writer.commit().unwrap();
        }

        let row: (i64, i64) = store
            .connection()
            .query_row(
                "SELECT first_seen, last_seen FROM mcp_tool_decls
                 WHERE project_id = ? AND tool_name = 'decide'",
                rusqlite::params![pid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row.0, 1, "first_seen must remain snapshot 1");
        assert_eq!(row.1, 2, "last_seen must advance to snapshot 2");
    }
```

- [ ] **Step 5: Run tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core store
```
Expected: 18 store tests (15 prior + 3 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 115 tests (112 prior + 3 new).

- [ ] **Step 6: Commit**

```sh
git add prograph/prograph-core/src/migrations/v4.sql prograph/prograph-core/src/store.rs
git commit -m "prograph: M5 schema v4 — mcp_tool_decls table + Store::alive_mcp_tool_decls / SnapshotWriter helpers"
```

---

## Task 2: Indexer — persist McpToolDecls each snapshot

**Files:**
- Modify: `prograph-core/src/indexer.rs`

The indexer already has the `mcp_decls` facts collected from each project (Tasks 6-7 of M4). M5 routes them through `SnapshotWriter::insert_mcp_tool_decl` during the persist phase. No diff is computed for tool decls — they're "current state" sub-data of each project, regenerated each snapshot. `insert_mcp_tool_decl` is idempotent (INSERT OR REPLACE preserving first_seen).

- [ ] **Step 1: Persist tool decls inside the existing per-project loop**

Find the project persist loop in `prograph-core/src/indexer.rs`. After the project is inserted/touched and `new_project_ids` updated, iterate the project's `mcp_decls` and persist:

After the existing match's success branches (Added / Unchanged / AttrsChanged), but BEFORE moving to the next iteration, add:

```rust
        // Persist any MCP tool decls this project declared (M5).
        // Only persist when the project itself is present in the new snapshot
        // (i.e., NOT in the Removed branch — that's handled by leaving the row
        // unadvanced so its last_seen reflects the last snapshot it was alive).
        if let Some(&pid) = new_project_ids.get(key) {
            let fact = facts.iter().find(|f| &f.project_root == key);
            if let Some(fact) = fact {
                for decl in &fact.mcp_decls {
                    writer.insert_mcp_tool_decl(
                        pid,
                        &decl.tool_name,
                        &decl.rel_path,
                        decl.line as i64,
                        snap_id,
                    )?;
                }
            }
        }
```

Place the block immediately after the closing brace of the project diff `match` block.

- [ ] **Step 2: Add an inline test**

Append to `indexer.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn mcp_tool_decls_persist_across_snapshots() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();
        fs::create_dir_all(dir.path().join("server")).unwrap();
        fs::write(
            dir.path().join("server/pyproject.toml"),
            r#"[project]
name = "srv"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("server/server.py"),
            r#"@server.tool()
def hello():
    return "world"
"#,
        )
        .unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        let alive = store.alive_mcp_tool_decls().unwrap();
        assert!(
            alive.values().any(|(path, _)| path == "server.py"),
            "expected hello tool decl persisted, got: {:?}",
            alive
        );
    }
```

- [ ] **Step 3: Run tests**

```sh
cargo test --package prograph-core indexer
```
Expected: 8 indexer tests (7 prior + 1 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 116 tests.

Verify clean.

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph-core/src/indexer.rs
git commit -m "prograph: M5 indexer — persist McpToolDecls into mcp_tool_decls table"
```

---

## Task 3: Aggregation pyclasses (`ProjectDescription`, `OutboundEdge`, etc.)

**Files:**
- Modify: `prograph-core/src/models.rs`
- Modify: `prograph-core/src/lib.rs`
- Modify: `prograph/_core.pyi`

Six new pyclasses bundle the data the renderer needs. They're read-only views; only `Store::describe_*` constructs them.

- [ ] **Step 1: Add pyclasses to `models.rs`**

Append to `prograph-core/src/models.rs`:
```rust
/// A single outbound edge as seen from a source project's MD card. The target
/// is denormalised — `target_name` is the project name OR the contract declared_id
/// (or content-hash prefix if no declared_id).
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct OutboundEdge {
    pub kind: String,           // "package_dep" | "mcp_call" | "contract_link"
    pub target_kind: String,    // "project" | "contract"
    pub target_name: String,
    pub target_slug: String,    // filename slug for wiki-links
    pub attrs_json: String,
}

#[pymethods]
impl OutboundEdge {
    fn __repr__(&self) -> String {
        format!("OutboundEdge({} → {}:{})", self.kind, self.target_kind, self.target_name)
    }
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct InboundEdge {
    pub kind: String,
    pub source_name: String,
    pub source_slug: String,
    pub attrs_json: String,
}

#[pymethods]
impl InboundEdge {
    fn __repr__(&self) -> String {
        format!("InboundEdge({} ← {})", self.kind, self.source_name)
    }
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct McpToolDeclRow {
    pub tool_name: String,
    pub rel_path: String,
    pub line: i64,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct ContractFileRow {
    pub contract_declared_id: Option<String>,
    pub contract_slug: String,
    pub contract_kind: String,
    pub rel_path: String,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct RecentChangeRow {
    pub snapshot_id: i64,
    pub ts: String,
    pub change: String,        // "added" | "removed" | "attrs_changed"
    pub summary: String,        // human-readable one-liner
}

/// Bundle of everything the renderer needs for one project's MD file.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct ProjectDescription {
    pub project_id: i64,
    pub name: String,
    pub slug: String,           // filesystem-safe filename without extension
    pub kind: String,
    pub root_path: String,
    pub attrs_json: String,     // manifest + declared name + version (raw JSON for renderer)
    pub snapshot_id: i64,
    pub snapshot_ts: String,
    pub mcp_decls: Vec<McpToolDeclRow>,
    pub contract_files: Vec<ContractFileRow>,
    pub outbound: Vec<OutboundEdge>,
    pub inbound: Vec<InboundEdge>,
    pub recent_changes: Vec<RecentChangeRow>,
}

#[pymethods]
impl ProjectDescription {
    fn __repr__(&self) -> String {
        format!(
            "ProjectDescription(name={}, kind={}, outbound={}, inbound={})",
            self.name, self.kind, self.outbound.len(), self.inbound.len()
        )
    }
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct ContractOwner {
    pub project_name: String,
    pub project_slug: String,
    pub rel_path: String,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct ContractDescription {
    pub contract_id: i64,
    pub declared_id: Option<String>,
    pub slug: String,
    pub kind: String,
    pub content_hash: String,
    pub snapshot_id: i64,
    pub snapshot_ts: String,
    pub owners: Vec<ContractOwner>,
    pub recent_changes: Vec<RecentChangeRow>,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct ProjectSummary {
    pub name: String,
    pub slug: String,
    pub kind: String,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct ContractSummary {
    pub slug: String,
    pub declared_id: Option<String>,
    pub kind: String,
    pub n_owners: i64,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct MonorepoOverview {
    pub monorepo_root: String,
    pub snapshot_id: i64,
    pub snapshot_ts: String,
    pub n_projects: i64,
    pub n_contracts: i64,
    pub n_edges: i64,
    pub projects: Vec<ProjectSummary>,
    pub contracts: Vec<ContractSummary>,
    pub recent_changes: Vec<RecentChangeRow>,
}
```

- [ ] **Step 2: Extend exports + pymodule registration**

In `prograph-core/src/lib.rs`, extend `pub use models::{...}`:
```rust
pub use models::{
    ChangeEvent, ChangeKind, Contract, ContractDescription, ContractFileRow, ContractOwner,
    ContractSummary, Edge, EdgeKind, EntityKind, InboundEdge, IndexSummary, McpToolDeclRow,
    MonorepoOverview, NodeKind, OutboundEdge, ProjectCandidate, ProjectDescription,
    ProjectKind, ProjectSummary, RecentChangeRow, SnapshotInfo,
};
```

And inside `#[pymodule]`, append `m.add_class::<...>()` calls for all 10 new types:
```rust
    m.add_class::<OutboundEdge>()?;
    m.add_class::<InboundEdge>()?;
    m.add_class::<McpToolDeclRow>()?;
    m.add_class::<ContractFileRow>()?;
    m.add_class::<RecentChangeRow>()?;
    m.add_class::<ProjectDescription>()?;
    m.add_class::<ContractOwner>()?;
    m.add_class::<ContractDescription>()?;
    m.add_class::<ProjectSummary>()?;
    m.add_class::<ContractSummary>()?;
    m.add_class::<MonorepoOverview>()?;
```

- [ ] **Step 3: Extend `_core.pyi`**

Append to `prograph/_core.pyi`:
```python
class OutboundEdge:
    kind: str
    target_kind: str
    target_name: str
    target_slug: str
    attrs_json: str

class InboundEdge:
    kind: str
    source_name: str
    source_slug: str
    attrs_json: str

class McpToolDeclRow:
    tool_name: str
    rel_path: str
    line: int

class ContractFileRow:
    contract_declared_id: str | None
    contract_slug: str
    contract_kind: str
    rel_path: str

class RecentChangeRow:
    snapshot_id: int
    ts: str
    change: str
    summary: str

class ProjectDescription:
    project_id: int
    name: str
    slug: str
    kind: str
    root_path: str
    attrs_json: str
    snapshot_id: int
    snapshot_ts: str
    mcp_decls: list[McpToolDeclRow]
    contract_files: list[ContractFileRow]
    outbound: list[OutboundEdge]
    inbound: list[InboundEdge]
    recent_changes: list[RecentChangeRow]

class ContractOwner:
    project_name: str
    project_slug: str
    rel_path: str

class ContractDescription:
    contract_id: int
    declared_id: str | None
    slug: str
    kind: str
    content_hash: str
    snapshot_id: int
    snapshot_ts: str
    owners: list[ContractOwner]
    recent_changes: list[RecentChangeRow]

class ProjectSummary:
    name: str
    slug: str
    kind: str

class ContractSummary:
    slug: str
    declared_id: str | None
    kind: str
    n_owners: int

class MonorepoOverview:
    monorepo_root: str
    snapshot_id: int
    snapshot_ts: str
    n_projects: int
    n_contracts: int
    n_edges: int
    projects: list[ProjectSummary]
    contracts: list[ContractSummary]
    recent_changes: list[RecentChangeRow]
```

- [ ] **Step 4: Rebuild + verify compilation**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv sync --reinstall-package prograph
cargo test --package prograph-core
```
Expected: 116 tests still pass (no new tests; just new types).

Verify clean (cargo fmt + clippy).

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/models.rs prograph/prograph-core/src/lib.rs \
        prograph/prograph/_core.pyi
git commit -m "prograph: M5 aggregation pyclasses — ProjectDescription / ContractDescription / MonorepoOverview"
```

---

## Task 4: `Store::describe_project`, `describe_contract`, `monorepo_overview`

**Files:**
- Modify: `prograph-core/src/store.rs`

Three big SELECT-with-joins queries that produce the aggregation pyclasses. Each is one method; each runs a small number of queries (≤5 per method). Determinism: every list is sorted by stable keys in SQL.

- [ ] **Step 1: Add a slugify helper**

Add at the top of `prograph-core/src/store.rs` (or below the existing module-level constants):
```rust
/// Sanitize an identifier (project name or contract declared_id) into a filesystem-safe
/// filename slug. Replaces any character that isn't alphanumeric, dash, or underscore
/// with `-`. Preserves case. Empty input → "_unnamed".
fn slugify(s: &str) -> String {
    if s.is_empty() {
        return "_unnamed".to_string();
    }
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect()
}

/// Slug for a contract — declared_id if present, else first 12 chars of content_hash.
fn contract_slug(declared_id: Option<&str>, content_hash: &str) -> String {
    match declared_id {
        Some(id) if !id.is_empty() => slugify(id),
        _ => format!("hash-{}", &content_hash[..content_hash.len().min(12)]),
    }
}
```

- [ ] **Step 2: `describe_project`**

Append to `impl Store`:
```rust
    /// Build a complete `ProjectDescription` for one project at the latest snapshot.
    /// Returns `None` if the project doesn't exist in the latest snapshot.
    pub fn describe_project(
        &self,
        project_id: i64,
    ) -> Result<Option<crate::models::ProjectDescription>> {
        use crate::models::*;

        // Latest snapshot id + ts.
        let snap_meta = self.conn.query_row(
            "SELECT id, ts FROM snapshots ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        );
        let (snap_id, snap_ts) = match snap_meta {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        // Project row.
        let proj = self.conn.query_row(
            "SELECT id, name, kind, root_path, attrs_json FROM projects
             WHERE id = ? AND last_seen = ?",
            rusqlite::params![project_id, snap_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        );
        let (pid, name, kind, root_path, attrs_json) = match proj {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        // MCP tool decls.
        let mut decls: Vec<McpToolDeclRow> = self
            .conn
            .prepare(
                "SELECT tool_name, rel_path, line FROM mcp_tool_decls
                 WHERE project_id = ? AND last_seen = ?
                 ORDER BY tool_name",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                Ok(McpToolDeclRow {
                    tool_name: r.get(0)?,
                    rel_path: r.get(1)?,
                    line: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        decls.sort_by(|a, b| a.tool_name.cmp(&b.tool_name));

        // Contract files owned by this project.
        let contract_files: Vec<ContractFileRow> = self
            .conn
            .prepare(
                "SELECT c.declared_id, c.content_hash, c.kind, cf.rel_path
                 FROM contract_files cf
                 JOIN contracts c ON c.id = cf.contract_id
                 WHERE cf.project_id = ? AND cf.last_seen = ?
                 ORDER BY COALESCE(c.declared_id, c.content_hash), cf.rel_path",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                let declared_id: Option<String> = r.get(0)?;
                let content_hash: String = r.get(1)?;
                Ok(ContractFileRow {
                    contract_slug: contract_slug(declared_id.as_deref(), &content_hash),
                    contract_declared_id: declared_id,
                    contract_kind: r.get(2)?,
                    rel_path: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        // Outbound edges.
        let outbound: Vec<OutboundEdge> = self
            .conn
            .prepare(
                "SELECT e.kind, e.to_kind, e.attrs_json,
                        CASE e.to_kind
                            WHEN 'project' THEN (SELECT name FROM projects WHERE id = e.to_id)
                            WHEN 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.to_id)
                        END AS target_name,
                        CASE e.to_kind
                            WHEN 'project' THEN (SELECT name FROM projects WHERE id = e.to_id)
                            WHEN 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.to_id)
                        END AS target_slug_src,
                        e.to_kind, e.to_id
                 FROM edges e
                 WHERE e.from_kind = 'project' AND e.from_id = ? AND e.last_seen = ?
                 ORDER BY e.kind, target_name",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                let kind: String = r.get(0)?;
                let target_kind: String = r.get(1)?;
                let attrs_json: String = r.get(2)?;
                let target_name: String = r.get::<_, Option<String>>(3)?.unwrap_or_default();
                let to_kind: String = r.get(5)?;
                let to_id: i64 = r.get(6)?;
                let target_slug = if to_kind == "contract" {
                    // Look up via contract slug.
                    let row: rusqlite::Result<(Option<String>, String)> = self.conn.query_row(
                        "SELECT declared_id, content_hash FROM contracts WHERE id = ?",
                        rusqlite::params![to_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    );
                    match row {
                        Ok((d, h)) => contract_slug(d.as_deref(), &h),
                        Err(_) => "unknown".into(),
                    }
                } else {
                    slugify(&target_name)
                };
                Ok(OutboundEdge { kind, target_kind, target_name, target_slug, attrs_json })
            })?
            .collect::<rusqlite::Result<_>>()?;

        // Inbound edges (only from projects — contract→project doesn't exist in M5).
        let inbound: Vec<InboundEdge> = self
            .conn
            .prepare(
                "SELECT e.kind, e.attrs_json, p.name
                 FROM edges e
                 JOIN projects p ON p.id = e.from_id
                 WHERE e.to_kind = 'project' AND e.to_id = ? AND e.last_seen = ?
                 ORDER BY e.kind, p.name",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                let source_name: String = r.get(2)?;
                Ok(InboundEdge {
                    kind: r.get(0)?,
                    attrs_json: r.get(1)?,
                    source_slug: slugify(&source_name),
                    source_name,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        // Recent changes touching this project (joined via change_log.entity_id when entity_kind='project').
        // Limit 5.
        let recent_changes: Vec<RecentChangeRow> = self
            .conn
            .prepare(
                "SELECT snapshot_id, ts, change, after_json, before_json
                 FROM change_log
                 WHERE entity_kind = 'project' AND entity_id = ?
                 ORDER BY snapshot_id DESC LIMIT 5",
            )?
            .query_map(rusqlite::params![pid], |r| {
                let change: String = r.get(2)?;
                Ok(RecentChangeRow {
                    snapshot_id: r.get(0)?,
                    ts: r.get(1)?,
                    summary: format!("project {}", change),
                    change,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        Ok(Some(ProjectDescription {
            project_id: pid,
            slug: slugify(&name),
            name,
            kind,
            root_path,
            attrs_json,
            snapshot_id: snap_id,
            snapshot_ts: snap_ts,
            mcp_decls: decls,
            contract_files,
            outbound,
            inbound,
            recent_changes,
        }))
    }
```

- [ ] **Step 3: `describe_contract`**

Append:
```rust
    pub fn describe_contract(
        &self,
        contract_id: i64,
    ) -> Result<Option<crate::models::ContractDescription>> {
        use crate::models::*;

        let snap_meta = self.conn.query_row(
            "SELECT id, ts FROM snapshots ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        );
        let (snap_id, snap_ts) = match snap_meta {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        let cont = self.conn.query_row(
            "SELECT id, declared_id, content_hash, kind FROM contracts
             WHERE id = ? AND last_seen = ?",
            rusqlite::params![contract_id, snap_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        );
        let (cid, declared_id, content_hash, kind) = match cont {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        let owners: Vec<ContractOwner> = self
            .conn
            .prepare(
                "SELECT p.name, cf.rel_path
                 FROM contract_files cf
                 JOIN projects p ON p.id = cf.project_id
                 WHERE cf.contract_id = ? AND cf.last_seen = ?
                 ORDER BY p.name, cf.rel_path",
            )?
            .query_map(rusqlite::params![cid, snap_id], |r| {
                let project_name: String = r.get(0)?;
                Ok(ContractOwner {
                    project_slug: slugify(&project_name),
                    project_name,
                    rel_path: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let recent_changes: Vec<RecentChangeRow> = self
            .conn
            .prepare(
                "SELECT snapshot_id, ts, change
                 FROM change_log
                 WHERE entity_kind = 'contract' AND entity_id = ?
                 ORDER BY snapshot_id DESC LIMIT 5",
            )?
            .query_map(rusqlite::params![cid], |r| {
                let change: String = r.get(2)?;
                Ok(RecentChangeRow {
                    snapshot_id: r.get(0)?,
                    ts: r.get(1)?,
                    summary: format!("contract {}", change),
                    change,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        Ok(Some(ContractDescription {
            contract_id: cid,
            slug: contract_slug(declared_id.as_deref(), &content_hash),
            declared_id,
            content_hash,
            kind,
            snapshot_id: snap_id,
            snapshot_ts: snap_ts,
            owners,
            recent_changes,
        }))
    }
```

- [ ] **Step 4: `monorepo_overview`**

Append:
```rust
    pub fn monorepo_overview(&self) -> Result<Option<crate::models::MonorepoOverview>> {
        use crate::models::*;

        let snap = self.conn.query_row(
            "SELECT id, ts, monorepo_root FROM snapshots ORDER BY id DESC LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        );
        let (snap_id, snap_ts, monorepo_root) = match snap {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        let projects: Vec<ProjectSummary> = self
            .conn
            .prepare(
                "SELECT name, kind FROM projects
                 WHERE last_seen = ?
                 ORDER BY name",
            )?
            .query_map(rusqlite::params![snap_id], |r| {
                let name: String = r.get(0)?;
                Ok(ProjectSummary {
                    slug: slugify(&name),
                    name,
                    kind: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let contracts: Vec<ContractSummary> = self
            .conn
            .prepare(
                "SELECT c.id, c.declared_id, c.content_hash, c.kind,
                        (SELECT COUNT(DISTINCT cf.project_id)
                         FROM contract_files cf
                         WHERE cf.contract_id = c.id AND cf.last_seen = ?) AS n_owners
                 FROM contracts c
                 WHERE c.last_seen = ?
                 ORDER BY COALESCE(c.declared_id, c.content_hash)",
            )?
            .query_map(rusqlite::params![snap_id, snap_id], |r| {
                let declared_id: Option<String> = r.get(1)?;
                let content_hash: String = r.get(2)?;
                Ok(ContractSummary {
                    slug: contract_slug(declared_id.as_deref(), &content_hash),
                    declared_id,
                    kind: r.get(3)?,
                    n_owners: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let n_edges: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE last_seen = ?",
            rusqlite::params![snap_id],
            |r| r.get(0),
        )?;

        let recent_changes: Vec<RecentChangeRow> = self
            .conn
            .prepare(
                "SELECT snapshot_id, ts, entity_kind, change
                 FROM change_log
                 ORDER BY snapshot_id DESC LIMIT 10",
            )?
            .query_map([], |r| {
                let entity_kind: String = r.get(2)?;
                let change: String = r.get(3)?;
                Ok(RecentChangeRow {
                    snapshot_id: r.get(0)?,
                    ts: r.get(1)?,
                    summary: format!("{} {}", entity_kind, change),
                    change,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        Ok(Some(MonorepoOverview {
            monorepo_root,
            snapshot_id: snap_id,
            snapshot_ts: snap_ts,
            n_projects: projects.len() as i64,
            n_contracts: contracts.len() as i64,
            n_edges,
            projects,
            contracts,
            recent_changes,
        }))
    }
```

- [ ] **Step 5: PyO3 wrappers**

In `prograph-core/src/lib.rs`, append PyO3 wrapper functions:
```rust
#[pyfunction]
#[pyo3(name = "describe_project")]
fn py_describe_project(db_path: &str, project_id: i64) -> PyResult<Option<ProjectDescription>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.describe_project(project_id)?)
}

#[pyfunction]
#[pyo3(name = "describe_contract")]
fn py_describe_contract(db_path: &str, contract_id: i64) -> PyResult<Option<ContractDescription>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.describe_contract(contract_id)?)
}

#[pyfunction]
#[pyo3(name = "monorepo_overview")]
fn py_monorepo_overview(db_path: &str) -> PyResult<Option<MonorepoOverview>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.monorepo_overview()?)
}
```

Register inside `#[pymodule]`:
```rust
    m.add_function(wrap_pyfunction!(py_describe_project, m)?)?;
    m.add_function(wrap_pyfunction!(py_describe_contract, m)?)?;
    m.add_function(wrap_pyfunction!(py_monorepo_overview, m)?)?;
```

Extend `prograph/_core.pyi`:
```python
def describe_project(db_path: str, project_id: int) -> ProjectDescription | None: ...
def describe_contract(db_path: str, contract_id: int) -> ContractDescription | None: ...
def monorepo_overview(db_path: str) -> MonorepoOverview | None: ...
```

- [ ] **Step 6: Tests**

Append to `store.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn describe_project_returns_none_for_empty_db() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        assert!(store.describe_project(1).unwrap().is_none());
    }

    #[test]
    fn describe_project_aggregates_full_card() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid_a = writer.insert_project(snap, "alpha", "./alpha", "python", "{}").unwrap();
        let pid_b = writer.insert_project(snap, "beta", "./beta", "python", "{}").unwrap();
        writer.insert_edge(
            snap, "package_dep", "project", pid_a, "project", pid_b,
            r#"{"dep_name":"beta"}"#, "h1",
        ).unwrap();
        writer.insert_mcp_tool_decl(pid_a, "decide", "src/server.py", 10, snap).unwrap();
        writer.commit().unwrap();

        let desc = store.describe_project(pid_a).unwrap().unwrap();
        assert_eq!(desc.name, "alpha");
        assert_eq!(desc.outbound.len(), 1);
        assert_eq!(desc.outbound[0].target_name, "beta");
        assert_eq!(desc.mcp_decls.len(), 1);
        assert_eq!(desc.mcp_decls[0].tool_name, "decide");
    }

    #[test]
    fn monorepo_overview_reports_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid_a = writer.insert_project(snap, "alpha", "./alpha", "python", "{}").unwrap();
        let pid_b = writer.insert_project(snap, "beta", "./beta", "rust", "{}").unwrap();
        writer.insert_edge(
            snap, "package_dep", "project", pid_a, "project", pid_b,
            "{}", "h",
        ).unwrap();
        writer.commit().unwrap();

        let ov = store.monorepo_overview().unwrap().unwrap();
        assert_eq!(ov.n_projects, 2);
        assert_eq!(ov.n_edges, 1);
        let names: Vec<_> = ov.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }
```

- [ ] **Step 7: Run tests**

```sh
cargo test --package prograph-core
```
Expected: 119 tests (116 prior + 3 new).

Verify clean.

- [ ] **Step 8: Commit**

```sh
git add prograph/prograph-core/src/store.rs prograph/prograph-core/src/lib.rs \
        prograph/prograph/_core.pyi
git commit -m "prograph: M5 Store::{describe_project, describe_contract, monorepo_overview} + PyO3 wrappers"
```

---

## Task 5: Pydantic mirrors of aggregation types

**Files:**
- Modify: `prograph/models.py`
- Modify: `prograph/__init__.py`

Pydantic mirrors that round-trip from `_core` types. The renderer imports these.

- [ ] **Step 1: Append mirrors to `prograph/models.py`**

```python
class OutboundEdge(BaseModel):
    model_config = ConfigDict(frozen=True)

    kind: str
    target_kind: str
    target_name: str
    target_slug: str
    attrs: dict[str, object]

    @classmethod
    def from_core(cls, value: _core.OutboundEdge) -> OutboundEdge:
        import json
        return cls(
            kind=value.kind,
            target_kind=value.target_kind,
            target_name=value.target_name,
            target_slug=value.target_slug,
            attrs=json.loads(value.attrs_json) if value.attrs_json else {},
        )


class InboundEdge(BaseModel):
    model_config = ConfigDict(frozen=True)

    kind: str
    source_name: str
    source_slug: str
    attrs: dict[str, object]

    @classmethod
    def from_core(cls, value: _core.InboundEdge) -> InboundEdge:
        import json
        return cls(
            kind=value.kind,
            source_name=value.source_name,
            source_slug=value.source_slug,
            attrs=json.loads(value.attrs_json) if value.attrs_json else {},
        )


class McpToolDeclRow(BaseModel):
    model_config = ConfigDict(frozen=True)

    tool_name: str
    rel_path: str
    line: int

    @classmethod
    def from_core(cls, value: _core.McpToolDeclRow) -> McpToolDeclRow:
        return cls(tool_name=value.tool_name, rel_path=value.rel_path, line=value.line)


class ContractFileRow(BaseModel):
    model_config = ConfigDict(frozen=True)

    contract_declared_id: str | None
    contract_slug: str
    contract_kind: str
    rel_path: str

    @classmethod
    def from_core(cls, value: _core.ContractFileRow) -> ContractFileRow:
        return cls(
            contract_declared_id=value.contract_declared_id,
            contract_slug=value.contract_slug,
            contract_kind=value.contract_kind,
            rel_path=value.rel_path,
        )


class RecentChangeRow(BaseModel):
    model_config = ConfigDict(frozen=True)

    snapshot_id: int
    ts: str
    change: str
    summary: str

    @classmethod
    def from_core(cls, value: _core.RecentChangeRow) -> RecentChangeRow:
        return cls(
            snapshot_id=value.snapshot_id,
            ts=value.ts,
            change=value.change,
            summary=value.summary,
        )


class ProjectDescription(BaseModel):
    model_config = ConfigDict(frozen=True)

    project_id: int
    name: str
    slug: str
    kind: str
    root_path: str
    attrs: dict[str, object]
    snapshot_id: int
    snapshot_ts: str
    mcp_decls: list[McpToolDeclRow]
    contract_files: list[ContractFileRow]
    outbound: list[OutboundEdge]
    inbound: list[InboundEdge]
    recent_changes: list[RecentChangeRow]

    @classmethod
    def from_core(cls, value: _core.ProjectDescription) -> ProjectDescription:
        import json
        return cls(
            project_id=value.project_id,
            name=value.name,
            slug=value.slug,
            kind=value.kind,
            root_path=value.root_path,
            attrs=json.loads(value.attrs_json) if value.attrs_json else {},
            snapshot_id=value.snapshot_id,
            snapshot_ts=value.snapshot_ts,
            mcp_decls=[McpToolDeclRow.from_core(d) for d in value.mcp_decls],
            contract_files=[ContractFileRow.from_core(c) for c in value.contract_files],
            outbound=[OutboundEdge.from_core(e) for e in value.outbound],
            inbound=[InboundEdge.from_core(e) for e in value.inbound],
            recent_changes=[RecentChangeRow.from_core(c) for c in value.recent_changes],
        )


class ContractOwner(BaseModel):
    model_config = ConfigDict(frozen=True)

    project_name: str
    project_slug: str
    rel_path: str

    @classmethod
    def from_core(cls, value: _core.ContractOwner) -> ContractOwner:
        return cls(
            project_name=value.project_name,
            project_slug=value.project_slug,
            rel_path=value.rel_path,
        )


class ContractDescription(BaseModel):
    model_config = ConfigDict(frozen=True)

    contract_id: int
    declared_id: str | None
    slug: str
    kind: str
    content_hash: str
    snapshot_id: int
    snapshot_ts: str
    owners: list[ContractOwner]
    recent_changes: list[RecentChangeRow]

    @classmethod
    def from_core(cls, value: _core.ContractDescription) -> ContractDescription:
        return cls(
            contract_id=value.contract_id,
            declared_id=value.declared_id,
            slug=value.slug,
            kind=value.kind,
            content_hash=value.content_hash,
            snapshot_id=value.snapshot_id,
            snapshot_ts=value.snapshot_ts,
            owners=[ContractOwner.from_core(o) for o in value.owners],
            recent_changes=[RecentChangeRow.from_core(c) for c in value.recent_changes],
        )


class ProjectSummary(BaseModel):
    model_config = ConfigDict(frozen=True)

    name: str
    slug: str
    kind: str

    @classmethod
    def from_core(cls, value: _core.ProjectSummary) -> ProjectSummary:
        return cls(name=value.name, slug=value.slug, kind=value.kind)


class ContractSummary(BaseModel):
    model_config = ConfigDict(frozen=True)

    slug: str
    declared_id: str | None
    kind: str
    n_owners: int

    @classmethod
    def from_core(cls, value: _core.ContractSummary) -> ContractSummary:
        return cls(
            slug=value.slug,
            declared_id=value.declared_id,
            kind=value.kind,
            n_owners=value.n_owners,
        )


class MonorepoOverview(BaseModel):
    model_config = ConfigDict(frozen=True)

    monorepo_root: str
    snapshot_id: int
    snapshot_ts: str
    n_projects: int
    n_contracts: int
    n_edges: int
    projects: list[ProjectSummary]
    contracts: list[ContractSummary]
    recent_changes: list[RecentChangeRow]

    @classmethod
    def from_core(cls, value: _core.MonorepoOverview) -> MonorepoOverview:
        return cls(
            monorepo_root=value.monorepo_root,
            snapshot_id=value.snapshot_id,
            snapshot_ts=value.snapshot_ts,
            n_projects=value.n_projects,
            n_contracts=value.n_contracts,
            n_edges=value.n_edges,
            projects=[ProjectSummary.from_core(p) for p in value.projects],
            contracts=[ContractSummary.from_core(c) for c in value.contracts],
            recent_changes=[RecentChangeRow.from_core(c) for c in value.recent_changes],
        )
```

- [ ] **Step 2: Extend `__init__.py` re-exports**

Append the new names to `prograph/__init__.py`'s `__all__` and import list (alphabetical):
```python
from prograph.models import (
    ChangeEvent,
    ChangeKind,
    Contract,
    ContractDescription,
    ContractFileRow,
    ContractOwner,
    ContractSummary,
    Edge,
    EdgeKind,
    EntityKind,
    InboundEdge,
    IndexSummary,
    McpToolDeclRow,
    MonorepoOverview,
    NodeKind,
    OutboundEdge,
    ProjectCandidate,
    ProjectDescription,
    ProjectKind,
    ProjectSummary,
    RecentChangeRow,
    SnapshotInfo,
)
```

Update `__all__` alphabetically to include the 10 new names.

- [ ] **Step 3: Smoke test the round-trip**

Append to `tests/unit/test_models.py`:
```python
def test_project_description_round_trip(tmp_path: Path):
    """End-to-end: construct a snapshot, fetch ProjectDescription, mirror to pydantic."""
    from prograph._core import index_monorepo, describe_project
    from prograph.models import ProjectDescription
    import sqlite3

    (tmp_path / ".prograph").mkdir()
    (tmp_path / "alpha").mkdir()
    (tmp_path / "alpha" / "pyproject.toml").write_text("[project]\nname='alpha'\n")
    (tmp_path / "beta").mkdir()
    (tmp_path / "beta" / "pyproject.toml").write_text("[project]\nname='beta'\ndependencies=['alpha']\n")

    db = tmp_path / ".prograph" / "graph.db"
    index_monorepo(str(tmp_path), str(db))

    conn = sqlite3.connect(db)
    pid = conn.execute(
        "SELECT id FROM projects WHERE name = 'beta' AND last_seen = (SELECT MAX(id) FROM snapshots)"
    ).fetchone()[0]
    conn.close()

    raw = describe_project(str(db), pid)
    assert raw is not None
    desc = ProjectDescription.from_core(raw)
    assert desc.name == "beta"
    assert any(e.target_name == "alpha" for e in desc.outbound)
```

(You'll need to add `Path` from `pathlib` at the top if not already imported.)

- [ ] **Step 4: Rebuild + run**

```sh
uv sync --reinstall-package prograph
uv run pytest tests/unit/test_models.py -v
```
Expected: 10 model tests (9 prior + 1 new).

Full suite:
```sh
uv run pytest -v
```
Expected: 46 tests.

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph/models.py prograph/prograph/__init__.py prograph/tests/unit/test_models.py
git commit -m "prograph: M5 pydantic mirrors of aggregation types + round-trip test"
```

---

## Task 6: `prograph.export.intro` — first-paragraph extraction

**Files:**
- Create: `prograph/export/__init__.py`
- Create: `prograph/export/intro.py`
- Create: `tests/unit/test_export_intro.py`

The `> intro` blockquote in each project MD comes from the first paragraph of `README.md` / `CLAUDE.md` / `TODO.md` (in that order; first match wins). Sanitization: strip the `# Title` line if present; take the next non-blank paragraph; cap at 200 characters; replace newlines with spaces.

- [ ] **Step 1: Create the empty package init**

`prograph/export/__init__.py`:
```python
"""prograph.export — Markdown rendering for snapshot data."""
```

- [ ] **Step 2: Write `intro.py`**

`prograph/export/intro.py`:
```python
"""Extract a one-line intro from a project's README/CLAUDE/TODO."""

from __future__ import annotations

import re
from pathlib import Path

_PROBES = ("README.md", "CLAUDE.md", "TODO.md")
_MAX_LEN = 200


def extract_intro(project_root: Path) -> str | None:
    """Return a one-line intro for `project_root`, or None if no probe file usable.

    Algorithm:
    1. For each probe file in order, read up to ~4KB from the start.
    2. Skip leading blank lines and the first `# Heading` line if any.
    3. Take the next non-blank paragraph.
    4. Strip Markdown emphasis markers (`*`, `_`, `**`); collapse whitespace to single spaces.
    5. Truncate at `_MAX_LEN` characters (split on word boundary if possible).
    """
    for probe in _PROBES:
        path = project_root / probe
        if not path.is_file():
            continue
        try:
            raw = path.read_text(encoding="utf-8")[:4096]
        except OSError:
            continue

        intro = _extract_from_text(raw)
        if intro:
            return intro
    return None


def _extract_from_text(raw: str) -> str | None:
    lines = raw.splitlines()
    i = 0
    # Skip leading blanks.
    while i < len(lines) and not lines[i].strip():
        i += 1
    # Skip an initial heading line.
    if i < len(lines) and lines[i].startswith("#"):
        i += 1
    # Skip blanks after heading.
    while i < len(lines) and not lines[i].strip():
        i += 1
    # Collect first paragraph until blank line or end.
    paragraph_lines: list[str] = []
    while i < len(lines) and lines[i].strip():
        paragraph_lines.append(lines[i].strip())
        i += 1
    if not paragraph_lines:
        return None

    text = " ".join(paragraph_lines)
    # Strip simple Markdown emphasis markers (deliberately not a full Markdown renderer).
    text = re.sub(r"\*\*|__|\*|_", "", text)
    # Collapse whitespace.
    text = re.sub(r"\s+", " ", text).strip()
    if not text:
        return None
    return _truncate(text)


def _truncate(text: str) -> str:
    if len(text) <= _MAX_LEN:
        return text
    cut = text[:_MAX_LEN]
    # Split on the last space inside the window if possible.
    last_space = cut.rfind(" ")
    if last_space > _MAX_LEN - 40:
        cut = cut[:last_space]
    return cut.rstrip(",.;:") + "…"
```

- [ ] **Step 3: Write `test_export_intro.py`**

`tests/unit/test_export_intro.py`:
```python
"""Tests for prograph.export.intro."""

from pathlib import Path

from prograph.export.intro import extract_intro


def test_intro_from_readme(tmp_path: Path):
    (tmp_path / "README.md").write_text(
        "# My Project\n\nA tool that does X and Y.\n\nMore details below.\n"
    )
    assert extract_intro(tmp_path) == "A tool that does X and Y."


def test_intro_prefers_readme_over_claude(tmp_path: Path):
    (tmp_path / "README.md").write_text("# X\n\nReadme says hi.\n")
    (tmp_path / "CLAUDE.md").write_text("# X\n\nClaude says hi.\n")
    assert extract_intro(tmp_path) == "Readme says hi."


def test_intro_falls_back_to_claude(tmp_path: Path):
    (tmp_path / "CLAUDE.md").write_text("# X\n\nClaude only.\n")
    assert extract_intro(tmp_path) == "Claude only."


def test_intro_falls_back_to_todo(tmp_path: Path):
    (tmp_path / "TODO.md").write_text("# X\n\nLast resort.\n")
    assert extract_intro(tmp_path) == "Last resort."


def test_intro_returns_none_when_no_probe(tmp_path: Path):
    assert extract_intro(tmp_path) is None


def test_intro_strips_markdown_emphasis(tmp_path: Path):
    (tmp_path / "README.md").write_text("# X\n\n**Bold** and *italic* text here.\n")
    assert extract_intro(tmp_path) == "Bold and italic text here."


def test_intro_collapses_multiline_paragraph(tmp_path: Path):
    (tmp_path / "README.md").write_text("# X\n\nLine one\nline two\nline three.\n\nNext para.\n")
    assert extract_intro(tmp_path) == "Line one line two line three."


def test_intro_truncates_long_text(tmp_path: Path):
    text = "Word " * 100  # > 400 chars
    (tmp_path / "README.md").write_text(f"# X\n\n{text}\n")
    intro = extract_intro(tmp_path)
    assert intro is not None
    assert len(intro) <= 201  # _MAX_LEN + ellipsis room
    assert intro.endswith("…")


def test_intro_skips_blank_after_heading(tmp_path: Path):
    (tmp_path / "README.md").write_text("# X\n\n\n\nFirst real paragraph.\n")
    assert extract_intro(tmp_path) == "First real paragraph."


def test_intro_handles_no_heading(tmp_path: Path):
    (tmp_path / "README.md").write_text("Plain text first paragraph.\n")
    assert extract_intro(tmp_path) == "Plain text first paragraph."
```

- [ ] **Step 4: Run tests**

```sh
uv run pytest tests/unit/test_export_intro.py -v
```
Expected: 10 passed.

Full suite:
```sh
uv run pytest -v
```
Expected: 56 tests.

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph/export/ prograph/tests/unit/test_export_intro.py
git commit -m "prograph: M5 export.intro — extract first paragraph from README/CLAUDE/TODO"
```

---

## Task 7: `prograph.export.slug` — filename slug helper

**Files:**
- Create: `prograph/export/slug.py`
- Create: `tests/unit/test_export_slug.py`

A Python mirror of Rust's `slugify` function. Used when constructing wiki-links inside the renderer (Rust already computed slugs but renderer also needs to construct them for consistency, e.g. when handling edge cases).

- [ ] **Step 1: Write `slug.py`**

`prograph/export/slug.py`:
```python
"""Filename slugification — mirror of Rust's slugify for filename + wiki-link generation."""

from __future__ import annotations


def slugify(s: str) -> str:
    """Replace any character that isn't alphanumeric, dash, or underscore with `-`.

    Preserves case. Empty input returns "_unnamed".
    """
    if not s:
        return "_unnamed"
    return "".join(c if (c.isalnum() and c.isascii()) or c in "-_" else "-" for c in s)


def contract_slug(declared_id: str | None, content_hash: str) -> str:
    """Slug for a contract — declared_id if present, else first 12 chars of content_hash prefixed `hash-`."""
    if declared_id:
        return slugify(declared_id)
    return f"hash-{content_hash[:12]}"
```

- [ ] **Step 2: Write `test_export_slug.py`**

`tests/unit/test_export_slug.py`:
```python
"""Tests for prograph.export.slug."""

from prograph.export.slug import contract_slug, slugify


def test_slugify_ascii_alphanumeric_preserved():
    assert slugify("alpha-beta_123") == "alpha-beta_123"


def test_slugify_replaces_non_safe_chars():
    assert slugify("foo/bar:baz") == "foo-bar-baz"


def test_slugify_preserves_case():
    assert slugify("Maestro") == "Maestro"


def test_slugify_empty_returns_unnamed():
    assert slugify("") == "_unnamed"


def test_slugify_unicode_replaced():
    # Cyrillic must be replaced; only ASCII alphanumeric counts.
    assert slugify("привет") == "------"


def test_contract_slug_uses_declared_id():
    assert contract_slug("obs-v1", "deadbeef" * 8) == "obs-v1"


def test_contract_slug_falls_back_to_hash():
    h = "abcdef0123456789" + "0" * 48
    assert contract_slug(None, h) == "hash-abcdef012345"


def test_contract_slug_empty_declared_falls_back():
    h = "0123456789ab" + "0" * 52
    assert contract_slug("", h) == "hash-0123456789ab"
```

- [ ] **Step 3: Run + commit**

```sh
uv run pytest tests/unit/test_export_slug.py -v
```
Expected: 8 passed.

```sh
git add prograph/prograph/export/slug.py prograph/tests/unit/test_export_slug.py
git commit -m "prograph: M5 export.slug — Python mirror of Rust slugify"
```

---

## Task 8: `prograph.export.render` — main render functions

**Files:**
- Create: `prograph/export/render.py`
- Create: `tests/unit/test_export_render.py`

Three pure functions take pydantic objects and return MD strings. The CLI command (Task 11) writes the strings to disk; this module has zero filesystem side effects.

Determinism rules enforced here:
- Frontmatter keys sorted alphabetically
- Lists already sorted by the Rust query layer (Task 4) — renderer does NOT re-sort
- `\n` line endings; single trailing newline; no trailing whitespace per line
- Empty sections render as "_None._" (italic) instead of being omitted — keeps section structure stable across snapshots

- [ ] **Step 1: Write `render.py`**

`prograph/export/render.py`:
```python
"""Render snapshot data to Markdown strings.

Three pure functions:
- render_project(desc) -> str
- render_contract(desc) -> str
- render_index(overview) -> str

All output is deterministic given identical input.
"""

from __future__ import annotations

import json

from prograph.models import (
    ContractDescription,
    MonorepoOverview,
    OutboundEdge,
    ProjectDescription,
)


def render_project(desc: ProjectDescription, intro: str | None = None) -> str:
    """Render one project's MD file."""
    frontmatter = _frontmatter(
        {
            "indexed_at": desc.snapshot_ts,
            "kind": desc.kind,
            "name": desc.name,
            "prograph": "project",
            "root": desc.root_path,
            "snapshot": desc.snapshot_id,
        }
    )

    lines: list[str] = []
    lines.append(frontmatter)
    lines.append("")
    lines.append(f"# {desc.name}")
    lines.append("")
    if intro:
        lines.append(f"> {intro}")
        lines.append("")

    # Manifest
    lines.append("## Manifest")
    lines.append("")
    decl_name = desc.attrs.get("declared_name") or desc.attrs.get("name") or desc.name
    version = desc.attrs.get("version")
    if version:
        lines.append(f"- declared package: `{decl_name}` version `{version}`")
    else:
        lines.append(f"- declared package: `{decl_name}`")
    lines.append("")

    # Public surface
    lines.append("## Public surface")
    lines.append("")
    lines.append("### MCP tools exposed")
    lines.append("")
    if desc.mcp_decls:
        for d in desc.mcp_decls:
            lines.append(f"- `{d.tool_name}` — `{d.rel_path}:{d.line}`")
    else:
        lines.append("_None._")
    lines.append("")

    lines.append("### Contracts declared")
    lines.append("")
    if desc.contract_files:
        seen_slugs: set[str] = set()
        for cf in desc.contract_files:
            if cf.contract_slug in seen_slugs:
                continue
            seen_slugs.add(cf.contract_slug)
            display = cf.contract_declared_id or cf.contract_slug
            lines.append(f"- [[{cf.contract_slug}]] ({cf.contract_kind}) — `{cf.rel_path}` — `{display}`")
    else:
        lines.append("_None._")
    lines.append("")

    # Outbound edges
    lines.append("## Outbound edges")
    lines.append("")
    if desc.outbound:
        for e in desc.outbound:
            lines.append(_render_outbound(e))
    else:
        lines.append("_None._")
    lines.append("")

    # Inbound edges
    lines.append("## Inbound edges")
    lines.append("")
    if desc.inbound:
        for e in desc.inbound:
            lines.append(f"- ← [[{e.source_slug}]] · `{e.kind}`{_inbound_attr_suffix(e.attrs, e.kind)}")
    else:
        lines.append("_None._")
    lines.append("")

    # Recent changes
    lines.append("## Recent changes (last 5)")
    lines.append("")
    if desc.recent_changes:
        for c in desc.recent_changes:
            lines.append(f"- snapshot {c.snapshot_id} ({c.ts}): {c.summary} ({c.change})")
    else:
        lines.append("_None._")
    lines.append("")

    return _finalize(lines)


def render_contract(desc: ContractDescription) -> str:
    """Render one contract's MD file."""
    title = desc.declared_id or desc.slug

    frontmatter = _frontmatter(
        {
            "content_hash": desc.content_hash,
            "declared_id": desc.declared_id or "",
            "indexed_at": desc.snapshot_ts,
            "kind": desc.kind,
            "prograph": "contract",
            "snapshot": desc.snapshot_id,
        }
    )

    lines: list[str] = []
    lines.append(frontmatter)
    lines.append("")
    lines.append(f"# Contract: {title}")
    lines.append("")
    lines.append(f"- kind: `{desc.kind}`")
    if desc.declared_id:
        lines.append(f"- declared id: `{desc.declared_id}`")
    lines.append(f"- content hash: `{desc.content_hash[:16]}…`")
    lines.append("")

    lines.append("## Owners")
    lines.append("")
    if desc.owners:
        for o in desc.owners:
            lines.append(f"- [[{o.project_slug}]] — `{o.rel_path}`")
    else:
        lines.append("_None._")
    lines.append("")

    lines.append("## Recent changes (last 5)")
    lines.append("")
    if desc.recent_changes:
        for c in desc.recent_changes:
            lines.append(f"- snapshot {c.snapshot_id} ({c.ts}): {c.summary} ({c.change})")
    else:
        lines.append("_None._")
    lines.append("")

    return _finalize(lines)


def render_index(overview: MonorepoOverview) -> str:
    """Render the monorepo-level index.md."""
    frontmatter = _frontmatter(
        {
            "indexed_at": overview.snapshot_ts,
            "n_contracts": overview.n_contracts,
            "n_edges": overview.n_edges,
            "n_projects": overview.n_projects,
            "prograph": "index",
            "snapshot": overview.snapshot_id,
        }
    )

    lines: list[str] = []
    lines.append(frontmatter)
    lines.append("")
    lines.append(f"# Monorepo: {overview.monorepo_root}")
    lines.append("")

    lines.append("## Projects")
    lines.append("")
    if overview.projects:
        for p in overview.projects:
            lines.append(f"- [[{p.slug}]] — {p.kind}")
    else:
        lines.append("_None._")
    lines.append("")

    lines.append("## Contracts")
    lines.append("")
    if overview.contracts:
        for c in overview.contracts:
            display = c.declared_id or c.slug
            owners_str = f"{c.n_owners} owner" + ("s" if c.n_owners != 1 else "")
            lines.append(f"- [[{c.slug}]] — `{c.kind}` ({owners_str}) — `{display}`")
    else:
        lines.append("_None._")
    lines.append("")

    lines.append("## Recent activity (last 10)")
    lines.append("")
    if overview.recent_changes:
        for c in overview.recent_changes:
            lines.append(f"- snapshot {c.snapshot_id} ({c.ts}): {c.summary} ({c.change})")
    else:
        lines.append("_None._")
    lines.append("")

    return _finalize(lines)


def _frontmatter(fields: dict[str, object]) -> str:
    """YAML frontmatter — keys sorted alphabetically for byte stability."""
    out = ["---"]
    for k in sorted(fields.keys()):
        v = fields[k]
        if isinstance(v, str):
            # If the value contains a colon or starts with special char, quote it.
            if any(ch in v for ch in ":#\n") or v.startswith(("'", '"', "[", "{", "-")):
                v = json.dumps(v)
            else:
                v = v
        out.append(f"{k}: {v}")
    out.append("---")
    return "\n".join(out)


def _render_outbound(e: OutboundEdge) -> str:
    arrow = "↔" if e.kind == "contract_link" else "→"
    suffix = ""
    if e.kind == "package_dep":
        dep_name = e.attrs.get("dep_name")
        version_req = e.attrs.get("version_req")
        if dep_name:
            if version_req:
                suffix = f" · `{dep_name}` `{version_req}`"
            else:
                suffix = f" · `{dep_name}`"
    elif e.kind == "mcp_call":
        tool = e.attrs.get("tool")
        if tool:
            suffix = f" · tool `{tool}`"
    elif e.kind == "contract_link":
        ckind = e.attrs.get("contract_kind")
        if ckind:
            suffix = f" · `{ckind}`"

    return f"- {arrow} [[{e.target_slug}]] · `{e.kind}`{suffix}"


def _inbound_attr_suffix(attrs: dict[str, object], kind: str) -> str:
    if kind == "package_dep":
        dep_name = attrs.get("dep_name")
        if dep_name:
            return f" · `{dep_name}`"
    elif kind == "mcp_call":
        tool = attrs.get("tool")
        if tool:
            return f" · tool `{tool}`"
    return ""


def _finalize(lines: list[str]) -> str:
    """Join, strip trailing whitespace per line, ensure exactly one trailing newline."""
    cleaned = [line.rstrip() for line in lines]
    text = "\n".join(cleaned).rstrip("\n") + "\n"
    return text
```

- [ ] **Step 2: Write unit tests**

`tests/unit/test_export_render.py`:
```python
"""Tests for prograph.export.render — focus on shape + determinism."""

from prograph.export.render import (
    render_contract,
    render_index,
    render_project,
    _frontmatter,
)
from prograph.models import (
    ContractDescription,
    ContractOwner,
    ContractSummary,
    InboundEdge,
    McpToolDeclRow,
    MonorepoOverview,
    OutboundEdge,
    ProjectDescription,
    ProjectSummary,
)


def _empty_desc(**overrides) -> ProjectDescription:
    defaults = {
        "project_id": 1,
        "name": "x",
        "slug": "x",
        "kind": "python",
        "root_path": "./x",
        "attrs": {"declared_name": "x", "version": "0.1.0"},
        "snapshot_id": 1,
        "snapshot_ts": "2026-05-26T00:00:00Z",
        "mcp_decls": [],
        "contract_files": [],
        "outbound": [],
        "inbound": [],
        "recent_changes": [],
    }
    defaults.update(overrides)
    return ProjectDescription(**defaults)


def test_render_project_minimal():
    desc = _empty_desc()
    md = render_project(desc)
    assert "# x" in md
    assert "_None._" in md  # used for empty sections
    assert md.endswith("\n")
    assert not md.endswith("\n\n")  # exactly one trailing newline


def test_render_project_frontmatter_alphabetical():
    desc = _empty_desc()
    md = render_project(desc)
    # The frontmatter keys are: indexed_at, kind, name, prograph, root, snapshot
    fm_lines = md.split("---")[1].strip().split("\n")
    keys = [line.split(":")[0] for line in fm_lines]
    assert keys == sorted(keys), f"frontmatter keys must be alphabetical, got: {keys}"


def test_render_project_includes_intro_when_provided():
    desc = _empty_desc()
    md = render_project(desc, intro="A short intro.")
    assert "> A short intro." in md


def test_render_project_omits_intro_when_none():
    desc = _empty_desc()
    md = render_project(desc, intro=None)
    assert "> " not in md  # blockquote line absent


def test_render_outbound_edge_package_dep_with_version():
    desc = _empty_desc(
        outbound=[
            OutboundEdge(
                kind="package_dep",
                target_kind="project",
                target_name="beta",
                target_slug="beta",
                attrs={"dep_name": "beta-sdk", "version_req": ">=2.0"},
            )
        ]
    )
    md = render_project(desc)
    assert "→ [[beta]] · `package_dep` · `beta-sdk` `>=2.0`" in md


def test_render_outbound_edge_mcp_call_includes_tool():
    desc = _empty_desc(
        outbound=[
            OutboundEdge(
                kind="mcp_call",
                target_kind="project",
                target_name="server",
                target_slug="server",
                attrs={"tool": "decide"},
            )
        ]
    )
    md = render_project(desc)
    assert "→ [[server]] · `mcp_call` · tool `decide`" in md


def test_render_outbound_edge_contract_link_uses_double_arrow():
    desc = _empty_desc(
        outbound=[
            OutboundEdge(
                kind="contract_link",
                target_kind="contract",
                target_name="obs-v1",
                target_slug="obs-v1",
                attrs={"contract_kind": "json_schema"},
            )
        ]
    )
    md = render_project(desc)
    assert "↔ [[obs-v1]] · `contract_link` · `json_schema`" in md


def test_render_project_is_deterministic():
    desc = _empty_desc(
        mcp_decls=[McpToolDeclRow(tool_name="t1", rel_path="a.py", line=1)],
        outbound=[
            OutboundEdge(
                kind="package_dep",
                target_kind="project",
                target_name="b",
                target_slug="b",
                attrs={"dep_name": "b"},
            )
        ],
    )
    assert render_project(desc) == render_project(desc)


def test_render_contract_minimal():
    desc = ContractDescription(
        contract_id=7,
        declared_id="obs-v1",
        slug="obs-v1",
        kind="json_schema",
        content_hash="a" * 64,
        snapshot_id=1,
        snapshot_ts="2026-05-26T00:00:00Z",
        owners=[
            ContractOwner(project_name="alpha", project_slug="alpha", rel_path="schemas/obs.json"),
        ],
        recent_changes=[],
    )
    md = render_contract(desc)
    assert "# Contract: obs-v1" in md
    assert "[[alpha]]" in md
    assert "`schemas/obs.json`" in md


def test_render_index_minimal():
    overview = MonorepoOverview(
        monorepo_root="/tmp/mr",
        snapshot_id=1,
        snapshot_ts="2026-05-26T00:00:00Z",
        n_projects=2,
        n_contracts=1,
        n_edges=3,
        projects=[
            ProjectSummary(name="alpha", slug="alpha", kind="python"),
            ProjectSummary(name="beta", slug="beta", kind="rust"),
        ],
        contracts=[
            ContractSummary(slug="obs-v1", declared_id="obs-v1", kind="json_schema", n_owners=2),
        ],
        recent_changes=[],
    )
    md = render_index(overview)
    assert "# Monorepo: /tmp/mr" in md
    assert "- [[alpha]] — python" in md
    assert "- [[beta]] — rust" in md
    assert "- [[obs-v1]] — `json_schema` (2 owners)" in md
```

- [ ] **Step 3: Run tests**

```sh
uv run pytest tests/unit/test_export_render.py -v
```
Expected: 10 passed.

Full suite:
```sh
uv run pytest -v
```
Expected: 66 tests.

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph/export/render.py prograph/tests/unit/test_export_render.py
git commit -m "prograph: M5 export.render — render_project / render_contract / render_index"
```

---

## Task 9: `prograph index --export-md` flag + `prograph export-md` standalone command

**Files:**
- Modify: `prograph/cli.py`
- Modify: `prograph/paths.py`

The CLI gains:
- `prograph index --export-md` — index, then write MD files to `.prograph/{projects,contracts,index.md}`
- `prograph export-md` — write MD files from the latest snapshot WITHOUT re-indexing
- `prograph index` honors `[output] auto_export = true` from `.prograph/config.toml` (Task 10 wires the config parse)

- [ ] **Step 1: Add `index_md_path` to `paths.py`**

In `prograph/paths.py`, add a property:
```python
    @property
    def index_md_path(self) -> Path:
        return self.prograph_dir / "index.md"
```

- [ ] **Step 2: Write the export pipeline**

Append to `prograph/export/__init__.py`:
```python
from __future__ import annotations

import sqlite3
from pathlib import Path

from prograph import _core
from prograph.export.intro import extract_intro
from prograph.export.render import render_contract, render_index, render_project
from prograph.models import (
    ContractDescription,
    MonorepoOverview,
    ProjectDescription,
)
from prograph.paths import PrographPaths


def export_snapshot(monorepo_root: Path) -> ExportReport:
    """Render MD files from the latest snapshot in `<monorepo_root>/.prograph/graph.db`
    into `<monorepo_root>/.prograph/{projects,contracts}/*.md` + `index.md`.

    Idempotent — repeated calls produce the same bytes for the same snapshot.
    """
    paths = PrographPaths(monorepo_root=monorepo_root)
    paths.ensure_dirs()

    db_path = paths.db_path
    if not db_path.is_file():
        return ExportReport(monorepo_root=monorepo_root, n_projects=0, n_contracts=0, wrote_index=False)

    overview_raw = _core.monorepo_overview(str(db_path))
    if overview_raw is None:
        return ExportReport(monorepo_root=monorepo_root, n_projects=0, n_contracts=0, wrote_index=False)
    overview = MonorepoOverview.from_core(overview_raw)

    project_ids = _project_ids_in_latest_snapshot(db_path)
    contract_ids = _contract_ids_in_latest_snapshot(db_path)

    n_projects = 0
    for pid in project_ids:
        raw = _core.describe_project(str(db_path), pid)
        if raw is None:
            continue
        desc = ProjectDescription.from_core(raw)
        intro = extract_intro(monorepo_root / desc.root_path.lstrip("./").lstrip("/"))
        md = render_project(desc, intro=intro)
        (paths.projects_md_dir / f"{desc.slug}.md").write_text(md, encoding="utf-8")
        n_projects += 1

    n_contracts = 0
    for cid in contract_ids:
        raw = _core.describe_contract(str(db_path), cid)
        if raw is None:
            continue
        desc = ContractDescription.from_core(raw)
        md = render_contract(desc)
        (paths.contracts_md_dir / f"{desc.slug}.md").write_text(md, encoding="utf-8")
        n_contracts += 1

    md_index = render_index(overview)
    paths.index_md_path.write_text(md_index, encoding="utf-8")

    return ExportReport(
        monorepo_root=monorepo_root,
        n_projects=n_projects,
        n_contracts=n_contracts,
        wrote_index=True,
    )


def _project_ids_in_latest_snapshot(db_path: Path) -> list[int]:
    conn = sqlite3.connect(db_path)
    try:
        rows = conn.execute(
            "SELECT id FROM projects WHERE last_seen = (SELECT MAX(id) FROM snapshots) ORDER BY id"
        ).fetchall()
        return [r[0] for r in rows]
    finally:
        conn.close()


def _contract_ids_in_latest_snapshot(db_path: Path) -> list[int]:
    conn = sqlite3.connect(db_path)
    try:
        rows = conn.execute(
            "SELECT id FROM contracts WHERE last_seen = (SELECT MAX(id) FROM snapshots) ORDER BY id"
        ).fetchall()
        return [r[0] for r in rows]
    finally:
        conn.close()


from dataclasses import dataclass


@dataclass(frozen=True)
class ExportReport:
    monorepo_root: Path
    n_projects: int
    n_contracts: int
    wrote_index: bool
```

(Note: the `import sqlite3` direct use here is a small bend of the "Python never touches SQLite" rule. M5 uses it solely to discover the ID lists that need exporting; the actual data fetch goes through `_core.describe_*`. Acceptable shortcut; M7 may add dedicated `_core.alive_project_ids()` helpers if needed.)

- [ ] **Step 3: Wire the `prograph export-md` command**

In `prograph/cli.py`, add (near the existing `index` and `status` commands):
```python
@app.command("export-md")
def export_md(
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
    """Render Markdown files from the latest snapshot — no reindex."""
    from prograph.export import export_snapshot

    root = _resolve_monorepo(monorepo)
    paths = PrographPaths(monorepo_root=root)
    if not paths.is_initialized():
        err_console.print(
            f"[red]error:[/red] not initialized at {paths.prograph_dir}. "
            "Run `prograph init` first."
        )
        raise typer.Exit(code=1)

    if not paths.db_path.exists():
        err_console.print(
            f"[red]error:[/red] no snapshot to export. Run `prograph index` first."
        )
        raise typer.Exit(code=1)

    report = export_snapshot(root)
    console.print(
        f"[green]exported[/green] {report.n_projects} projects, {report.n_contracts} contracts, "
        f"index{'.md' if report.wrote_index else ' skipped'}"
    )
```

- [ ] **Step 4: Add `--export-md` to `index`**

Find the existing `index` command in `cli.py`. Add a new parameter:
```python
    export_md: bool = typer.Option(  # noqa: B008
        False,
        "--export-md",
        help="Also write MD files after indexing.",
    ),
```

After the existing success path (after `summary = IndexSummary.from_core(raw)` and before the output), add:
```python
    if export_md:
        from prograph.export import export_snapshot
        export_snapshot(root)
```

- [ ] **Step 5: Integration test scaffolding**

`tests/integration/test_cli_export_md.py`:
```python
"""Tests for `prograph index --export-md` and `prograph export-md`."""

from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()


def _setup(root: Path) -> None:
    (root / "alpha").mkdir()
    (root / "alpha" / "pyproject.toml").write_text("[project]\nname='alpha'\n")
    (root / "alpha" / "README.md").write_text("# alpha\n\nThe alpha project.\n")
    (root / "beta").mkdir()
    (root / "beta" / "pyproject.toml").write_text("[project]\nname='beta'\ndependencies=['alpha']\n")


def test_index_with_export_md_writes_files(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--export-md"])
    assert result.exit_code == 0, result.stdout

    paths = PrographPaths(monorepo_root=tmp_path)
    assert (paths.projects_md_dir / "alpha.md").is_file()
    assert (paths.projects_md_dir / "beta.md").is_file()
    assert paths.index_md_path.is_file()

    alpha_md = (paths.projects_md_dir / "alpha.md").read_text()
    assert "# alpha" in alpha_md
    assert "> The alpha project." in alpha_md


def test_export_md_standalone(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    result = runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.stdout

    paths = PrographPaths(monorepo_root=tmp_path)
    assert (paths.projects_md_dir / "alpha.md").is_file()


def test_export_md_idempotent_byte_stable(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])

    paths = PrographPaths(monorepo_root=tmp_path)
    first = (paths.projects_md_dir / "alpha.md").read_bytes()

    runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])
    second = (paths.projects_md_dir / "alpha.md").read_bytes()

    assert first == second, "two export-md runs on the same snapshot must produce identical bytes"


def test_export_md_requires_init(tmp_path: Path):
    result = runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1


def test_export_md_requires_snapshot(tmp_path: Path):
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    # No `index` run — no snapshot.
    result = runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1
    assert "no snapshot" in (result.stdout + result.stderr).lower()
```

- [ ] **Step 6: Run**

```sh
uv run pytest tests/integration/test_cli_export_md.py -v
```
Expected: 5 passed.

Full suite:
```sh
uv run pytest -v
```
Expected: 71 tests.

- [ ] **Step 7: Commit**

```sh
git add prograph/prograph/cli.py prograph/prograph/export/__init__.py prograph/prograph/paths.py \
        prograph/tests/integration/test_cli_export_md.py
git commit -m "prograph: M5 'prograph index --export-md' + 'prograph export-md' standalone"
```

---

## Task 10: `[output] auto_export = true` config option

**Files:**
- Modify: `prograph/cli.py`
- Modify: `prograph/prograph-core/src/...` (NO — config parsing is Python-side)
- Modify: `prograph/prograph/paths.py` (or new `config.py`)

When `auto_export = true` is set in `.prograph/config.toml`, `prograph index` runs the MD export automatically without needing `--export-md`.

- [ ] **Step 1: Add config reading helper**

Create `prograph/config.py`:
```python
"""Read .prograph/config.toml settings."""

from __future__ import annotations

from pathlib import Path

try:
    import tomllib  # Python 3.11+
except ImportError:
    import tomli as tomllib  # type: ignore[no-redef]


def read_auto_export(config_path: Path) -> bool:
    """Return True if `.prograph/config.toml` sets `[output] auto_export = true`."""
    if not config_path.is_file():
        return False
    try:
        data = tomllib.loads(config_path.read_text(encoding="utf-8"))
    except Exception:
        return False
    output = data.get("output")
    if not isinstance(output, dict):
        return False
    return bool(output.get("auto_export", False))
```

- [ ] **Step 2: Wire into `index` command**

In `prograph/cli.py`'s `index` command, replace the existing `if export_md:` block with:
```python
    auto = read_auto_export(paths.config_path)
    if export_md or auto:
        from prograph.export import export_snapshot
        export_snapshot(root)
```

Add the import at the top of `cli.py`:
```python
from prograph.config import read_auto_export
```

- [ ] **Step 3: Update the default `config.toml` template**

In `prograph/cli.py`, find `DEFAULT_CONFIG_TOML`. Append a new `[output]` section:
```python
DEFAULT_CONFIG_TOML = """\
# prograph configuration — edit by hand. Re-running `prograph init` will not overwrite this file.

[monorepo]
# `include` / `exclude` accept glob patterns relative to the monorepo root. If `include` is empty,
# all first-level subdirs are scanned (modulo the exclude list).
include = []
exclude = ["target", "node_modules", "dist", "build", "__pycache__"]

[output]
# When true, `prograph index` automatically writes MD files to .prograph/{projects,contracts}/
# and .prograph/index.md. Same effect as passing `--export-md` to every invocation.
auto_export = false

# Override classification or rename projects whose directory name differs from the package name.
# Example:
#   [[project]]
#   path = "./atp-platform"
#   name = "atp_platform"
#   kind = "python"
"""
```

- [ ] **Step 4: Tests**

Append to `tests/integration/test_cli_export_md.py`:
```python
def test_auto_export_in_config_triggers_md(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    paths = PrographPaths(monorepo_root=tmp_path)
    # Flip auto_export = true in config.toml.
    config = paths.config_path.read_text()
    paths.config_path.write_text(config.replace("auto_export = false", "auto_export = true"))

    # Now `prograph index` (without --export-md) should still write MD.
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0
    assert (paths.projects_md_dir / "alpha.md").is_file()


def test_auto_export_false_skips_md(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    # auto_export defaults to false; index without flag should NOT write MD.
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])

    paths = PrographPaths(monorepo_root=tmp_path)
    assert not (paths.projects_md_dir / "alpha.md").is_file()
```

- [ ] **Step 5: Run + commit**

```sh
uv run pytest tests/integration/test_cli_export_md.py -v
```
Expected: 7 passed.

```sh
git add prograph/prograph/config.py prograph/prograph/cli.py prograph/tests/integration/test_cli_export_md.py
git commit -m "prograph: M5 [output] auto_export config option"
```

---

## Task 11: Golden test infrastructure

**Files:**
- Modify: `tests/conftest.py`
- Create: helper in `tests/conftest.py`

A pytest helper that walks the produced `.prograph/{projects,contracts}/` + `index.md` and compares each file to its peer under `tests/fixtures/<fixture>/golden/`. When `PROGRAPH_UPDATE_GOLDEN=1` is set in the env, the helper writes the produced files to the golden location instead of asserting.

- [ ] **Step 1: Append to `tests/conftest.py`**

```python
import os
import shutil
from pathlib import Path


def assert_md_dir_matches_golden(produced_dir: Path, golden_dir: Path) -> None:
    """Compare every .md file under produced_dir to its peer under golden_dir.

    If PROGRAPH_UPDATE_GOLDEN=1 is set, regenerate the golden directory from produced.
    """
    if os.environ.get("PROGRAPH_UPDATE_GOLDEN") == "1":
        if golden_dir.exists():
            shutil.rmtree(golden_dir)
        shutil.copytree(produced_dir, golden_dir)
        return

    if not golden_dir.exists():
        raise AssertionError(
            f"golden directory missing: {golden_dir}. "
            f"Run with PROGRAPH_UPDATE_GOLDEN=1 to create it."
        )

    produced_files = sorted(p.relative_to(produced_dir) for p in produced_dir.rglob("*.md"))
    golden_files = sorted(p.relative_to(golden_dir) for p in golden_dir.rglob("*.md"))

    if produced_files != golden_files:
        only_in_produced = sorted(set(produced_files) - set(golden_files))
        only_in_golden = sorted(set(golden_files) - set(produced_files))
        msg = ["MD file lists differ between produced and golden:"]
        if only_in_produced:
            msg.append(f"  Only in produced: {only_in_produced}")
        if only_in_golden:
            msg.append(f"  Only in golden: {only_in_golden}")
        msg.append("  Set PROGRAPH_UPDATE_GOLDEN=1 to refresh.")
        raise AssertionError("\n".join(msg))

    for rel in produced_files:
        p_bytes = (produced_dir / rel).read_bytes()
        g_bytes = (golden_dir / rel).read_bytes()
        if p_bytes != g_bytes:
            # Compute a small textual diff for the error message.
            import difflib

            diff = "\n".join(
                difflib.unified_diff(
                    g_bytes.decode("utf-8", errors="replace").splitlines(),
                    p_bytes.decode("utf-8", errors="replace").splitlines(),
                    fromfile=f"golden/{rel}",
                    tofile=f"produced/{rel}",
                    lineterm="",
                )
            )
            raise AssertionError(
                f"MD file differs from golden: {rel}\n{diff}\n"
                f"Set PROGRAPH_UPDATE_GOLDEN=1 to refresh."
            )
```

Make `assert_md_dir_matches_golden` available to test files via a pytest fixture:
```python
import pytest


@pytest.fixture
def md_matcher():
    """Return the assert_md_dir_matches_golden helper as a callable fixture."""
    return assert_md_dir_matches_golden
```

- [ ] **Step 2: Tests for the helper itself**

Append to `tests/unit/test_export_render.py` (or create `test_golden_helper.py`):
```python
def test_golden_helper_passes_on_identical(tmp_path):
    from tests.conftest import assert_md_dir_matches_golden

    produced = tmp_path / "produced"
    golden = tmp_path / "golden"
    produced.mkdir()
    golden.mkdir()
    (produced / "a.md").write_text("hello\n")
    (golden / "a.md").write_text("hello\n")

    assert_md_dir_matches_golden(produced, golden)


def test_golden_helper_raises_on_diff(tmp_path):
    from tests.conftest import assert_md_dir_matches_golden

    produced = tmp_path / "produced"
    golden = tmp_path / "golden"
    produced.mkdir()
    golden.mkdir()
    (produced / "a.md").write_text("hello\n")
    (golden / "a.md").write_text("WORLD\n")

    import pytest
    with pytest.raises(AssertionError, match="differs from golden"):
        assert_md_dir_matches_golden(produced, golden)


def test_golden_helper_raises_on_missing_file(tmp_path):
    from tests.conftest import assert_md_dir_matches_golden

    produced = tmp_path / "produced"
    golden = tmp_path / "golden"
    produced.mkdir()
    golden.mkdir()
    (produced / "a.md").write_text("hi\n")

    import pytest
    with pytest.raises(AssertionError, match="lists differ"):
        assert_md_dir_matches_golden(produced, golden)
```

- [ ] **Step 3: Run + commit**

```sh
uv run pytest tests/unit/test_export_render.py -v
```
Expected: 13 passed (10 prior + 3 new).

```sh
git add prograph/tests/conftest.py prograph/tests/unit/test_export_render.py
git commit -m "prograph: M5 golden test helper (PROGRAPH_UPDATE_GOLDEN=1 to regenerate)"
```

---

## Task 12: Golden tests against all 3 fixtures

**Files:**
- Modify: `tests/integration/test_cli_export_md.py`
- Create: `tests/fixtures/monorepo_full/golden/*.md`
- Create: `tests/fixtures/monorepo_multilang/golden/*.md`
- Create: `tests/fixtures/monorepo_mcp/golden/*.md`

For each fixture, add a test that copies the fixture to `tmp_path`, runs `prograph init && prograph index --export-md`, then asserts the produced `.prograph/` contents match the checked-in `golden/` directory. The first run uses `PROGRAPH_UPDATE_GOLDEN=1` to generate the golden files; subsequent CI runs verify.

- [ ] **Step 1: Add three golden tests**

Append to `tests/integration/test_cli_export_md.py`:
```python
import shutil


def _run_full_export(fixture_name: str, tmp_path: Path) -> Path:
    """Copy fixture into tmp_path, init + index --export-md, return the produced .prograph dir."""
    src = Path(__file__).resolve().parents[1] / "fixtures" / fixture_name
    dst = tmp_path / fixture_name
    shutil.copytree(src, dst, ignore=shutil.ignore_patterns("golden"))
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    result = runner.invoke(app, ["index", "--monorepo", str(dst), "--export-md"])
    assert result.exit_code == 0, result.stdout
    return dst / ".prograph"


def test_golden_monorepo_full(tmp_path: Path, md_matcher):
    produced = _run_full_export("monorepo_full", tmp_path)
    golden = Path(__file__).resolve().parents[1] / "fixtures" / "monorepo_full" / "golden"
    md_matcher(produced, golden)


def test_golden_monorepo_multilang(tmp_path: Path, md_matcher):
    produced = _run_full_export("monorepo_multilang", tmp_path)
    golden = Path(__file__).resolve().parents[1] / "fixtures" / "monorepo_multilang" / "golden"
    md_matcher(produced, golden)


def test_golden_monorepo_mcp(tmp_path: Path, md_matcher):
    produced = _run_full_export("monorepo_mcp", tmp_path)
    golden = Path(__file__).resolve().parents[1] / "fixtures" / "monorepo_mcp" / "golden"
    md_matcher(produced, golden)
```

Note: the `shutil.ignore_patterns("golden")` excludes the `golden/` dir from being copied — otherwise nested goldens would pollute the temp tree.

There's one wrinkle: the `indexed_at` and `snapshot_id` fields in MD frontmatter change with every run. Since two `prograph index` runs on a fresh tmp_path always produce snapshot_id=1 and timestamps within the same second (mostly), this is usually stable — but a slow run that crosses a second boundary can flake.

Mitigation: ALWAYS run the index test in a freshly-copied tmp_path with snapshot_id=1, and accept timestamp drift by normalizing the timestamp line in the helper. Alternatively, expose a `prograph index --ts <ISO8601>` flag for deterministic testing. M5 takes the simpler path: a normalizer.

Update the helper in `tests/conftest.py` to normalize `indexed_at` lines BEFORE comparing:
```python
def _normalize(raw: bytes) -> bytes:
    """Replace timestamp values for byte comparison."""
    import re
    text = raw.decode("utf-8", errors="replace")
    text = re.sub(
        r"indexed_at: \d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z",
        "indexed_at: <ts>",
        text,
    )
    return text.encode("utf-8")
```

And in the comparison:
```python
        p_bytes = _normalize((produced_dir / rel).read_bytes())
        g_bytes = _normalize((golden_dir / rel).read_bytes())
```

- [ ] **Step 2: Generate golden files for all three fixtures**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_full -v
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_multilang -v
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_mcp -v
```

This populates `tests/fixtures/<name>/golden/` with the current MD output. Inspect the generated files — read a few of them to confirm they look reasonable. If a section is wrong (e.g. ordering, intro misses), fix the renderer and regenerate.

- [ ] **Step 3: Verify tests pass without the env var**

```sh
unset PROGRAPH_UPDATE_GOLDEN
uv run pytest tests/integration/test_cli_export_md.py -v
```
Expected: all 10 tests pass (7 prior + 3 new golden tests).

- [ ] **Step 4: Commit**

```sh
git add prograph/tests/conftest.py prograph/tests/integration/test_cli_export_md.py \
        prograph/tests/fixtures/monorepo_full/golden/ \
        prograph/tests/fixtures/monorepo_multilang/golden/ \
        prograph/tests/fixtures/monorepo_mcp/golden/
git commit -m "prograph: M5 golden tests + checked-in expected MD for monorepo_{full,multilang,mcp}"
```

---

## Task 13: Idempotency assertion + reindex stability

**Files:**
- Modify: `tests/integration/test_cli_export_md.py`

Two additional assertions:

1. **Within-snapshot idempotency**: running `prograph export-md` twice on the same snapshot produces byte-identical files (already covered by `test_export_md_idempotent_byte_stable` — verify it still holds).

2. **Cross-snapshot stability**: re-running `prograph index` on the same state (no source changes) produces MD files that are identical EXCEPT for the `snapshot:` and `indexed_at:` frontmatter values. Strip those before comparing.

- [ ] **Step 1: Add cross-snapshot stability test**

Append to `tests/integration/test_cli_export_md.py`:
```python
def test_reindex_md_stable_modulo_timestamps(tmp_path: Path):
    """Re-indexing same state produces MD files identical except for ts/snapshot fields."""
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--export-md"])

    paths = PrographPaths(monorepo_root=tmp_path)
    first = (paths.projects_md_dir / "alpha.md").read_text()

    # Second index — same state, fresh snapshot row.
    runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--export-md"])
    second = (paths.projects_md_dir / "alpha.md").read_text()

    # Strip the per-snapshot frontmatter fields.
    import re
    def normalize(t: str) -> str:
        t = re.sub(r"^indexed_at: .*$", "indexed_at: <ts>", t, flags=re.MULTILINE)
        t = re.sub(r"^snapshot: \d+$", "snapshot: <n>", t, flags=re.MULTILINE)
        return t

    assert normalize(first) == normalize(second), (
        "MD bytes must be stable across reindexes of identical source"
    )
```

- [ ] **Step 2: Run + commit**

```sh
uv run pytest tests/integration/test_cli_export_md.py -v
```
Expected: 11 passed.

```sh
git add prograph/tests/integration/test_cli_export_md.py
git commit -m "prograph: M5 cross-snapshot MD stability (identical bytes modulo ts/snapshot fields)"
```

---

## Task 14: Real-monorepo MD output + smoke

**Files:**
- Modify: `tests/integration/test_smoke_real.py`

The `realmonorepo` smoke test now also runs `prograph index --export-md` against `all_ai_orchestrators/` and asserts that the expected MD files exist. We don't byte-compare — the real monorepo's projects change frequently — but we verify structure.

- [ ] **Step 1: Update the smoke test**

In `tests/integration/test_smoke_real.py`, append to the existing test function (after the existing assertions):

```python
    # M5: also run export-md and verify expected MD files exist.
    md = runner.invoke(app, ["export-md", "--monorepo", str(real)])
    assert md.exit_code == 0, md.stdout

    paths_db = real / ".prograph" / "graph.db"
    if not paths_db.exists():
        return

    projects_md_dir = real / ".prograph" / "projects"
    contracts_md_dir = real / ".prograph" / "contracts"
    index_md = real / ".prograph" / "index.md"

    assert index_md.is_file(), "index.md must be written"
    assert any(projects_md_dir.glob("*.md")), "expected at least one project MD"

    # Spot-check: one of the known projects (Maestro / arbiter / atp-platform) should
    # have an MD card.
    known = {"Maestro", "arbiter", "atp-platform"}
    found = {p.stem for p in projects_md_dir.glob("*.md")}
    assert known & found, f"expected one of {known} in produced MDs, got {found}"

    # Spot-check: if any contract was detected (M4: 2 contract_link edges), there
    # should be at least one contract MD.
    import sqlite3
    conn = sqlite3.connect(paths_db)
    n_contracts = conn.execute(
        "SELECT COUNT(*) FROM contracts WHERE last_seen = (SELECT MAX(id) FROM snapshots)"
    ).fetchone()[0]
    conn.close()
    if n_contracts > 0:
        assert any(contracts_md_dir.glob("*.md")), (
            f"expected ≥1 contract MD given n_contracts={n_contracts}"
        )
```

- [ ] **Step 2: Run + commit**

```sh
uv run pytest -m realmonorepo -v
```
Expected: 1 passed. The real monorepo's `.prograph/{projects,contracts,index.md}` is materialised.

```sh
git add prograph/tests/integration/test_smoke_real.py
git commit -m "prograph: M5 real-monorepo smoke — also exercises 'prograph export-md'"
```

- [ ] **Step 3: Commit the real monorepo's MD files (manual)**

This step is OUTSIDE the test loop. Optionally, after running the smoke, inspect `../.prograph/projects/*.md` and `../.prograph/index.md` and decide whether to commit them at the outer-repo level so PR diffs reflect structural changes going forward.

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators
ls .prograph/projects/ .prograph/contracts/ .prograph/index.md
```

If the output looks reasonable AND you want structural-diff-on-PR behaviour: `git add` the files at the outer repo. **This is a user-discretion step**, not a hard task requirement. Don't auto-commit without inspection.

---

## Task 15: README + CLAUDE.md updates + M5 close

**Files:**
- Modify: `prograph/README.md`
- Modify: `prograph/CLAUDE.md`
- Modify: this plan file

- [ ] **Step 1: Update README**

Replace the Status line:
```markdown
**Status:** M5 — Markdown exporter. `prograph index --export-md` (or `[output] auto_export = true` in `.prograph/config.toml`) writes byte-stable per-project + per-contract + monorepo-level Markdown files to `.prograph/{projects,contracts}/*.md` + `.prograph/index.md`. Files use YAML frontmatter + Obsidian-style `[[wiki-links]]`. Re-rendering the same snapshot is byte-identical (idempotent); re-indexing the same source produces MD differing only in `indexed_at` + `snapshot:` fields. Browser UI + MCP stdio server: M6/M7.
```

Add a new subsection under "Usage":
```markdown
### Markdown export

After `prograph index --export-md` (or with `[output] auto_export = true`), `.prograph/` contains:

- `index.md` — monorepo overview: project list, contract list, recent activity
- `projects/<slug>.md` — one per project, with manifest / public surface / outbound + inbound edges / recent changes
- `contracts/<slug>.md` — one per shared contract, with owner list and provenance

Files are Obsidian-friendly: open `.prograph/` as a vault and follow `[[wiki-links]]` between projects and contracts.

To re-render without re-indexing (after changing a renderer template):

\`\`\`sh
prograph export-md
\`\`\`

To regenerate the test golden files after intentional output changes:

\`\`\`sh
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_full
\`\`\`
```

Update the limitations list:
```markdown
### M5 limitations (intentional — addressed in later milestones)

- **Module-level facts** (public Python symbols, internal imports, public Rust crate items) — they'd live in the "Public surface" MD section alongside MCP tools. Deferred to a later parser-expansion milestone.
- **No "previous snapshot" diff view** in MD — recent_changes lists the last 5 events but doesn't render a structural diff. M6+ can add it.
- **No customisable templates** in M5 — the rendering is hardcoded. Configurable templates are M7+ if/when a user asks.
- **No browser UI** — M6.
- **No MCP stdio server for AI agents** — M7.
```

- [ ] **Step 2: Update CLAUDE.md**

Replace the "Architecture (M4 state)" section header with "Architecture (M5 state)" and update the components list:

```markdown
## Architecture (M5 state)

Two-layer build:

- **`prograph-core` (Rust crate via PyO3):**
  - `discovery` — project classification + monorepo walk (M1)
  - `parsers/python` — `pyproject.toml` + tree-sitter MCP scan (M2-M4)
  - `parsers/rust` — `Cargo.toml` + tree-sitter MCP scan (M3+M4)
  - `parsers/js` — `package.json` parsing (M3)
  - `parsers/contracts` — file-system JSON Schema / OpenAPI / .proto scanner (M4)
  - `detectors/{deps,contracts,mcp}` — three edge-kind detectors (M2-M4)
  - `diff`, `lock`, `indexer` — pipeline (M2-M4)
  - `store` — SQLite schema v4 + `describe_project` / `describe_contract` / `monorepo_overview` aggregations (M5)
  - `models` — pyclasses including aggregation views: `ProjectDescription`, `ContractDescription`, `MonorepoOverview`, `OutboundEdge`, `InboundEdge`, `McpToolDeclRow`, `ContractFileRow`, `RecentChangeRow`, `ProjectSummary`, `ContractSummary` (M5)
  - `facts` — extracted facts including `Manifest`, `McpToolDecl`, `ContractFile` (M2-M4)
  - `migrations/v1.sql..v4.sql` — additive schema chain
- **`prograph` (Python package):**
  - `cli.py` — `init`, `index --export-md`, `status`, `export-md`, `--version`
  - `config.py` — `[output] auto_export` reader (M5)
  - `export/` — Markdown rendering sub-package (M5)
    - `intro.py` — first-paragraph extraction from README/CLAUDE/TODO
    - `render.py` — `render_project` / `render_contract` / `render_index`
    - `slug.py` — filename slugification
    - `__init__.py` — `export_snapshot(monorepo_root)` orchestrator
  - `models.py` — pydantic mirrors of all `_core` data types
  - `paths.py` — `.prograph/` layout helper (includes `index_md_path`, `projects_md_dir`, `contracts_md_dir`)
```

Replace "What is NOT in M4" with:
```markdown
## What is NOT in M5

- Module-level facts (public Python symbols, internal imports, public Rust crate items) — a later parser-expansion milestone.
- Customisable MD templates — M7+ if/when needed.
- Browser UI — M6.
- MCP stdio server for AI agents — M7.

(See `docs/superpowers/plans/` for individual milestone plans.)
```

Add to "Common commands":
```sh
uv run prograph index [--monorepo PATH] [--export-md] [--json]  # index with optional MD export
uv run prograph export-md [--monorepo PATH]                     # re-render MD from latest snapshot
```

Add a new sub-section "Golden tests":
```markdown
### Golden tests

`tests/fixtures/<name>/golden/` directories hold the expected MD output for each fixture. After intentional renderer changes, regenerate with:

\`\`\`sh
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_full
\`\`\`

Then `git diff` to review the change before committing.
```

- [ ] **Step 3: Run the full local gate**

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

Expected: every command exits 0. Cargo ≥119; pytest ≥84; realmonorepo 1.

- [ ] **Step 4: Check the DoD boxes**

Mark every `- [ ]` in "Definition of Done (M5)" as `- [x]` with achieved counts.

- [ ] **Step 5: Final commit**

```sh
git add prograph/README.md prograph/CLAUDE.md \
        prograph/docs/superpowers/plans/2026-05-26-prograph-m5-md-exporter.md
git commit -m "prograph: M5 close — docs updated, full gate green, DoD checked"
```

---

## Definition of Done (M5)

- [x] `cargo test --all-targets` passes (119 tests).
- [x] `uv run pytest -v` passes (89 tests; 1 deselected).
- [x] `uv run pytest -m realmonorepo -v` passes against the real `all_ai_orchestrators/` and produces `.prograph/projects/*.md` + `index.md` (7 projects + 6 contracts).
- [x] Schema v4 (`mcp_tool_decls`) applies cleanly over v3 and preserves existing data.
- [x] `Store::describe_project`, `describe_contract`, `monorepo_overview` return correctly-shaped aggregations matching the spec §5.3 frontmatter + sections.
- [x] PyO3 wrappers `_core.describe_project` / `_core.describe_contract` / `_core.monorepo_overview` exposed and stub-documented in `_core.pyi`.
- [x] Pydantic mirrors round-trip from `_core` types without loss.
- [x] `prograph.export.intro.extract_intro` handles README/CLAUDE/TODO in priority order, strips Markdown emphasis, truncates at 200 chars.
- [x] `prograph.export.slug.slugify` mirrors Rust's `slugify` (alphanumeric + dash + underscore preserved; everything else → `-`; empty → `_unnamed`).
- [x] `prograph.export.render.render_project / render_contract / render_index` produce byte-stable, Obsidian-compatible Markdown.
- [x] `prograph index --export-md` writes the MD tree; `prograph export-md` re-renders without re-indexing.
- [x] `[output] auto_export = true` in `.prograph/config.toml` triggers automatic export on `prograph index` (no flag needed).
- [x] Two consecutive `prograph export-md` runs on the same snapshot produce byte-identical files.
- [x] Re-indexing the same source state produces MD files differing only in `indexed_at:` and `snapshot:` frontmatter fields.
- [x] Golden tests on `monorepo_full`, `monorepo_multilang`, `monorepo_mcp` pass against checked-in `tests/fixtures/<name>/golden/*.md`.
- [x] `PROGRAPH_UPDATE_GOLDEN=1` regenerates the golden directories for intentional changes.
- [x] CI workflow continues to pass with no changes required.
- [x] All commits follow the `prograph: M5 ...` prefix convention.

## What is NOT done in M5 (handled in subsequent milestones)

- **M6** — Browser UI (FastAPI + static + d3/cytoscape) + REST API.
- **M7** — MCP stdio server with `list_projects` / `describe_project` / `find_edges` / `changelog` / `monorepo_overview` tools for AI agents. Configurable detection patterns for arbiter-style MCP idioms.
- **M8** — Module-level facts (public Python symbols / internal imports / public Rust crate items) for richer "Public surface" MD sections.
- **M9+** — JS MCP detection, HTTP/REST runtime edges, workspace auto-discovery, real-monorepo CI matrix, performance baselines, customisable MD templates.
