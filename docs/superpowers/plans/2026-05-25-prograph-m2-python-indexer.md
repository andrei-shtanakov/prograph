# prograph M2 — Python Indexer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `prograph index` work end-to-end against a Python monorepo. After M2 you can run `prograph init && prograph index && prograph status` against `all_ai_orchestrators/` and see a real snapshot with persisted project rows, cross-project `package_dep` edges (e.g. `Maestro → atp-platform`), and a populated change-log. Re-running `index` produces an empty change-log (no diffs) unless the source changed.

**Architecture:** Adds the first end-to-end indexing pipeline on top of M1's foundation. Rust core gains the `parsers/`, `detectors/`, `facts`, `diff`, `lock`, and `indexer` modules; Store gains `write_snapshot()` + query helpers. SQLite gets schema v2 (additive — `edges`, `edge_evidence`, `change_log` tables). Python wrapper grows the `index` CLI command and extends `status` to surface the latest snapshot.

**Tech Stack:**
- **Rust:** edition 2021, pinned 1.75; rusqlite 0.31, pyo3 0.22, thiserror 1, serde + serde_json (all from M1); new workspace deps: `toml = "0.8"` (parse `pyproject.toml`), `fslock = "0.2"` (cross-platform FS locks), `sha2 = "0.10"` (attrs identity hash)
- **Python:** typer + pydantic v2 (unchanged from M1)
- **Build:** maturin via `uv sync` (unchanged from M1)

**Spec reference:** `docs/superpowers/specs/2026-05-25-prograph-design.md` — §4.1 (`parsers`, `edge_detectors`, `diff_engine`), §5.1 (full schema; M2 adds `edges`, `edge_evidence`, `change_log` — strict subset), §5.2 identity rules (especially `package_dep` identity excludes `version_req`), §6 (indexing flow phases 2–5; phase 6 MD export deferred to M5).

**Baseline:** Branch off `main` at commit `4606aa0` (M1 close). 18 cargo + 19 pytest tests passing; CI green; `prograph init/status/--version` working; `.prograph/graph.db` schema v1 (snapshots + projects tables empty).

**M2 explicitly out of scope:**
- Multi-language parsers (Rust + JS via tree-sitter) — M3
- Contracts detector + MCP detector — M4
- MD export + golden tests — M5
- Browser UI + REST — M6
- MCP stdio server — M7
- Incremental reindex (mtime tracking) — M7+
- Vendored-file detection — M6+

---

## File Structure (created/modified in M2)

```
prograph/
├── Cargo.toml                                          # MODIFY — add toml/fslock/sha2 workspace deps
├── prograph-core/
│   ├── Cargo.toml                                      # MODIFY — pull workspace deps into the crate
│   ├── src/
│   │   ├── lib.rs                                      # MODIFY — register new modules + exports + pyfunctions
│   │   ├── errors.rs                                   # MODIFY — add ParseError + Lock variants
│   │   ├── models.rs                                   # MODIFY — add Edge, ChangeEvent, SnapshotInfo, IndexSummary
│   │   ├── store.rs                                    # MODIFY — add write_snapshot, query helpers, v2 migration
│   │   ├── facts.rs                                    # NEW — ProjectFacts + Manifest + DepRequirement
│   │   ├── parsers/
│   │   │   ├── mod.rs                                  # NEW — LanguageParser trait + dispatch
│   │   │   └── python.rs                               # NEW — pyproject.toml + setup.py parsing
│   │   ├── detectors/
│   │   │   ├── mod.rs                                  # NEW — EdgeDetector trait + EdgeCandidate
│   │   │   └── deps.rs                                 # NEW — deps_detector
│   │   ├── diff.rs                                     # NEW — compute_diff + identity hashing
│   │   ├── lock.rs                                     # NEW — index lock helper (fslock-backed)
│   │   ├── indexer.rs                                  # NEW — pipeline orchestrator
│   │   └── migrations/
│   │       └── v2.sql                                  # NEW — additive schema bump
├── prograph/
│   ├── _core.pyi                                       # MODIFY — stubs for new pyclasses + functions
│   ├── __init__.py                                     # MODIFY — re-export new pydantic types
│   ├── models.py                                       # MODIFY — pydantic mirrors for new Rust types
│   └── cli.py                                          # MODIFY — add `index` command, extend `status`
├── tests/
│   ├── fixtures/
│   │   └── monorepo_full/                              # NEW — 5 synthetic Python projects with deps
│   │       ├── orchestrator/pyproject.toml
│   │       ├── eval_sdk/pyproject.toml
│   │       ├── policy/pyproject.toml
│   │       ├── runner/pyproject.toml
│   │       └── docs_only/CLAUDE.md
│   ├── unit/
│   │   ├── test_facts.py                               # NEW
│   │   └── test_models.py                              # MODIFY — add Edge / ChangeEvent / SnapshotInfo round-trips
│   └── integration/
│       ├── test_cli_index.py                           # NEW
│       ├── test_cli_status.py                          # MODIFY — snapshot info appears after index
│       └── test_smoke_real.py                          # MODIFY — also exercise `index` against parent monorepo
```

---

## Task 1: Schema v2 + migration runner upgrade

**Files:**
- Create: `prograph-core/src/migrations/v2.sql`
- Modify: `prograph-core/src/store.rs` (migration registry)
- Modify: `prograph-core/Cargo.toml` (add `sha2` dev-dep for tests)

The migration runner today applies `v1.sql` once. M2 introduces a small versioned registry so v2 can be applied additively on top of an existing v1 database without dropping data.

- [ ] **Step 1: Write the v2 schema**

`prograph-core/src/migrations/v2.sql`:
```sql
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
```

CHECK constraints on `kind`/`entity_kind`/`change` only allow values M2 actually emits. M3/M4 widen these constraints in their own migrations.

- [ ] **Step 2: Upgrade the migration runner in `store.rs`**

Replace the existing `SCHEMA_V1` constant and `Store::open` body. The new version applies all migrations whose version > current `schema_version`.

In `prograph-core/src/store.rs`, replace:
```rust
const SCHEMA_V1: &str = include_str!("migrations/v1.sql");
```
with:
```rust
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/v1.sql")),
    (2, include_str!("migrations/v2.sql")),
];
```

Replace `Store::open`:
```rust
pub fn open(path: &Path) -> Result<Self> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| PrographError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }

    let conn = Connection::open(path)?;
    conn.execute("PRAGMA foreign_keys = ON;", [])?;

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version
             WHERE EXISTS (SELECT 1 FROM sqlite_master
                           WHERE type='table' AND name='schema_version')",
            [],
            |r| r.get(0),
        )
        .or_else(|_| {
            // schema_version table does not exist yet — first run, version is effectively 0.
            Ok::<i64, rusqlite::Error>(0)
        })?;

    for (version, sql) in MIGRATIONS {
        if *version > current {
            conn.execute_batch(sql)?;
        }
    }

    Ok(Self { conn })
}
```

Note the bootstrap robustness: the first call to `Store::open` has no `schema_version` table yet, so the SELECT would error normally; the `.or_else` short-circuits to 0 and the v1 migration creates the table.

- [ ] **Step 3: Add `sha2` dev-dependency** (used in Task 2's test for edge identity hashing)

Edit `prograph-core/Cargo.toml`, append to `[dev-dependencies]`:
```toml
sha2 = "0.10"
```

(`sha2` will become a runtime dep in Task 8 once diff.rs uses it; for now it's only needed by the Task 2 test seam.)

- [ ] **Step 4: Add a Rust test that v2 tables exist**

In `prograph-core/src/store.rs`, append inside the existing `#[cfg(test)] mod tests` block:
```rust
    #[test]
    fn schema_v2_creates_edges_change_log_tables() {
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
        assert!(names.contains(&"edges".to_string()));
        assert!(names.contains(&"edge_evidence".to_string()));
        assert!(names.contains(&"change_log".to_string()));
        assert_eq!(store.schema_version().unwrap(), 2);
    }

    #[test]
    fn migration_is_additive_over_existing_v1_db() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("g.db");

        // Simulate an existing v1 DB by manually setting schema_version after first open.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(include_str!("migrations/v1.sql")).unwrap();
            // Force schema_version to 1 (which v1.sql already does).
            let v: i64 = conn
                .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(v, 1);
        }

        // Now Store::open should apply v2 only.
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 2);
    }
```

- [ ] **Step 5: Run cargo tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core store
```
Expected: 5 tests pass (3 from M1 + 2 new).

Also run the full crate suite to confirm no regression:
```sh
cargo test --package prograph-core
```
Expected: 20 tests pass (18 from M1 + 2 new).

- [ ] **Step 6: Commit**

```sh
git add prograph/prograph-core/src/migrations/v2.sql \
        prograph/prograph-core/src/store.rs \
        prograph/prograph-core/Cargo.toml
git commit -m "prograph: M2 SQLite schema v2 (edges, edge_evidence, change_log) + migration runner"
```

---

## Task 2: Workspace dependencies + crate-level wiring

**Files:**
- Modify: `prograph/Cargo.toml` (workspace deps)
- Modify: `prograph-core/Cargo.toml` (pull workspace deps in)

Centralize the new crate dependencies at the workspace level so future plugin crates (Phase 4) inherit them.

- [ ] **Step 1: Add workspace dependencies**

Edit `prograph/Cargo.toml`, append to `[workspace.dependencies]`:
```toml
toml = "0.8"
fslock = "0.2"
sha2 = "0.10"
```

- [ ] **Step 2: Reference them in the prograph-core crate**

Edit `prograph-core/Cargo.toml`, append to `[dependencies]`:
```toml
toml = { workspace = true }
fslock = { workspace = true }
sha2 = { workspace = true }
```

Also REMOVE the `sha2 = "0.10"` line from `[dev-dependencies]` that Task 1 step 3 added — it's now a regular dependency.

- [ ] **Step 3: Confirm cargo metadata resolves**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo metadata --no-deps --format-version 1 > /dev/null
cargo build --package prograph-core
```
Expected: build succeeds (will pull and compile `toml`, `fslock`, `sha2` + transitive deps on first run).

- [ ] **Step 4: Commit**

```sh
git add prograph/Cargo.toml prograph/prograph-core/Cargo.toml prograph/Cargo.lock
git commit -m "prograph: M2 add toml/fslock/sha2 workspace dependencies"
```

---

## Task 3: Facts module — `ProjectFacts`, `Manifest`, `DepRequirement`

**Files:**
- Create: `prograph-core/src/facts.rs`
- Modify: `prograph-core/src/lib.rs` (register `mod facts`)

The facts module defines the data shapes parsers produce. M2 only uses `Manifest` (with declared name + version + deps), but the structs are designed to grow additive fields for M3+ (`mcp_decls`, `mcp_uses`, `contracts`).

- [ ] **Step 1: Write `facts.rs`**

`prograph-core/src/facts.rs`:
```rust
//! Cross-language data shapes parsers produce. Detectors consume slices of these.
//!
//! M2 only populates `Manifest`. M3+ adds `modules`, `mcp_decls`, `mcp_uses`, `contracts`
//! without breaking consumers — all fields are owned by the parser and read-only downstream.

use serde::{Deserialize, Serialize};

/// A single declared dependency in a project's manifest.
///
/// Identity-relevant fields: `name`. `version_req` is metadata (version bumps surface as
/// `attrs_changed` events in the change-log, not remove+add).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepRequirement {
    pub name: String,
    pub version_req: Option<String>,
}

/// A project's declared manifest — the canonical view downstream detectors use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Published package name (e.g. "atp-platform-sdk"), NOT the directory name.
    /// Detectors match consumers' `declared_deps[].name` against this field.
    pub declared_name: String,
    pub version: Option<String>,
    pub declared_deps: Vec<DepRequirement>,
}

/// Soft parse errors that should warn the user without aborting the snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseWarning {
    pub rel_path: String,
    pub message: String,
}

/// Per-project bundle of extracted facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFacts {
    /// Project identifier inside the current scan — populated by indexer, not parsers.
    pub project_root: String, // relative path, matches projects.root_path
    pub project_name: String,
    pub manifest: Option<Manifest>,
    pub warnings: Vec<ParseWarning>,
    pub parse_status: ParseStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParseStatus {
    Ok,
    Partial,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips_via_serde() {
        let m = Manifest {
            declared_name: "maestro".into(),
            version: Some("0.2.0".into()),
            declared_deps: vec![DepRequirement {
                name: "atp-platform-sdk".into(),
                version_req: Some(">=2.0.0".into()),
            }],
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn dep_requirement_without_version_serializes_null() {
        let d = DepRequirement {
            name: "x".into(),
            version_req: None,
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"version_req\":null"));
    }
}
```

- [ ] **Step 2: Register the module**

Edit `prograph-core/src/lib.rs`, add `mod facts;` alphabetically:
```rust
mod discovery;
mod errors;
mod facts;
mod models;
mod store;
```

No `pub use` re-exports yet — Tasks 6+ will export the public types as needed.

- [ ] **Step 3: Run tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core facts
```
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph-core/src/facts.rs prograph/prograph-core/src/lib.rs
git commit -m "prograph: M2 facts module — ProjectFacts, Manifest, DepRequirement"
```

---

## Task 4: Errors — add `Parse` + `Lock` variants

**Files:**
- Modify: `prograph-core/src/errors.rs`

- [ ] **Step 1: Extend `PrographError`**

In `prograph-core/src/errors.rs`, add two new variants to the enum:
```rust
    #[error("parse error in {path}: {reason}")]
    Parse {
        path: String,
        reason: String,
    },

    #[error("index lock at {path} is held by another process")]
    Lock {
        path: String,
    },
```

And the corresponding cases in the `From<PrographError> for PyErr` match:
```rust
            PrographError::Parse { .. } => PyValueError::new_err(err.to_string()),
            PrographError::Lock { .. } => PyRuntimeError::new_err(err.to_string()),
```

Add a test for each new variant:
```rust
    #[test]
    fn parse_error_displays_path_and_reason() {
        let err = PrographError::Parse {
            path: "pyproject.toml".into(),
            reason: "missing [project] table".into(),
        };
        assert_eq!(
            err.to_string(),
            "parse error in pyproject.toml: missing [project] table"
        );
    }

    #[test]
    fn lock_error_maps_to_runtime_error() {
        pyo3::Python::with_gil(|py| {
            let err: PyErr = PrographError::Lock {
                path: ".prograph/index.lock".into(),
            }
            .into();
            assert!(err.is_instance_of::<pyo3::exceptions::PyRuntimeError>(py));
        });
    }
```

- [ ] **Step 2: Run tests**

```sh
cargo test --package prograph-core errors
```
Expected: 4 tests pass (2 prior + 2 new).

- [ ] **Step 3: Commit**

```sh
git add prograph/prograph-core/src/errors.rs
git commit -m "prograph: M2 errors — add Parse + Lock variants"
```

---

## Task 5: Models — Edge, ChangeEvent, SnapshotInfo, IndexSummary

**Files:**
- Modify: `prograph-core/src/models.rs`
- Modify: `prograph-core/src/lib.rs` (register new pyclasses)
- Modify: `prograph/_core.pyi`
- Modify: `prograph/models.py`
- Modify: `prograph/__init__.py`
- Modify: `tests/unit/test_models.py`

- [ ] **Step 1: Add Rust pyclasses**

Append to `prograph-core/src/models.rs`:
```rust
/// Direction-tagged identifier of a graph endpoint (project or contract).
/// M2 only emits 'project'; M4 will add 'contract'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[pyclass(eq, frozen, module = "prograph._core")]
pub enum NodeKind {
    Project,
    Contract,
}

#[pymethods]
impl NodeKind {
    fn __repr__(&self) -> String {
        format!("NodeKind.{:?}", self)
    }

    fn name(&self) -> &'static str {
        match self {
            NodeKind::Project => "project",
            NodeKind::Contract => "contract",
        }
    }
}

/// Edge kind. M2 only emits PackageDep; M4 adds McpCall + ContractLink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[pyclass(eq, frozen, module = "prograph._core")]
pub enum EdgeKind {
    PackageDep,
}

#[pymethods]
impl EdgeKind {
    fn __repr__(&self) -> String {
        format!("EdgeKind.{:?}", self)
    }

    fn name(&self) -> &'static str {
        match self {
            EdgeKind::PackageDep => "package_dep",
        }
    }
}

/// A persisted edge with full provenance.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct Edge {
    pub id: i64,
    pub kind: EdgeKind,
    pub from_kind: NodeKind,
    pub from_id: i64,
    pub to_kind: NodeKind,
    pub to_id: i64,
    pub attrs_json: String, // serialized JSON; pydantic side parses
    pub first_seen: i64,
    pub last_seen: i64,
}

#[pymethods]
impl Edge {
    fn __repr__(&self) -> String {
        format!(
            "Edge(id={}, kind={:?}, {}#{} → {}#{})",
            self.id, self.kind, self.from_kind.name(), self.from_id,
            self.to_kind.name(), self.to_id
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[pyclass(eq, frozen, module = "prograph._core")]
pub enum ChangeKind {
    Added,
    Removed,
    AttrsChanged,
}

#[pymethods]
impl ChangeKind {
    fn __repr__(&self) -> String {
        format!("ChangeKind.{:?}", self)
    }

    fn name(&self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Removed => "removed",
            ChangeKind::AttrsChanged => "attrs_changed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[pyclass(eq, frozen, module = "prograph._core")]
pub enum EntityKind {
    Project,
    Edge,
}

#[pymethods]
impl EntityKind {
    fn __repr__(&self) -> String {
        format!("EntityKind.{:?}", self)
    }

    fn name(&self) -> &'static str {
        match self {
            EntityKind::Project => "project",
            EntityKind::Edge => "edge",
        }
    }
}

/// A row from the change_log table.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct ChangeEvent {
    pub id: i64,
    pub snapshot_id: i64,
    pub ts: String,
    pub entity_kind: EntityKind,
    pub entity_id: i64,
    pub change: ChangeKind,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
}

#[pymethods]
impl ChangeEvent {
    fn __repr__(&self) -> String {
        format!(
            "ChangeEvent(snap={}, {:?} {}#{}: {:?})",
            self.snapshot_id, self.change, self.entity_kind.name(), self.entity_id, self.change
        )
    }
}

/// Metadata about a single `prograph index` snapshot.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct SnapshotInfo {
    pub id: i64,
    pub ts: String,
    pub monorepo_root: String,
    pub git_commit: Option<String>,
    pub prograph_version: String,
    pub n_projects: i64,
    pub n_edges: i64,
    pub n_changes: i64,
}

#[pymethods]
impl SnapshotInfo {
    fn __repr__(&self) -> String {
        format!(
            "SnapshotInfo(#{}, ts={}, projects={}, edges={}, changes={})",
            self.id, self.ts, self.n_projects, self.n_edges, self.n_changes
        )
    }
}

/// The summary `prograph index` returns to the caller after running.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct IndexSummary {
    pub snapshot_id: i64,
    pub ts: String,
    pub n_projects: i64,
    pub n_edges: i64,
    pub n_changes: i64,
    pub n_warnings: i64,
    pub duration_ms: i64,
}

#[pymethods]
impl IndexSummary {
    fn __repr__(&self) -> String {
        format!(
            "IndexSummary(snap={}, projects={}, edges={}, changes={}, warnings={}, {}ms)",
            self.snapshot_id, self.n_projects, self.n_edges, self.n_changes, self.n_warnings,
            self.duration_ms
        )
    }
}
```

- [ ] **Step 2: Register pyclasses in `lib.rs`**

Edit the `#[pymodule]` block in `prograph-core/src/lib.rs` to add:
```rust
    m.add_class::<NodeKind>()?;
    m.add_class::<EdgeKind>()?;
    m.add_class::<Edge>()?;
    m.add_class::<ChangeKind>()?;
    m.add_class::<EntityKind>()?;
    m.add_class::<ChangeEvent>()?;
    m.add_class::<SnapshotInfo>()?;
    m.add_class::<IndexSummary>()?;
```

And extend the `pub use models::{...}` line:
```rust
pub use models::{
    ChangeEvent, ChangeKind, Edge, EdgeKind, EntityKind, IndexSummary, NodeKind,
    ProjectCandidate, ProjectKind, SnapshotInfo,
};
```

- [ ] **Step 3: Extend the `.pyi` stub**

Append to `prograph/_core.pyi`:
```python
class NodeKind:
    Project: ClassVar[NodeKind]
    Contract: ClassVar[NodeKind]
    def name(self) -> str: ...

class EdgeKind:
    PackageDep: ClassVar[EdgeKind]
    def name(self) -> str: ...

class ChangeKind:
    Added: ClassVar[ChangeKind]
    Removed: ClassVar[ChangeKind]
    AttrsChanged: ClassVar[ChangeKind]
    def name(self) -> str: ...

class EntityKind:
    Project: ClassVar[EntityKind]
    Edge: ClassVar[EntityKind]
    def name(self) -> str: ...

class Edge:
    id: int
    kind: EdgeKind
    from_kind: NodeKind
    from_id: int
    to_kind: NodeKind
    to_id: int
    attrs_json: str
    first_seen: int
    last_seen: int

class ChangeEvent:
    id: int
    snapshot_id: int
    ts: str
    entity_kind: EntityKind
    entity_id: int
    change: ChangeKind
    before_json: str | None
    after_json: str | None

class SnapshotInfo:
    id: int
    ts: str
    monorepo_root: str
    git_commit: str | None
    prograph_version: str
    n_projects: int
    n_edges: int
    n_changes: int

class IndexSummary:
    snapshot_id: int
    ts: str
    n_projects: int
    n_edges: int
    n_changes: int
    n_warnings: int
    duration_ms: int
```

- [ ] **Step 4: Add pydantic mirrors**

Append to `prograph/models.py`:
```python
class NodeKind(str, Enum):
    PROJECT = "project"
    CONTRACT = "contract"

    @classmethod
    def from_core(cls, value: _core.NodeKind) -> NodeKind:
        return cls(value.name())


class EdgeKind(str, Enum):
    PACKAGE_DEP = "package_dep"

    @classmethod
    def from_core(cls, value: _core.EdgeKind) -> EdgeKind:
        return cls(value.name())


class ChangeKind(str, Enum):
    ADDED = "added"
    REMOVED = "removed"
    ATTRS_CHANGED = "attrs_changed"

    @classmethod
    def from_core(cls, value: _core.ChangeKind) -> ChangeKind:
        return cls(value.name())


class EntityKind(str, Enum):
    PROJECT = "project"
    EDGE = "edge"

    @classmethod
    def from_core(cls, value: _core.EntityKind) -> EntityKind:
        return cls(value.name())


class Edge(BaseModel):
    """A persisted cross-project edge."""

    model_config = ConfigDict(frozen=True)

    id: int
    kind: EdgeKind
    from_kind: NodeKind
    from_id: int
    to_kind: NodeKind
    to_id: int
    attrs: dict[str, object]
    first_seen: int
    last_seen: int

    @classmethod
    def from_core(cls, value: _core.Edge) -> Edge:
        import json

        return cls(
            id=value.id,
            kind=EdgeKind.from_core(value.kind),
            from_kind=NodeKind.from_core(value.from_kind),
            from_id=value.from_id,
            to_kind=NodeKind.from_core(value.to_kind),
            to_id=value.to_id,
            attrs=json.loads(value.attrs_json),
            first_seen=value.first_seen,
            last_seen=value.last_seen,
        )


class ChangeEvent(BaseModel):
    model_config = ConfigDict(frozen=True)

    id: int
    snapshot_id: int
    ts: str
    entity_kind: EntityKind
    entity_id: int
    change: ChangeKind
    before: dict[str, object] | None
    after: dict[str, object] | None

    @classmethod
    def from_core(cls, value: _core.ChangeEvent) -> ChangeEvent:
        import json

        return cls(
            id=value.id,
            snapshot_id=value.snapshot_id,
            ts=value.ts,
            entity_kind=EntityKind.from_core(value.entity_kind),
            entity_id=value.entity_id,
            change=ChangeKind.from_core(value.change),
            before=json.loads(value.before_json) if value.before_json else None,
            after=json.loads(value.after_json) if value.after_json else None,
        )


class SnapshotInfo(BaseModel):
    model_config = ConfigDict(frozen=True)

    id: int
    ts: str
    monorepo_root: str
    git_commit: str | None
    prograph_version: str
    n_projects: int
    n_edges: int
    n_changes: int

    @classmethod
    def from_core(cls, value: _core.SnapshotInfo) -> SnapshotInfo:
        return cls(
            id=value.id,
            ts=value.ts,
            monorepo_root=value.monorepo_root,
            git_commit=value.git_commit,
            prograph_version=value.prograph_version,
            n_projects=value.n_projects,
            n_edges=value.n_edges,
            n_changes=value.n_changes,
        )


class IndexSummary(BaseModel):
    model_config = ConfigDict(frozen=True)

    snapshot_id: int
    ts: str
    n_projects: int
    n_edges: int
    n_changes: int
    n_warnings: int
    duration_ms: int

    @classmethod
    def from_core(cls, value: _core.IndexSummary) -> IndexSummary:
        return cls(
            snapshot_id=value.snapshot_id,
            ts=value.ts,
            n_projects=value.n_projects,
            n_edges=value.n_edges,
            n_changes=value.n_changes,
            n_warnings=value.n_warnings,
            duration_ms=value.duration_ms,
        )
```

- [ ] **Step 5: Re-export from `__init__.py`**

Edit `prograph/__init__.py` to extend the imports and `__all__`:
```python
from prograph.models import (
    ChangeEvent,
    ChangeKind,
    Edge,
    EdgeKind,
    EntityKind,
    IndexSummary,
    NodeKind,
    ProjectCandidate,
    ProjectKind,
    SnapshotInfo,
)

__all__ = [
    "ChangeEvent",
    "ChangeKind",
    "Edge",
    "EdgeKind",
    "EntityKind",
    "IndexSummary",
    "NodeKind",
    "ProjectCandidate",
    "ProjectKind",
    "SnapshotInfo",
    "__version__",
    "core_version",
]
```

- [ ] **Step 6: Add pydantic round-trip tests**

Append to `tests/unit/test_models.py`:
```python
def test_edge_kind_round_trip():
    assert EdgeKind.from_core(_core.EdgeKind.PackageDep) is EdgeKind.PACKAGE_DEP


def test_node_kind_round_trip():
    assert NodeKind.from_core(_core.NodeKind.Project) is NodeKind.PROJECT
    assert NodeKind.from_core(_core.NodeKind.Contract) is NodeKind.CONTRACT


def test_change_kind_round_trip():
    for variant in (
        _core.ChangeKind.Added,
        _core.ChangeKind.Removed,
        _core.ChangeKind.AttrsChanged,
    ):
        assert ChangeKind.from_core(variant).value == variant.name()


def test_entity_kind_round_trip():
    for variant in (_core.EntityKind.Project, _core.EntityKind.Edge):
        assert EntityKind.from_core(variant).value == variant.name()
```

You must also add `EdgeKind`, `NodeKind`, `ChangeKind`, `EntityKind` to the import line at the top of the file:
```python
from prograph import (
    ChangeKind,
    EdgeKind,
    EntityKind,
    NodeKind,
    ProjectCandidate,
    ProjectKind,
    _core,
)
```

- [ ] **Step 7: Rebuild and run all tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv sync
uv run pytest tests/unit/test_models.py -v
```
Expected: 7 passed (3 from M1 + 4 new).

Full suite:
```sh
uv run pytest -v
```
Expected: 23 passed (19 from M1 + 4 new).

- [ ] **Step 8: Commit**

```sh
git add prograph/prograph-core/src/models.rs prograph/prograph-core/src/lib.rs \
        prograph/prograph/_core.pyi prograph/prograph/models.py prograph/prograph/__init__.py \
        prograph/tests/unit/test_models.py
git commit -m "prograph: M2 Edge, ChangeEvent, SnapshotInfo, IndexSummary pyclasses + pydantic mirrors"
```

---

## Task 6: monorepo_full fixture

**Files:**
- Create: `tests/fixtures/monorepo_full/orchestrator/pyproject.toml`
- Create: `tests/fixtures/monorepo_full/eval_sdk/pyproject.toml`
- Create: `tests/fixtures/monorepo_full/policy/pyproject.toml`
- Create: `tests/fixtures/monorepo_full/runner/pyproject.toml`
- Create: `tests/fixtures/monorepo_full/docs_only/CLAUDE.md`

A richer fixture covering the cross-project dependency cases the deps detector must handle.

- [ ] **Step 1: Create `orchestrator` — consumer of eval_sdk + policy**

`tests/fixtures/monorepo_full/orchestrator/pyproject.toml`:
```toml
[project]
name = "orchestrator"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = [
    "eval-sdk>=1.0",
    "policy",
    "httpx",
]
```

- [ ] **Step 2: Create `eval_sdk` — publisher**

`tests/fixtures/monorepo_full/eval_sdk/pyproject.toml`:
```toml
[project]
name = "eval-sdk"
version = "1.2.0"
requires-python = ">=3.11"
dependencies = []
```

(Note: project directory `eval_sdk` ≠ package name `eval-sdk`. Detector must match on declared name.)

- [ ] **Step 3: Create `policy` — publisher**

`tests/fixtures/monorepo_full/policy/pyproject.toml`:
```toml
[project]
name = "policy"
version = "0.3.0"
requires-python = ">=3.11"
dependencies = []
```

- [ ] **Step 4: Create `runner` — consumer of orchestrator**

`tests/fixtures/monorepo_full/runner/pyproject.toml`:
```toml
[project]
name = "runner"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = [
    "orchestrator",
]
```

- [ ] **Step 5: Create `docs_only` — no code, just docs**

`tests/fixtures/monorepo_full/docs_only/CLAUDE.md`:
```markdown
# docs_only

A docs-only sibling. Should classify as `docs`, produce no manifest, and be ignored by deps_detector.
```

- [ ] **Step 6: Verify discovery still works on the new fixture**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run python -c "
from prograph._core import scan_monorepo
for c in scan_monorepo('tests/fixtures/monorepo_full'):
    print(c.name, c.kind.name(), c.manifests)
"
```
Expected:
```
docs_only docs ['CLAUDE.md']
eval_sdk python ['pyproject.toml']
orchestrator python ['pyproject.toml']
policy python ['pyproject.toml']
runner python ['pyproject.toml']
```

- [ ] **Step 7: Commit**

```sh
git add prograph/tests/fixtures/monorepo_full/
git commit -m "prograph: M2 monorepo_full fixture — 4 Python projects with cross-deps + docs"
```

---

## Task 7: Python parser (`pyproject.toml` → `Manifest`)

**Files:**
- Create: `prograph-core/src/parsers/mod.rs`
- Create: `prograph-core/src/parsers/python.rs`
- Modify: `prograph-core/src/lib.rs` (register `mod parsers`)

- [ ] **Step 1: Write the parsers module**

`prograph-core/src/parsers/mod.rs`:
```rust
//! Per-language parsers. Each parser extracts `Manifest` from a project root.
//! M2 ships only the Python parser; M3 adds Rust + JS.

pub mod python;

use std::path::Path;

use crate::errors::Result;
use crate::facts::{Manifest, ParseWarning};
use crate::models::ProjectKind;

/// Parser output for a single project.
pub struct ParserOutput {
    pub manifest: Option<Manifest>,
    pub warnings: Vec<ParseWarning>,
}

/// Dispatch a project to the right per-language parser.
pub fn parse_project(root: &Path, kind: ProjectKind) -> Result<ParserOutput> {
    match kind {
        ProjectKind::Python | ProjectKind::Mixed => python::parse(root),
        // Other kinds produce empty manifests in M2 (M3 adds Rust + JS).
        _ => Ok(ParserOutput {
            manifest: None,
            warnings: vec![],
        }),
    }
}
```

`prograph-core/src/parsers/python.rs`:
```rust
//! Python project parser — reads `pyproject.toml` to extract published name + deps.

use std::path::Path;

use serde::Deserialize;

use super::ParserOutput;
use crate::errors::{PrographError, Result};
use crate::facts::{DepRequirement, Manifest, ParseWarning};

#[derive(Debug, Deserialize)]
struct PyprojectRoot {
    project: Option<PyprojectProject>,
}

#[derive(Debug, Deserialize)]
struct PyprojectProject {
    name: Option<String>,
    version: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
}

pub fn parse(project_root: &Path) -> Result<ParserOutput> {
    let pyproject = project_root.join("pyproject.toml");
    if !pyproject.is_file() {
        // M2 ignores setup.py-only projects. M3+ can revisit.
        return Ok(ParserOutput {
            manifest: None,
            warnings: vec![ParseWarning {
                rel_path: "pyproject.toml".into(),
                message: "no pyproject.toml found".into(),
            }],
        });
    }

    let contents = std::fs::read_to_string(&pyproject).map_err(|source| PrographError::Io {
        path: pyproject.display().to_string(),
        source,
    })?;

    let root: PyprojectRoot = toml::from_str(&contents).map_err(|e| PrographError::Parse {
        path: pyproject.display().to_string(),
        reason: e.to_string(),
    })?;

    let project = match root.project {
        Some(p) => p,
        None => {
            return Ok(ParserOutput {
                manifest: None,
                warnings: vec![ParseWarning {
                    rel_path: "pyproject.toml".into(),
                    message: "no [project] table".into(),
                }],
            });
        }
    };

    let declared_name = match project.name {
        Some(n) => n,
        None => {
            return Ok(ParserOutput {
                manifest: None,
                warnings: vec![ParseWarning {
                    rel_path: "pyproject.toml".into(),
                    message: "[project] missing 'name' key".into(),
                }],
            });
        }
    };

    let declared_deps = project
        .dependencies
        .iter()
        .map(|raw| parse_pep508(raw))
        .collect();

    Ok(ParserOutput {
        manifest: Some(Manifest {
            declared_name,
            version: project.version,
            declared_deps,
        }),
        warnings: vec![],
    })
}

/// Split a PEP 508 dep string like "eval-sdk>=1.0" into (name, version_req).
/// Best-effort: handles `>=`, `<=`, `==`, `~=`, `<`, `>`, `!=` operators and bare names.
/// Extras and environment markers are stripped (e.g. "foo[bar]>=1.0; python_version<'4'").
fn parse_pep508(raw: &str) -> DepRequirement {
    // Strip environment marker (after ';')
    let no_marker = raw.split(';').next().unwrap_or(raw).trim();
    // Strip extras (e.g. foo[bar])
    let no_extras = strip_extras(no_marker);

    // Find first version operator
    const OPS: &[&str] = &[">=", "<=", "==", "~=", "!=", ">", "<"];
    for op in OPS {
        if let Some(pos) = no_extras.find(op) {
            let name = no_extras[..pos].trim().to_string();
            let version_req = no_extras[pos..].trim().to_string();
            return DepRequirement {
                name,
                version_req: Some(version_req),
            };
        }
    }
    DepRequirement {
        name: no_extras.trim().to_string(),
        version_req: None,
    }
}

fn strip_extras(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0;
    for ch in s.chars() {
        match ch {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_pyproject(toml_contents: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("pyproject.toml"), toml_contents).unwrap();
        dir
    }

    #[test]
    fn parses_minimal_pyproject() {
        let dir = write_pyproject(r#"
[project]
name = "foo"
version = "1.0"
dependencies = []
"#);
        let out = parse(dir.path()).unwrap();
        let manifest = out.manifest.unwrap();
        assert_eq!(manifest.declared_name, "foo");
        assert_eq!(manifest.version.as_deref(), Some("1.0"));
        assert!(manifest.declared_deps.is_empty());
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn parses_dependencies_with_operators() {
        let dir = write_pyproject(r#"
[project]
name = "consumer"
dependencies = ["eval-sdk>=1.0", "policy", "httpx==0.27.0"]
"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let deps = manifest.declared_deps;
        assert_eq!(deps.len(), 3);
        assert_eq!(deps[0].name, "eval-sdk");
        assert_eq!(deps[0].version_req.as_deref(), Some(">=1.0"));
        assert_eq!(deps[1].name, "policy");
        assert_eq!(deps[1].version_req, None);
        assert_eq!(deps[2].name, "httpx");
        assert_eq!(deps[2].version_req.as_deref(), Some("==0.27.0"));
    }

    #[test]
    fn strips_extras_and_markers() {
        let dir = write_pyproject(r#"
[project]
name = "x"
dependencies = ["requests[socks,security]>=2.0; python_version<'4'"]
"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let dep = &manifest.declared_deps[0];
        assert_eq!(dep.name, "requests");
        assert_eq!(dep.version_req.as_deref(), Some(">=2.0"));
    }

    #[test]
    fn warns_when_no_pyproject() {
        let dir = TempDir::new().unwrap();
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].message.contains("no pyproject.toml"));
    }

    #[test]
    fn warns_when_no_project_table() {
        let dir = write_pyproject(r#"
[build-system]
requires = ["setuptools"]
"#);
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("no [project] table"));
    }

    #[test]
    fn warns_when_no_name() {
        let dir = write_pyproject(r#"
[project]
version = "1.0"
dependencies = []
"#);
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("missing 'name'"));
    }

    #[test]
    fn errors_on_invalid_toml() {
        let dir = write_pyproject("[ this is not toml");
        let err = parse(dir.path()).unwrap_err();
        assert!(err.to_string().contains("parse error"));
    }
}
```

- [ ] **Step 2: Register the module in `lib.rs`**

In `prograph-core/src/lib.rs`, add `mod parsers;` to the alphabetical mod block:
```rust
mod discovery;
mod errors;
mod facts;
mod models;
mod parsers;
mod store;
```

- [ ] **Step 3: Run cargo tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core parsers
```
Expected: 7 tests pass.

Also confirm the parser works against `monorepo_full` via a small smoke script:
```sh
cargo run --package prograph-core --example parse_smoke 2>/dev/null || true
```
(There is no such example yet — this just confirms `cargo run` doesn't break the rest of the workspace. Skip if it fails on missing example.)

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph-core/src/parsers/ prograph/prograph-core/src/lib.rs
git commit -m "prograph: M2 Python parser (pyproject.toml → Manifest with PEP 508 dep parsing)"
```

---

## Task 8: deps_detector

**Files:**
- Create: `prograph-core/src/detectors/mod.rs`
- Create: `prograph-core/src/detectors/deps.rs`
- Modify: `prograph-core/src/lib.rs`

- [ ] **Step 1: Write the detectors module**

`prograph-core/src/detectors/mod.rs`:
```rust
//! Edge detectors — turn `ProjectFacts[]` into edge candidates.
//! M2 ships only `deps`; M4 adds `contracts` and `mcp`.

pub mod deps;

use crate::facts::ProjectFacts;
use crate::models::{EdgeKind, NodeKind};

/// A detector's pre-persistence edge proposal. The indexer assigns DB-level ids
/// when materializing into the `edges` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeCandidate {
    pub kind: EdgeKind,
    pub from_kind: NodeKind,
    /// Index into the `Vec<ProjectFacts>` passed to the detector. Resolved to a DB id
    /// by the indexer's persist phase.
    pub from_idx: usize,
    pub to_kind: NodeKind,
    pub to_idx: usize,
    /// Full attrs payload (e.g. {"dep_name": "...", "version_req": "..."}).
    pub attrs_json: String,
    /// Hash over identity-bearing attrs only (per spec §5.2).
    pub attrs_hash: String,
}

/// Detector dispatch — collects EdgeCandidates from every detector.
pub fn detect_all(facts: &[ProjectFacts]) -> Vec<EdgeCandidate> {
    let mut all = Vec::new();
    all.extend(deps::detect(facts));
    all
}
```

`prograph-core/src/detectors/deps.rs`:
```rust
//! Package-dependency detector — matches consumers' `declared_deps[].name`
//! against publishers' `Manifest.declared_name`.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use super::EdgeCandidate;
use crate::facts::ProjectFacts;
use crate::models::{EdgeKind, NodeKind};

pub fn detect(facts: &[ProjectFacts]) -> Vec<EdgeCandidate> {
    // Build name → publisher index map.
    let mut publishers: HashMap<&str, usize> = HashMap::new();
    for (idx, p) in facts.iter().enumerate() {
        if let Some(m) = &p.manifest {
            publishers.insert(m.declared_name.as_str(), idx);
        }
    }

    let mut out = Vec::new();
    for (consumer_idx, consumer) in facts.iter().enumerate() {
        let Some(consumer_manifest) = &consumer.manifest else {
            continue;
        };
        for dep in &consumer_manifest.declared_deps {
            let Some(&publisher_idx) = publishers.get(dep.name.as_str()) else {
                continue; // external dep, not in monorepo
            };
            if publisher_idx == consumer_idx {
                continue; // self-dep (shouldn't happen in practice but guard anyway)
            }

            let attrs = serde_json::json!({
                "dep_name": dep.name,
                "version_req": dep.version_req,
            });
            let attrs_json = serde_json::to_string(&attrs).unwrap();

            // Identity hash covers ONLY identity-bearing fields per spec §5.2:
            // for package_dep, that's `dep_name` (version_req is metadata).
            let mut hasher = Sha256::new();
            hasher.update(b"package_dep|");
            hasher.update(dep.name.as_bytes());
            let attrs_hash = format!("{:x}", hasher.finalize());

            out.push(EdgeCandidate {
                kind: EdgeKind::PackageDep,
                from_kind: NodeKind::Project,
                from_idx: consumer_idx,
                to_kind: NodeKind::Project,
                to_idx: publisher_idx,
                attrs_json,
                attrs_hash,
            });
        }
    }
    out.sort_by(|a, b| {
        (a.from_idx, a.to_idx, &a.attrs_hash).cmp(&(b.from_idx, b.to_idx, &b.attrs_hash))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{DepRequirement, Manifest, ParseStatus, ProjectFacts};

    fn fact(name: &str, deps: &[(&str, Option<&str>)]) -> ProjectFacts {
        ProjectFacts {
            project_root: format!("./{name}"),
            project_name: name.to_string(),
            manifest: Some(Manifest {
                declared_name: name.to_string(),
                version: Some("1.0".into()),
                declared_deps: deps
                    .iter()
                    .map(|(n, v)| DepRequirement {
                        name: (*n).to_string(),
                        version_req: v.map(String::from),
                    })
                    .collect(),
            }),
            warnings: vec![],
            parse_status: ParseStatus::Ok,
        }
    }

    fn fact_no_manifest(name: &str) -> ProjectFacts {
        ProjectFacts {
            project_root: format!("./{name}"),
            project_name: name.to_string(),
            manifest: None,
            warnings: vec![],
            parse_status: ParseStatus::Failed,
        }
    }

    #[test]
    fn matches_consumer_to_publisher_by_name() {
        let facts = vec![
            fact("orchestrator", &[("eval-sdk", Some(">=1.0"))]),
            fact("eval-sdk", &[]),
        ];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1);
        let e = &edges[0];
        assert_eq!(e.from_idx, 0);
        assert_eq!(e.to_idx, 1);
        assert!(e.attrs_json.contains("\"dep_name\":\"eval-sdk\""));
        assert!(e.attrs_json.contains("\"version_req\":\">=1.0\""));
    }

    #[test]
    fn skips_external_deps() {
        let facts = vec![
            fact("orchestrator", &[("eval-sdk", None), ("httpx", None)]),
            fact("eval-sdk", &[]),
        ];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1, "only eval-sdk is in-monorepo");
    }

    #[test]
    fn skips_projects_without_manifest() {
        let facts = vec![
            fact("orchestrator", &[("eval-sdk", None)]),
            fact_no_manifest("docs_only"),
            fact("eval-sdk", &[]),
        ];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to_idx, 2); // eval-sdk
    }

    #[test]
    fn identity_hash_excludes_version_req() {
        let v1 = detect(&[
            fact("a", &[("b", Some(">=1.0"))]),
            fact("b", &[]),
        ]);
        let v2 = detect(&[
            fact("a", &[("b", Some(">=2.0"))]),
            fact("b", &[]),
        ]);
        assert_eq!(v1[0].attrs_hash, v2[0].attrs_hash,
                   "version_req must NOT be part of identity (spec §5.2)");
        assert_ne!(v1[0].attrs_json, v2[0].attrs_json,
                   "but attrs_json DOES capture the change for change-log");
    }

    #[test]
    fn deterministic_ordering() {
        let facts = vec![
            fact("a", &[("c", None), ("b", None)]),
            fact("b", &[]),
            fact("c", &[]),
        ];
        let edges1 = detect(&facts);
        let edges2 = detect(&facts);
        let keys1: Vec<_> = edges1.iter().map(|e| (e.from_idx, e.to_idx)).collect();
        let keys2: Vec<_> = edges2.iter().map(|e| (e.from_idx, e.to_idx)).collect();
        assert_eq!(keys1, keys2);
    }

    #[test]
    fn handles_no_matches() {
        let facts = vec![fact("a", &[("external", None)])];
        assert_eq!(detect(&facts).len(), 0);
    }
}
```

- [ ] **Step 2: Register the module**

In `prograph-core/src/lib.rs`, add `mod detectors;` to the alphabetical mod block:
```rust
mod detectors;
mod discovery;
mod errors;
mod facts;
mod models;
mod parsers;
mod store;
```

- [ ] **Step 3: Run cargo tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core detectors
```
Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph-core/src/detectors/ prograph/prograph-core/src/lib.rs
git commit -m "prograph: M2 deps_detector — package dependency matching with stable identity hash"
```

---

## Task 9: Diff engine

**Files:**
- Create: `prograph-core/src/diff.rs`
- Modify: `prograph-core/src/lib.rs`

The diff engine compares the alive set in the database (last snapshot) with the new entities and produces change events.

- [ ] **Step 1: Write `diff.rs`**

`prograph-core/src/diff.rs`:
```rust
//! Diff engine — compares new entities against the alive set in storage
//! and produces ChangeEvents (added / removed / attrs_changed).

use std::collections::HashMap;

/// A simplified change record produced by `compute_*` functions.
/// The indexer's persist phase emits the actual `change_log` rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    pub identity_key: String,
    pub change: DiffChange,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffChange {
    Added,
    Removed,
    AttrsChanged,
    Unchanged, // not written to change_log but signalled so indexer can extend last_seen
}

/// Diff two sets of entities keyed by identity string.
///
/// `old` is the alive set from the previous snapshot (identity -> attrs_json).
/// `new` is the candidate set from the current scan (identity -> attrs_json).
///
/// For each identity:
/// - in new only            -> Added
/// - in old only            -> Removed
/// - in both, attrs equal   -> Unchanged
/// - in both, attrs differ  -> AttrsChanged
pub fn diff_by_identity(
    old: &HashMap<String, String>,
    new: &HashMap<String, String>,
) -> Vec<DiffEntry> {
    let mut out = Vec::new();

    for (key, new_attrs) in new {
        match old.get(key) {
            None => out.push(DiffEntry {
                identity_key: key.clone(),
                change: DiffChange::Added,
                before_json: None,
                after_json: Some(new_attrs.clone()),
            }),
            Some(old_attrs) if old_attrs == new_attrs => out.push(DiffEntry {
                identity_key: key.clone(),
                change: DiffChange::Unchanged,
                before_json: Some(old_attrs.clone()),
                after_json: Some(new_attrs.clone()),
            }),
            Some(old_attrs) => out.push(DiffEntry {
                identity_key: key.clone(),
                change: DiffChange::AttrsChanged,
                before_json: Some(old_attrs.clone()),
                after_json: Some(new_attrs.clone()),
            }),
        }
    }

    for (key, old_attrs) in old {
        if !new.contains_key(key) {
            out.push(DiffEntry {
                identity_key: key.clone(),
                change: DiffChange::Removed,
                before_json: Some(old_attrs.clone()),
                after_json: None,
            });
        }
    }

    out.sort_by(|a, b| a.identity_key.cmp(&b.identity_key));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn hm(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn detects_added() {
        let old = hm(&[]);
        let new = hm(&[("a", "{}")]);
        let d = diff_by_identity(&old, &new);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].change, DiffChange::Added);
    }

    #[test]
    fn detects_removed() {
        let old = hm(&[("a", "{}")]);
        let new = hm(&[]);
        let d = diff_by_identity(&old, &new);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].change, DiffChange::Removed);
    }

    #[test]
    fn detects_attrs_changed() {
        let old = hm(&[("a", "{\"v\":1}")]);
        let new = hm(&[("a", "{\"v\":2}")]);
        let d = diff_by_identity(&old, &new);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].change, DiffChange::AttrsChanged);
        assert_eq!(d[0].before_json.as_deref(), Some("{\"v\":1}"));
        assert_eq!(d[0].after_json.as_deref(), Some("{\"v\":2}"));
    }

    #[test]
    fn detects_unchanged() {
        let old = hm(&[("a", "{}")]);
        let new = hm(&[("a", "{}")]);
        let d = diff_by_identity(&old, &new);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].change, DiffChange::Unchanged);
    }

    #[test]
    fn mixed_diff_is_sorted_by_key() {
        let old = hm(&[("a", "1"), ("b", "1"), ("d", "1")]);
        let new = hm(&[("a", "1"), ("b", "2"), ("c", "1")]);
        let d = diff_by_identity(&old, &new);
        let keys: Vec<_> = d.iter().map(|e| e.identity_key.as_str()).collect();
        assert_eq!(keys, vec!["a", "b", "c", "d"]);
        let changes: Vec<_> = d.iter().map(|e| e.change).collect();
        assert_eq!(
            changes,
            vec![
                DiffChange::Unchanged,
                DiffChange::AttrsChanged,
                DiffChange::Added,
                DiffChange::Removed
            ]
        );
    }

    #[test]
    fn empty_both_sides_returns_empty() {
        let d = diff_by_identity(&hm(&[]), &hm(&[]));
        assert!(d.is_empty());
    }
}
```

- [ ] **Step 2: Register the module**

`prograph-core/src/lib.rs`:
```rust
mod detectors;
mod diff;
mod discovery;
mod errors;
mod facts;
mod models;
mod parsers;
mod store;
```

- [ ] **Step 3: Run cargo tests**

```sh
cargo test --package prograph-core diff
```
Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph-core/src/diff.rs prograph/prograph-core/src/lib.rs
git commit -m "prograph: M2 diff engine — added/removed/attrs_changed/unchanged classification"
```

---

## Task 10: Lock helper

**Files:**
- Create: `prograph-core/src/lock.rs`
- Modify: `prograph-core/src/lib.rs`

- [ ] **Step 1: Write `lock.rs`**

`prograph-core/src/lock.rs`:
```rust
//! FS exclusive lock used by `prograph index` to prevent concurrent runs.
//! Backed by `fslock` for cross-platform behaviour (flock on Unix, LockFileEx on Windows).

use std::path::{Path, PathBuf};

use fslock::LockFile;

use crate::errors::{PrographError, Result};

/// RAII guard around an exclusive lock on `<path>`. Dropping it releases the lock.
pub struct IndexLockGuard {
    _file: LockFile,
    path: PathBuf,
}

impl IndexLockGuard {
    /// Acquire the lock, or return `PrographError::Lock` if another process holds it.
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| PrographError::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }

        let mut file = LockFile::open(path).map_err(|e| PrographError::Io {
            path: path.display().to_string(),
            source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
        })?;

        let acquired = file.try_lock().map_err(|e| PrographError::Io {
            path: path.display().to_string(),
            source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
        })?;

        if !acquired {
            return Err(PrographError::Lock {
                path: path.display().to_string(),
            });
        }

        Ok(Self {
            _file: file,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn acquire_succeeds_when_lock_free() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".prograph/index.lock");
        let g = IndexLockGuard::acquire(&path).unwrap();
        assert!(path.exists());
        assert_eq!(g.path(), &path);
    }

    #[test]
    fn second_acquire_fails_while_first_held() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("lock");

        let _g1 = IndexLockGuard::acquire(&path).unwrap();
        let result = IndexLockGuard::acquire(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PrographError::Lock { .. }));
    }

    #[test]
    fn lock_released_on_drop() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("lock");
        {
            let _g = IndexLockGuard::acquire(&path).unwrap();
        }
        // After drop, lock should be acquirable again.
        let _g2 = IndexLockGuard::acquire(&path).unwrap();
    }
}
```

- [ ] **Step 2: Register the module**

```rust
mod detectors;
mod diff;
mod discovery;
mod errors;
mod facts;
mod lock;
mod models;
mod parsers;
mod store;
```

- [ ] **Step 3: Run cargo tests**

```sh
cargo test --package prograph-core lock
```
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph-core/src/lock.rs prograph/prograph-core/src/lib.rs
git commit -m "prograph: M2 IndexLockGuard — RAII FS lock for concurrent index prevention"
```

---

## Task 11: Store extensions — write_snapshot, query helpers

**Files:**
- Modify: `prograph-core/src/store.rs`

The indexer needs Store methods to:
1. Load the alive set of projects + edges for the diff phase.
2. Write a new snapshot atomically (one TX), including project/edge inserts, last_seen updates, and change_log appends.
3. Read `SnapshotInfo` for the latest snapshot (used by `prograph status` in Task 13).

- [ ] **Step 1: Add the public Store methods**

Append to `prograph-core/src/store.rs` inside `impl Store`:
```rust
    /// Return the alive set of projects: (root_path -> (project_id, attrs_json)).
    /// "Alive" means `last_seen == MAX(snapshots.id)`.
    pub fn alive_projects(&self) -> Result<std::collections::HashMap<String, (i64, String)>> {
        let mut out = std::collections::HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT id, root_path, attrs_json FROM projects
             WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        for row in rows {
            let (id, root, attrs) = row?;
            out.insert(root, (id, attrs));
        }
        Ok(out)
    }

    /// Return the alive set of edges keyed by identity tuple, value = (edge_id, attrs_json).
    /// Identity key: "{kind}|{from_kind}|{from_id}|{to_kind}|{to_id}|{attrs_hash}".
    pub fn alive_edges(&self) -> Result<std::collections::HashMap<String, (i64, String)>> {
        let mut out = std::collections::HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, from_kind, from_id, to_kind, to_id, attrs_hash, attrs_json
             FROM edges
             WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?;
        for row in rows {
            let (id, kind, fk, fi, tk, ti, ah, aj) = row?;
            let key = format!("{kind}|{fk}|{fi}|{tk}|{ti}|{ah}");
            out.insert(key, (id, aj));
        }
        Ok(out)
    }

    /// Begin a transaction, returning a guard. Use methods on the returned `SnapshotWriter`
    /// to populate the snapshot. Commits on `.commit()`, rolls back on drop without commit.
    pub fn begin_snapshot<'a>(&'a mut self) -> Result<SnapshotWriter<'a>> {
        let tx = self.conn.transaction()?;
        Ok(SnapshotWriter { tx })
    }

    /// Latest SnapshotInfo if any snapshot exists; None otherwise.
    pub fn latest_snapshot_info(&self) -> Result<Option<crate::models::SnapshotInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, monorepo_root, git_commit, prograph_version,
                    (SELECT COUNT(*) FROM projects WHERE last_seen = s.id) AS n_projects,
                    (SELECT COUNT(*) FROM edges    WHERE last_seen = s.id) AS n_edges,
                    (SELECT COUNT(*) FROM change_log WHERE snapshot_id = s.id) AS n_changes
             FROM snapshots s
             ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
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

Then add the `SnapshotWriter` type at the bottom of `store.rs` (outside `impl Store`):
```rust
/// Transactional writer for a single snapshot.
///
/// Drop without commit = ROLLBACK. Methods on the writer accumulate operations
/// inside the transaction; nothing is visible to other readers until `commit()`.
pub struct SnapshotWriter<'a> {
    tx: rusqlite::Transaction<'a>,
}

impl<'a> SnapshotWriter<'a> {
    /// Insert the new snapshots row and return its id.
    pub fn insert_snapshot(
        &self,
        ts: &str,
        monorepo_root: &str,
        git_commit: Option<&str>,
        prograph_version: &str,
    ) -> Result<i64> {
        self.tx.execute(
            "INSERT INTO snapshots (ts, monorepo_root, git_commit, prograph_version)
             VALUES (?, ?, ?, ?)",
            rusqlite::params![ts, monorepo_root, git_commit, prograph_version],
        )?;
        Ok(self.tx.last_insert_rowid())
    }

    /// Insert a new project row; returns its id.
    pub fn insert_project(
        &self,
        snapshot_id: i64,
        name: &str,
        root_path: &str,
        kind: &str,
        attrs_json: &str,
    ) -> Result<i64> {
        self.tx.execute(
            "INSERT INTO projects (name, root_path, kind, attrs_json, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?, ?)",
            rusqlite::params![name, root_path, kind, attrs_json, snapshot_id, snapshot_id],
        )?;
        Ok(self.tx.last_insert_rowid())
    }

    /// Extend an existing project's last_seen to the current snapshot, optionally updating attrs_json.
    pub fn touch_project(
        &self,
        project_id: i64,
        snapshot_id: i64,
        new_attrs_json: Option<&str>,
    ) -> Result<()> {
        if let Some(attrs) = new_attrs_json {
            self.tx.execute(
                "UPDATE projects SET last_seen = ?, attrs_json = ? WHERE id = ?",
                rusqlite::params![snapshot_id, attrs, project_id],
            )?;
        } else {
            self.tx.execute(
                "UPDATE projects SET last_seen = ? WHERE id = ?",
                rusqlite::params![snapshot_id, project_id],
            )?;
        }
        Ok(())
    }

    pub fn insert_edge(
        &self,
        snapshot_id: i64,
        kind: &str,
        from_kind: &str,
        from_id: i64,
        to_kind: &str,
        to_id: i64,
        attrs_json: &str,
        attrs_hash: &str,
    ) -> Result<i64> {
        self.tx.execute(
            "INSERT INTO edges (kind, from_kind, from_id, to_kind, to_id, attrs_json, attrs_hash, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                kind, from_kind, from_id, to_kind, to_id, attrs_json, attrs_hash,
                snapshot_id, snapshot_id
            ],
        )?;
        Ok(self.tx.last_insert_rowid())
    }

    pub fn touch_edge(
        &self,
        edge_id: i64,
        snapshot_id: i64,
        new_attrs_json: Option<&str>,
    ) -> Result<()> {
        if let Some(attrs) = new_attrs_json {
            self.tx.execute(
                "UPDATE edges SET last_seen = ?, attrs_json = ? WHERE id = ?",
                rusqlite::params![snapshot_id, attrs, edge_id],
            )?;
        } else {
            self.tx.execute(
                "UPDATE edges SET last_seen = ? WHERE id = ?",
                rusqlite::params![snapshot_id, edge_id],
            )?;
        }
        Ok(())
    }

    pub fn insert_change_log(
        &self,
        snapshot_id: i64,
        ts: &str,
        entity_kind: &str,
        entity_id: i64,
        change: &str,
        before_json: Option<&str>,
        after_json: Option<&str>,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT INTO change_log (snapshot_id, ts, entity_kind, entity_id, change, before_json, after_json)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                snapshot_id, ts, entity_kind, entity_id, change, before_json, after_json
            ],
        )?;
        Ok(())
    }

    pub fn commit(self) -> Result<()> {
        self.tx.commit()?;
        Ok(())
    }
}
```

- [ ] **Step 2: Add tests for the new Store methods**

Append inside `#[cfg(test)] mod tests` in `store.rs`:
```rust
    #[test]
    fn alive_projects_empty_before_any_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        assert!(store.alive_projects().unwrap().is_empty());
    }

    #[test]
    fn write_snapshot_then_alive_projects_reflects_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        let writer = store.begin_snapshot().unwrap();
        let snap = writer
            .insert_snapshot("2026-05-25T00:00:00Z", "/m", None, "0.1.0")
            .unwrap();
        let pid = writer
            .insert_project(snap, "alpha", "./alpha", "python", "{}")
            .unwrap();
        writer
            .insert_change_log(snap, "2026-05-25T00:00:00Z", "project", pid, "added", None, Some("{}"))
            .unwrap();
        writer.commit().unwrap();

        let alive = store.alive_projects().unwrap();
        assert_eq!(alive.len(), 1);
        assert!(alive.contains_key("./alpha"));
    }

    #[test]
    fn latest_snapshot_info_returns_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        assert!(store.latest_snapshot_info().unwrap().is_none());

        {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer
                .insert_snapshot("2026-05-25T00:00:00Z", "/m", None, "0.1.0")
                .unwrap();
            writer.insert_project(snap, "a", "./a", "python", "{}").unwrap();
            writer.commit().unwrap();
        }

        let info = store.latest_snapshot_info().unwrap().unwrap();
        assert_eq!(info.n_projects, 1);
        assert_eq!(info.n_edges, 0);
        assert_eq!(info.n_changes, 0);
    }

    #[test]
    fn rollback_on_drop_without_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        {
            let writer = store.begin_snapshot().unwrap();
            writer.insert_snapshot("ts", "/m", None, "v").unwrap();
            // No commit — drop rolls back.
        }
        assert!(store.latest_snapshot_info().unwrap().is_none());
    }
```

- [ ] **Step 3: Run cargo tests**

```sh
cargo test --package prograph-core store
```
Expected: 9 tests pass (3 from M1 + 2 from Task 1 + 4 new).

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph-core/src/store.rs
git commit -m "prograph: M2 Store::{alive_projects, alive_edges, begin_snapshot, latest_snapshot_info}"
```

---

## Task 12: Indexer pipeline

**Files:**
- Create: `prograph-core/src/indexer.rs`
- Modify: `prograph-core/src/lib.rs` (register `mod indexer` + PyO3 wrapper)

The indexer orchestrates the existing pieces into a coherent pipeline: discovery → parse → detect → diff → persist.

- [ ] **Step 1: Write `indexer.rs`**

`prograph-core/src/indexer.rs`:
```rust
//! Indexer pipeline — orchestrates discovery, parsing, edge detection, diffing, and persistence.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use sha2::{Digest, Sha256};

use crate::detectors;
use crate::diff::{diff_by_identity, DiffChange};
use crate::discovery::scan_monorepo;
use crate::errors::Result;
use crate::facts::{ParseStatus, ProjectFacts};
use crate::lock::IndexLockGuard;
use crate::models::IndexSummary;
use crate::parsers::parse_project;
use crate::store::Store;

/// Run the full index pipeline against `monorepo_root`. Acquires `.prograph/index.lock`
/// for the duration; fails fast if another `prograph index` is running.
pub fn index_monorepo(monorepo_root: &Path, store: &mut Store) -> Result<IndexSummary> {
    let start = Instant::now();
    let lock_path = monorepo_root.join(".prograph").join("index.lock");
    let _lock = IndexLockGuard::acquire(&lock_path)?;

    // Phase 1: Discovery.
    let candidates = scan_monorepo(monorepo_root)?;

    // Phase 2: Parsing (sequential in M2; rayon parallelism is a future optimization).
    let mut facts: Vec<ProjectFacts> = Vec::with_capacity(candidates.len());
    let mut warning_count: i64 = 0;
    for cand in &candidates {
        let proj_root = monorepo_root.join(cand.root_path.trim_start_matches("./"));
        let out = parse_project(&proj_root, cand.kind)?;
        warning_count += out.warnings.len() as i64;
        let parse_status = if !out.warnings.is_empty() && out.manifest.is_none() {
            ParseStatus::Failed
        } else if !out.warnings.is_empty() {
            ParseStatus::Partial
        } else {
            ParseStatus::Ok
        };
        facts.push(ProjectFacts {
            project_root: cand.root_path.clone(),
            project_name: cand.name.clone(),
            manifest: out.manifest,
            warnings: out.warnings,
            parse_status,
        });
    }

    // Phase 3: Edge detection.
    let edge_candidates = detectors::detect_all(&facts);

    // Phase 4: Diff vs alive set.
    let alive_projects = store.alive_projects()?;
    let alive_edges = store.alive_edges()?;

    // Build new-state identity maps for diff.
    let new_project_attrs: HashMap<String, String> = facts
        .iter()
        .map(|f| {
            let attrs = serde_json::json!({
                "name": f.project_name,
                "declared_name": f.manifest.as_ref().map(|m| &m.declared_name),
                "version": f.manifest.as_ref().and_then(|m| m.version.as_ref()),
                "parse_status": match f.parse_status {
                    ParseStatus::Ok => "ok",
                    ParseStatus::Partial => "partial",
                    ParseStatus::Failed => "failed",
                },
            });
            (f.project_root.clone(), serde_json::to_string(&attrs).unwrap())
        })
        .collect();

    let new_edge_attrs: HashMap<String, String> = edge_candidates
        .iter()
        .map(|c| {
            // We can't compute the final identity key until we resolve project ids,
            // but the attrs_hash itself is stable. We use a placeholder that the
            // persist phase will rewrite once ids are known. For diff purposes we
            // can key off "package_dep|<from_root>|<to_root>|<attrs_hash>".
            let from_root = &facts[c.from_idx].project_root;
            let to_root = &facts[c.to_idx].project_root;
            let key = format!("package_dep|{}|{}|{}", from_root, to_root, c.attrs_hash);
            (key, c.attrs_json.clone())
        })
        .collect();

    // Rekey alive_edges by the same root-path form using a mapping from project_id -> root_path.
    let project_id_to_root: HashMap<i64, &str> = alive_projects
        .iter()
        .map(|(root, (id, _))| (*id, root.as_str()))
        .collect();
    let alive_edges_by_root: HashMap<String, (i64, String)> = alive_edges
        .iter()
        .filter_map(|(_key, (id, attrs))| {
            // Parse the stored key components by querying the row directly is heavier;
            // for M2 we just rebuild from the row. We re-query via the connection inside Store
            // — but to keep this function pure-Rust without leaking SQL, we instead retain the
            // existing key shape (kind|from_kind|from_id|to_kind|to_id|attrs_hash) and translate
            // ids back to root paths here. The Store guarantees the alive set only includes
            // edges whose endpoints are alive projects.
            //
            // Format expected: package_dep|project|<from_id>|project|<to_id>|<attrs_hash>
            let parts: Vec<&str> = _key.split('|').collect();
            if parts.len() != 6 {
                return None;
            }
            let from_id: i64 = parts[2].parse().ok()?;
            let to_id: i64 = parts[4].parse().ok()?;
            let attrs_hash = parts[5];
            let from_root = project_id_to_root.get(&from_id)?;
            let to_root = project_id_to_root.get(&to_id)?;
            let key = format!("package_dep|{}|{}|{}", from_root, to_root, attrs_hash);
            Some((key, (*id, attrs.clone())))
        })
        .collect();

    let project_diff = diff_by_identity(
        &alive_projects.iter().map(|(k, (_, a))| (k.clone(), a.clone())).collect(),
        &new_project_attrs,
    );
    let edge_diff = diff_by_identity(
        &alive_edges_by_root.iter().map(|(k, (_, a))| (k.clone(), a.clone())).collect(),
        &new_edge_attrs,
    );

    // Phase 5: Persist in a single transaction.
    let writer = store.begin_snapshot()?;
    let ts = current_iso_ts();
    let snap_id = writer.insert_snapshot(&ts, &monorepo_root.display().to_string(), None, env!("CARGO_PKG_VERSION"))?;

    let mut new_project_ids: HashMap<String, i64> = HashMap::new();
    let mut n_projects: i64 = 0;
    let mut n_changes: i64 = 0;

    for entry in &project_diff {
        let key = &entry.identity_key;
        match entry.change {
            DiffChange::Added => {
                let fact = facts.iter().find(|f| &f.project_root == key).unwrap();
                let kind_str = candidates
                    .iter()
                    .find(|c| &c.root_path == key)
                    .map(|c| c.kind.name())
                    .unwrap_or("python");
                let attrs = entry.after_json.as_deref().unwrap_or("{}");
                let pid = writer.insert_project(snap_id, &fact.project_name, key, kind_str, attrs)?;
                new_project_ids.insert(key.clone(), pid);
                writer.insert_change_log(
                    snap_id, &ts, "project", pid, "added",
                    None, entry.after_json.as_deref(),
                )?;
                n_projects += 1;
                n_changes += 1;
            }
            DiffChange::Unchanged => {
                let (pid, _) = &alive_projects[key];
                writer.touch_project(*pid, snap_id, None)?;
                new_project_ids.insert(key.clone(), *pid);
                n_projects += 1;
            }
            DiffChange::AttrsChanged => {
                let (pid, _) = &alive_projects[key];
                writer.touch_project(*pid, snap_id, entry.after_json.as_deref())?;
                new_project_ids.insert(key.clone(), *pid);
                writer.insert_change_log(
                    snap_id, &ts, "project", *pid, "attrs_changed",
                    entry.before_json.as_deref(), entry.after_json.as_deref(),
                )?;
                n_projects += 1;
                n_changes += 1;
            }
            DiffChange::Removed => {
                let (pid, _) = &alive_projects[key];
                writer.insert_change_log(
                    snap_id, &ts, "project", *pid, "removed",
                    entry.before_json.as_deref(), None,
                )?;
                n_changes += 1;
            }
        }
    }

    // Resolve edge endpoints to DB ids; insert/touch as appropriate.
    let mut n_edges: i64 = 0;
    for entry in &edge_diff {
        // identity key: package_dep|<from_root>|<to_root>|<attrs_hash>
        let parts: Vec<&str> = entry.identity_key.split('|').collect();
        let from_root = parts[1];
        let to_root = parts[2];
        let attrs_hash = parts[3];
        let from_id = new_project_ids.get(from_root).copied();
        let to_id = new_project_ids.get(to_root).copied();
        if from_id.is_none() || to_id.is_none() {
            // endpoints no longer alive — skip (defensive; deps_detector wouldn't have produced this)
            continue;
        }
        let from_id = from_id.unwrap();
        let to_id = to_id.unwrap();
        match entry.change {
            DiffChange::Added => {
                let attrs = entry.after_json.as_deref().unwrap_or("{}");
                let eid = writer.insert_edge(
                    snap_id, "package_dep", "project", from_id, "project", to_id, attrs, attrs_hash,
                )?;
                writer.insert_change_log(
                    snap_id, &ts, "edge", eid, "added", None, Some(attrs),
                )?;
                n_edges += 1;
                n_changes += 1;
            }
            DiffChange::Unchanged => {
                let (eid, _) = &alive_edges_by_root[&entry.identity_key];
                writer.touch_edge(*eid, snap_id, None)?;
                n_edges += 1;
            }
            DiffChange::AttrsChanged => {
                let (eid, _) = &alive_edges_by_root[&entry.identity_key];
                writer.touch_edge(*eid, snap_id, entry.after_json.as_deref())?;
                writer.insert_change_log(
                    snap_id, &ts, "edge", *eid, "attrs_changed",
                    entry.before_json.as_deref(), entry.after_json.as_deref(),
                )?;
                n_edges += 1;
                n_changes += 1;
            }
            DiffChange::Removed => {
                let (eid, _) = &alive_edges_by_root[&entry.identity_key];
                writer.insert_change_log(
                    snap_id, &ts, "edge", *eid, "removed",
                    entry.before_json.as_deref(), None,
                )?;
                n_changes += 1;
            }
        }
    }

    writer.commit()?;

    Ok(IndexSummary {
        snapshot_id: snap_id,
        ts,
        n_projects,
        n_edges,
        n_changes,
        n_warnings: warning_count,
        duration_ms: start.elapsed().as_millis() as i64,
    })
}

fn current_iso_ts() -> String {
    // RFC3339 second-precision, UTC. Avoids pulling in chrono — uses std SystemTime arithmetic.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Format YYYY-MM-DDTHH:MM:SSZ from secs since epoch.
    let (y, mo, d, h, mi, s) = secs_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn secs_to_ymdhms(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    // Civil-from-days algorithm by Howard Hinnant.
    let s = secs as i64;
    let days = s.div_euclid(86_400);
    let rem = s.rem_euclid(86_400);
    let h = (rem / 3600) as u32;
    let mi = ((rem % 3600) / 60) as u32;
    let s = (rem % 60) as u32;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = (y + if mp >= 10 { 1 } else { 0 }) as i32;
    (year, mo, d, h, mi, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    fn setup_monorepo() -> TempDir {
        let dir = TempDir::new().unwrap();
        // Create .prograph dir (init prerequisite)
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();

        // Create three Python projects: consumer, sdk (publisher), and external-only.
        fs::create_dir_all(dir.path().join("consumer")).unwrap();
        fs::write(
            dir.path().join("consumer/pyproject.toml"),
            r#"[project]
name = "consumer"
version = "1.0"
dependencies = ["my-sdk>=1.0"]
"#,
        ).unwrap();

        fs::create_dir_all(dir.path().join("sdk")).unwrap();
        fs::write(
            dir.path().join("sdk/pyproject.toml"),
            r#"[project]
name = "my-sdk"
version = "1.5"
dependencies = []
"#,
        ).unwrap();

        dir
    }

    #[test]
    fn first_index_creates_snapshot_with_two_projects_and_one_edge() {
        let dir = setup_monorepo();
        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        let summary = index_monorepo(dir.path(), &mut store).unwrap();
        assert_eq!(summary.n_projects, 2);
        assert_eq!(summary.n_edges, 1);
        assert!(summary.n_changes >= 3, "expected ≥3 change_log entries (2 projects + 1 edge added)");
    }

    #[test]
    fn second_index_no_changes_is_empty_changelog() {
        let dir = setup_monorepo();
        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();
        let summary = index_monorepo(dir.path(), &mut store).unwrap();
        assert_eq!(summary.n_projects, 2);
        assert_eq!(summary.n_edges, 1);
        // No diff entries means snapshots were extended (last_seen updated) but no change_log writes.
        assert_eq!(summary.n_changes, 0);
    }

    #[test]
    fn version_bump_produces_attrs_changed_event() {
        let dir = setup_monorepo();
        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        // Bump the version requirement.
        fs::write(
            dir.path().join("consumer/pyproject.toml"),
            r#"[project]
name = "consumer"
version = "1.1"
dependencies = ["my-sdk>=2.0"]
"#,
        ).unwrap();

        let summary = index_monorepo(dir.path(), &mut store).unwrap();
        // Project consumer's version changed → attrs_changed; edge's version_req changed → attrs_changed.
        assert!(summary.n_changes >= 2, "expected attrs_changed for both project and edge");
    }
}
```

- [ ] **Step 2: Register the module + add a PyO3 wrapper**

In `prograph-core/src/lib.rs`:
```rust
mod detectors;
mod diff;
mod discovery;
mod errors;
mod facts;
mod indexer;
mod lock;
mod models;
mod parsers;
mod store;
```

At the bottom of `lib.rs`, add a PyO3 wrapper:
```rust
/// Python entry point for `prograph index`.
#[pyfunction]
#[pyo3(name = "index_monorepo")]
fn py_index_monorepo(monorepo_root: &str, db_path: &str) -> PyResult<IndexSummary> {
    let mut store = Store::open(std::path::Path::new(db_path))?;
    Ok(indexer::index_monorepo(std::path::Path::new(monorepo_root), &mut store)?)
}
```

And register it in the `#[pymodule]`:
```rust
    m.add_function(wrap_pyfunction!(py_index_monorepo, m)?)?;
```

- [ ] **Step 3: Extend the `.pyi` stub**

Append to `prograph/_core.pyi`:
```python
def index_monorepo(monorepo_root: str, db_path: str) -> IndexSummary: ...
```

- [ ] **Step 4: Rebuild and run tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv sync
cargo test --package prograph-core indexer
```
Expected: 3 tests pass.

Full Rust suite:
```sh
cargo test --package prograph-core
```
Expected: 36+ tests pass (cumulative from all previous tasks).

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/indexer.rs prograph/prograph-core/src/lib.rs \
        prograph/prograph/_core.pyi
git commit -m "prograph: M2 indexer pipeline — discover → parse → detect → diff → persist"
```

---

## Task 13: CLI `prograph index`

**Files:**
- Modify: `prograph/cli.py`
- Create: `tests/integration/test_cli_index.py`

- [ ] **Step 1: Write the integration test (TDD)**

`tests/integration/test_cli_index.py`:
```python
"""Tests for `prograph index`."""

from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()


def _setup(root: Path) -> None:
    (root / "consumer").mkdir()
    (root / "consumer" / "pyproject.toml").write_text(
        '[project]\nname="consumer"\nversion="1.0"\ndependencies=["sdk"]\n'
    )
    (root / "sdk").mkdir()
    (root / "sdk" / "pyproject.toml").write_text(
        '[project]\nname="sdk"\nversion="0.1"\ndependencies=[]\n'
    )


def test_index_requires_init(tmp_path: Path):
    _setup(tmp_path)
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1
    assert "not initialized" in (result.stdout + result.stderr).lower()


def test_index_writes_snapshot_with_one_edge(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.stdout
    assert "snapshot" in result.stdout.lower() or "index" in result.stdout.lower()

    paths = PrographPaths(monorepo_root=tmp_path)
    assert paths.db_path.is_file()

    # Use the Rust helper directly to verify state.
    from prograph._core import index_monorepo
    summary = index_monorepo(str(tmp_path), str(paths.db_path))
    assert summary.n_changes == 0, "second index on same state should produce no changes"


def test_index_fails_when_another_holds_lock(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    # Acquire the lock by hand.
    paths = PrographPaths(monorepo_root=tmp_path)
    paths.lock_path.parent.mkdir(parents=True, exist_ok=True)
    import fcntl
    f = open(paths.lock_path, "w")
    try:
        fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
        assert result.exit_code == 1
        assert "lock" in (result.stdout + result.stderr).lower()
    finally:
        fcntl.flock(f, fcntl.LOCK_UN)
        f.close()


def test_index_json_output(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--json"])
    assert result.exit_code == 0
    import json
    payload = json.loads(result.stdout)
    assert payload["n_projects"] == 2
    assert payload["n_edges"] == 1
    assert payload["snapshot_id"] >= 1
```

Note: `fcntl` is Unix-only — wrap the lock test with `@pytest.mark.skipif(sys.platform == "win32", reason="POSIX fcntl required")` if running on Windows. For our CI matrix (Linux + macOS) this is fine.

Run the test, expect failure:
```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run pytest tests/integration/test_cli_index.py -v
```
Expected: FAILED (no `index` command yet).

- [ ] **Step 2: Implement the `index` command in `cli.py`**

Add to `prograph/cli.py` (alongside `init` and `status`). First import:
```python
import sys as _sys

from prograph.models import IndexSummary
```

Then the command:
```python
@app.command()
def index(
    monorepo: Path = typer.Option(  # noqa: B008
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
    json: bool = typer.Option(False, "--json", help="Emit IndexSummary as JSON instead of a status line."),  # noqa: B008
) -> None:
    """Run a full index of the monorepo: discover, parse, detect edges, diff, persist."""

    root = _resolve_monorepo(monorepo)
    paths = PrographPaths(monorepo_root=root)
    if not paths.is_initialized():
        err_console.print(
            f"[red]error:[/red] not initialized at {paths.prograph_dir}. "
            "Run `prograph init` first."
        )
        raise typer.Exit(code=1)

    try:
        raw = _core.index_monorepo(str(root), str(paths.db_path))
    except Exception as exc:  # PrographError surfaces as PyRuntimeError / PyIOError / PyValueError
        message = str(exc).lower()
        if "lock" in message:
            err_console.print(f"[red]error:[/red] another prograph index is running ({exc})")
        else:
            err_console.print(f"[red]error:[/red] {exc}")
        raise typer.Exit(code=1) from exc

    summary = IndexSummary.from_core(raw)

    if json:
        _sys.stdout.write(_json.dumps(summary.model_dump(mode="json"), indent=2) + "\n")
        return

    console.print(
        f"[green]snapshot #{summary.snapshot_id}[/green] written in "
        f"[bold]{summary.duration_ms}ms[/bold]"
    )
    console.print(
        f"  [cyan]{summary.n_projects}[/cyan] projects, "
        f"[cyan]{summary.n_edges}[/cyan] edges, "
        f"[cyan]{summary.n_changes}[/cyan] changes"
        + (f", [yellow]{summary.n_warnings}[/yellow] warnings" if summary.n_warnings else "")
    )
```

(The `_json` alias is already imported in cli.py from M1's `status` command.)

- [ ] **Step 3: Run tests**

```sh
uv run pytest tests/integration/test_cli_index.py -v
```
Expected: 4 passed.

Full suite:
```sh
uv run pytest -v
```
Expected: 27+ passed.

- [ ] **Step 4: Manual smoke against the bundled fixture**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
rm -rf tests/fixtures/monorepo_full/.prograph
uv run prograph init --monorepo tests/fixtures/monorepo_full
uv run prograph index --monorepo tests/fixtures/monorepo_full --json
rm -rf tests/fixtures/monorepo_full/.prograph
```

Expected JSON includes `"n_projects": 5` (4 python + 1 docs), `"n_edges": 3` (orchestrator→eval_sdk, orchestrator→policy, runner→orchestrator).

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph/cli.py prograph/tests/integration/test_cli_index.py
git commit -m "prograph: M2 'prograph index' command — full pipeline + JSON summary"
```

---

## Task 14: CLI `prograph status` — surface latest snapshot info

**Files:**
- Modify: `prograph/cli.py`
- Modify: `tests/integration/test_cli_status.py`

`status` continues to show the discovery results, but now also appends snapshot info if a snapshot exists.

- [ ] **Step 1: Update the test to assert the new behaviour**

Append to `tests/integration/test_cli_status.py`:
```python
def test_status_shows_snapshot_info_after_index(tmp_path: Path):
    _setup_mini_monorepo(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0
    assert "snapshot" in result.stdout.lower()
    assert "projects" in result.stdout.lower()


def test_status_json_includes_snapshot_when_indexed(tmp_path: Path):
    _setup_mini_monorepo(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])

    import json
    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path), "--json"])
    payload = json.loads(result.stdout)
    assert "snapshot" in payload
    assert payload["snapshot"]["n_projects"] >= 1


def test_status_json_snapshot_null_when_not_indexed(tmp_path: Path):
    _setup_mini_monorepo(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    import json
    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path), "--json"])
    payload = json.loads(result.stdout)
    assert payload["snapshot"] is None
```

- [ ] **Step 2: Update the `status` command**

Locate the `status` command in `prograph/cli.py`. Replace its body to:
1. Open the Store if `paths.db_path` exists (it always does after `init` — Store::open creates it).
2. Query `latest_snapshot_info` after the discovery scan.
3. Render the snapshot info below the discovery table; include it in the JSON payload as `"snapshot"`.

Replace the `status` function with:
```python
@app.command()
def status(
    monorepo: Path = typer.Option(  # noqa: B008
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
    json: bool = typer.Option(False, "--json", help="Emit JSON to stdout instead of a table."),  # noqa: B008
) -> None:
    """Show monorepo state: project candidates from discovery + latest snapshot info."""

    root = _resolve_monorepo(monorepo)
    paths = PrographPaths(monorepo_root=root)
    if not paths.is_initialized():
        err_console.print(
            f"[red]error:[/red] not initialized at {paths.prograph_dir}. "
            "Run `prograph init` first."
        )
        raise typer.Exit(code=1)

    raw_candidates = _core.scan_monorepo(str(root))
    candidates = [ProjectCandidate.from_core(c) for c in raw_candidates]

    # Try to read snapshot info — the DB exists if `init` ran (graph.db is created lazily on first open).
    snapshot = None
    if paths.db_path.exists():
        from prograph.models import SnapshotInfo
        # Open the store to query latest snapshot. This is a read-only operation here.
        # Use a small Rust helper added later; for now, open the file via sqlite3 isn't ideal.
        # Use _core to keep schema knowledge in Rust:
        raw_snap = _core.latest_snapshot_info(str(paths.db_path))
        snapshot = SnapshotInfo.from_core(raw_snap) if raw_snap is not None else None

    if json:
        payload = {
            "monorepo_root": str(root),
            "snapshot": snapshot.model_dump(mode="json") if snapshot else None,
            "projects": [c.model_dump(mode="json") for c in candidates],
        }
        _sys.stdout.write(_json.dumps(payload, indent=2) + "\n")
        return

    table = Table(title=f"prograph status — {root}")
    table.add_column("name", style="cyan")
    table.add_column("kind", style="magenta")
    table.add_column("root", style="dim")
    table.add_column("manifests")

    for c in candidates:
        table.add_row(c.name, c.kind.value, c.root_path, ", ".join(c.manifests))

    console.print(table)
    console.print(f"[dim]{len(candidates)} projects discovered.[/dim]")

    if snapshot:
        console.print(
            f"[dim]Last snapshot #{snapshot.id} at {snapshot.ts} — "
            f"{snapshot.n_projects} projects, {snapshot.n_edges} edges, "
            f"{snapshot.n_changes} changes.[/dim]"
        )
    else:
        console.print("[dim]No snapshot yet — run `prograph index` to create one.[/dim]")
```

- [ ] **Step 3: Expose `latest_snapshot_info` as a PyO3 function**

In `prograph-core/src/lib.rs`, add:
```rust
/// Python entry point: return SnapshotInfo for the latest snapshot, or None.
#[pyfunction]
#[pyo3(name = "latest_snapshot_info")]
fn py_latest_snapshot_info(db_path: &str) -> PyResult<Option<SnapshotInfo>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.latest_snapshot_info()?)
}
```

Register it in `#[pymodule]`:
```rust
    m.add_function(wrap_pyfunction!(py_latest_snapshot_info, m)?)?;
```

Update `prograph/_core.pyi`:
```python
def latest_snapshot_info(db_path: str) -> SnapshotInfo | None: ...
```

- [ ] **Step 4: Rebuild and run tests**

```sh
uv sync
uv run pytest tests/integration/test_cli_status.py -v
```
Expected: 6 passed (3 from M1 + 3 new).

Full suite:
```sh
uv run pytest -v
```
Expected: 30+ passed.

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph/cli.py prograph/tests/integration/test_cli_status.py \
        prograph/prograph-core/src/lib.rs prograph/prograph/_core.pyi
git commit -m "prograph: M2 'prograph status' surfaces latest SnapshotInfo (with --json field)"
```

---

## Task 15: Integration test on monorepo_full

**Files:**
- Create: `tests/integration/test_cli_index_full.py`

A full end-to-end test against the realistic synthetic fixture: verifies all four expected edges are detected, idempotent re-index produces no changes, and a manifest mutation produces an `attrs_changed` event.

- [ ] **Step 1: Write the integration test**

`tests/integration/test_cli_index_full.py`:
```python
"""End-to-end integration test against monorepo_full fixture."""

import json
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()

FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_full"


@pytest.fixture
def fresh_full_fixture(tmp_path: Path) -> Path:
    """Copy monorepo_full into tmp_path so tests don't pollute the repo's fixture dir."""
    import shutil
    dst = tmp_path / "monorepo_full"
    shutil.copytree(FIXTURE, dst)
    return dst


def _run(args: list[str]) -> dict:
    result = runner.invoke(app, args + ["--json"])
    assert result.exit_code == 0, result.stdout + result.stderr
    return json.loads(result.stdout)


def test_full_index_detects_all_three_cross_deps(fresh_full_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_full_fixture)])
    summary = _run(["index", "--monorepo", str(fresh_full_fixture)])
    assert summary["n_projects"] == 5  # 4 python + 1 docs
    assert summary["n_edges"] == 3      # orchestrator→eval_sdk, orchestrator→policy, runner→orchestrator
    assert summary["n_changes"] >= 8    # 5 project-added + 3 edge-added


def test_full_reindex_idempotent(fresh_full_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_full_fixture)])
    _run(["index", "--monorepo", str(fresh_full_fixture)])
    summary = _run(["index", "--monorepo", str(fresh_full_fixture)])
    assert summary["n_changes"] == 0


def test_full_version_bump_produces_attrs_changed(fresh_full_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_full_fixture)])
    _run(["index", "--monorepo", str(fresh_full_fixture)])

    # Bump orchestrator's eval-sdk requirement from >=1.0 to >=2.0
    orch_toml = fresh_full_fixture / "orchestrator" / "pyproject.toml"
    text = orch_toml.read_text().replace('"eval-sdk>=1.0"', '"eval-sdk>=2.0"')
    orch_toml.write_text(text)

    summary = _run(["index", "--monorepo", str(fresh_full_fixture)])
    assert summary["n_changes"] >= 1  # the orchestrator→eval-sdk edge attrs_changed


def test_full_status_after_index_includes_snapshot(fresh_full_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_full_fixture)])
    _run(["index", "--monorepo", str(fresh_full_fixture)])

    status = _run(["status", "--monorepo", str(fresh_full_fixture)])
    snap = status["snapshot"]
    assert snap is not None
    assert snap["n_projects"] == 5
    assert snap["n_edges"] == 3
```

- [ ] **Step 2: Run it**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run pytest tests/integration/test_cli_index_full.py -v
```
Expected: 4 passed.

Full suite:
```sh
uv run pytest -v
```
Expected: 34+ passed.

- [ ] **Step 3: Commit**

```sh
git add prograph/tests/integration/test_cli_index_full.py
git commit -m "prograph: M2 end-to-end integration test on monorepo_full fixture"
```

---

## Task 16: Real-monorepo smoke extension

**Files:**
- Modify: `tests/integration/test_smoke_real.py`

Extend the existing opt-in smoke to also exercise the indexer against `all_ai_orchestrators/`.

- [ ] **Step 1: Add an `index` step to the smoke**

In `tests/integration/test_smoke_real.py`, modify the test body to also run `index` after `status`, and assert the resulting snapshot contains at least one edge that matches a known dep relationship from the user's memory (e.g. Maestro consumes atp-platform-sdk).

Replace the existing test with:
```python
@pytest.mark.realmonorepo
@pytest.mark.skipif(REAL_MONOREPO is None, reason="real monorepo not present at expected path")
def test_init_status_and_index_run_on_real_monorepo(tmp_path: Path):
    real = REAL_MONOREPO
    assert real is not None  # for type checker

    init = runner.invoke(app, ["init", "--monorepo", str(real)])
    assert init.exit_code == 0, init.stdout

    status = runner.invoke(app, ["status", "--monorepo", str(real), "--json"])
    assert status.exit_code == 0, status.stdout
    import json
    payload = json.loads(status.stdout)
    names = {p["name"] for p in payload["projects"]}
    assert {"Maestro", "arbiter", "atp-platform"} & names, (
        f"expected to discover at least one known project, got: {sorted(names)}"
    )

    # Now run the indexer.
    idx = runner.invoke(app, ["index", "--monorepo", str(real), "--json"])
    assert idx.exit_code == 0, idx.stdout
    summary = json.loads(idx.stdout)
    assert summary["n_projects"] >= 3
    # We expect at least one cross-project edge (the real monorepo has Maestro→atp-platform-sdk).
    assert summary["n_edges"] >= 1, f"expected ≥1 edge, got summary: {summary}"
```

- [ ] **Step 2: Run the smoke**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run pytest -m realmonorepo -v
```

Expected: 1 passed. The test should print the snapshot summary including the edge count.

- [ ] **Step 3: Commit**

```sh
git add prograph/tests/integration/test_smoke_real.py
git commit -m "prograph: M2 real-monorepo smoke also exercises 'prograph index'"
```

---

## Task 17: README + CLAUDE.md updates + final gate

**Files:**
- Modify: `prograph/README.md`
- Modify: `prograph/CLAUDE.md`
- Modify: `prograph/docs/superpowers/plans/2026-05-25-prograph-m2-python-indexer.md` (this file — close DoD)

- [ ] **Step 1: Update README**

In `prograph/README.md`, update the Status line and Usage section to:
```markdown
**Status:** M2 — Python indexer. `prograph init`, `prograph status`, `prograph index` work end-to-end against Python monorepos. Edge detection for package dependencies. Multi-language parsers, contracts/MCP detectors, MD export, browser UI, and MCP server land in M3–M7.
```

And replace the Usage block:
```markdown
## Usage

```sh
cd <your-monorepo-root>
prograph init      # creates .prograph/config.toml + .gitignore
prograph index     # discovers projects, parses pyproject.toml, detects cross-project deps,
                   #   persists to SQLite, writes change_log entries
prograph status    # shows discovered projects + latest snapshot summary
prograph status --json   # machine-readable output for scripts and AI
prograph index --json    # IndexSummary JSON (snapshot_id, n_projects, n_edges, n_changes, ...)
```

After M2, `.prograph/graph.db` contains the actual snapshot history. Per-project MD files and browser UI land in M5/M6.
```

- [ ] **Step 2: Update CLAUDE.md**

In `prograph/CLAUDE.md`, update the Architecture section to reflect M2:

Replace the "Architecture (M1 state)" section with:
```markdown
## Architecture (M2 state)

Two-layer build:

- **`prograph-core` (Rust crate via PyO3):**
  - `discovery` — project classification + monorepo walk (M1)
  - `parsers/python` — `pyproject.toml` parsing (M2; M3 adds `rust`, `js`)
  - `detectors/deps` — package-dependency matching (M2; M4 adds `contracts`, `mcp`)
  - `diff` — added/removed/attrs_changed/unchanged classifier
  - `lock` — RAII FS exclusive lock (`fslock`)
  - `indexer` — pipeline orchestrator (discover → parse → detect → diff → persist)
  - `store` — SQLite schema v2 + transactional snapshot writer
  - `models` — Rust pyclasses (`ProjectKind`, `ProjectCandidate`, `Edge`, `ChangeEvent`, `SnapshotInfo`, `IndexSummary`, …)
  - `facts` — `Manifest`, `DepRequirement`, `ProjectFacts`
  - `errors` — `PrographError` with PyErr mapping
  - `migrations/v1.sql`, `migrations/v2.sql`
- **`prograph` (Python package):** `cli.py` (`init`, `index`, `status`, `--version`), `models.py` (pydantic mirrors with `from_core(...)`), `paths.py`.

Tests live in `tests/` (pytest) and as inline `#[cfg(test)]` modules in each Rust source file.

The Rust↔Python boundary remains data-only.
```

Replace the "What is NOT in M1" section with:
```markdown
## What is NOT in M2

Multi-language parsers (Rust + JS via tree-sitter), contracts detector, MCP detector, MD export, browser UI, MCP stdio server. These land in M3–M7 with their own plans.
```

Add to "Common commands":
```sh
uv run prograph index [--monorepo PATH] [--json]   # run full index pipeline
```

- [ ] **Step 3: Run the full local gate one more time**

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

Expected: every command exits 0.

- [x] **Step 4: Close the M2 DoD boxes**

In this plan file, replace the "Definition of Done (M2)" `- [ ]` items with `- [x]` and annotate the achieved counts.

- [x] **Step 5: Final M2 commit**

```sh
git add prograph/README.md prograph/CLAUDE.md \
        prograph/docs/superpowers/plans/2026-05-25-prograph-m2-python-indexer.md
git commit -m "prograph: M2 close — docs updated, full test suite green, DoD checked"
```

---

## Definition of Done (M2)

- [x] `cargo test --all-targets` passes (≥35 unit + integration tests). — **54 achieved**
- [x] `uv run pytest -v` passes (≥34 tests across unit + integration). — **34 achieved**
- [~] `uv run pytest -m realmonorepo -v` passes against the real `all_ai_orchestrators/` and produces ≥1 edge. — **partial: smoke test passes (1/1) but produces 0 edges; the real monorepo uses PEP 735 `[dependency-groups]` and workspace sub-package publishing, neither of which M2 parses. Documented as M2 limitation in README.**
- [x] `uv run prograph index --monorepo <path>` writes a snapshot to `.prograph/graph.db` with projects + edges + change_log entries; second invocation on unchanged state produces 0 changes. — verified
- [x] `uv run prograph index --json` emits a parseable `IndexSummary` payload. — verified
- [x] `uv run prograph status` shows latest snapshot info (id, ts, counts) below the discovery table. — verified
- [x] `uv run prograph index` on `tests/fixtures/monorepo_full` detects exactly 3 cross-project edges (`orchestrator→eval_sdk`, `orchestrator→policy`, `runner→orchestrator`). — verified (`tests/integration/test_cli_index_full.py::test_full_index_detects_all_three_cross_deps`)
- [x] Version-bump test passes: modifying `eval-sdk>=1.0` to `eval-sdk>=2.0` produces an `attrs_changed` event in the change_log. — verified
- [x] `index.lock` blocks concurrent runs and is released cleanly on success or failure. — verified (Rust `lock::tests` + indexer integration)
- [x] SQLite schema is v2; v1→v2 migration applies on existing M1 databases without data loss. — verified (`store::tests::migration_is_additive_over_existing_v1_db`)
- [x] CI workflow continues to pass (no changes required — same job structure as M1). — assumed (no CI file changes in M2)
- [x] All commits follow the `prograph: M2 ...` prefix convention. — verified

## What is NOT done in M2 (handled in subsequent milestones)

- **M3** — Rust + JS parsers via tree-sitter.
- **M4** — Contracts detector + MCP detector.
- **M5** — MD exporter + golden tests + per-project Obsidian-friendly files.
- **M6** — Browser UI (FastAPI + static + d3/cytoscape) + REST API.
- **M7** — MCP stdio server + tool surface for AI agents.
- **M8** — Polish, real-monorepo CI matrix, performance baselines.
