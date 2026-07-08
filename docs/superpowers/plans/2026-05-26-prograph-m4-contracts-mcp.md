# prograph M4 — Contracts + MCP Detectors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After M4, `prograph index` detects two new edge kinds in addition to `package_dep`:

1. **`contract_link`** — when ≥2 projects reference the same JSON Schema / OpenAPI / `.proto` file (matched by `$id`, `info.title`, or content hash). Each contract becomes a first-class graph node.
2. **`mcp_call`** — when project A registers an MCP tool by name and project B invokes it. Tool decls are extracted from Python (`@server.tool()` decorator family) and Rust source via tree-sitter.

On the real `all_ai_orchestrators/` monorepo, M4 should detect at minimum: the shared `_cowork_output/observability-contract` JSON Schema linking Maestro + spec-runner (as `contract_link`), and the Maestro → arbiter MCP relationship (as `mcp_call`) provided arbiter's Rust source registers tools via patterns we recognise.

**Architecture:**
- **Source parsing arrives in M4** (the spec originally pushed this to M5 — we bring it forward because MCP detection genuinely needs it). We introduce `tree-sitter` + `tree-sitter-python` + `tree-sitter-rust` as workspace deps; JS source parsing is deferred (no JS MCP servers in the target monorepo).
- **Contracts detection is pure file-system + JSON/YAML decode** — no AST. Walks each project for `*.json`/`*.yaml`/`*.yml`/`*.proto` files, sniffs structure to classify (JsonSchema vs OpenAPI vs Proto), extracts `$id`/`info.title`/`package` declarations + content hash.
- **Schema v3** is strictly additive: introduces `contracts` + `contract_files` tables and widens the `edges.kind` CHECK from `('package_dep')` to `('package_dep', 'mcp_call', 'contract_link')`. The `from_kind`/`to_kind` constraints already allow `('project', 'contract')` (added in M2). The `change_log.entity_kind` widens from `('project', 'edge')` to `('project', 'edge', 'contract')`. M2 snapshots remain readable; the v3 migration runs additively on top.
- **`EdgeKind` widens** with `McpCall` and `ContractLink` variants (Rust enum + pydantic mirror update + `.pyi` stub).
- **`ParserOutput` gains three new fact lists**: `mcp_decls`, `mcp_uses`, `contracts`. The Python and Rust parsers populate them; JS parser leaves them empty.
- **Two new detectors** (`detectors/contracts.rs`, `detectors/mcp.rs`) compose alongside `detectors/deps.rs` via the existing `detect_all` aggregator.
- **The indexer** gains a Contract diff pass alongside the existing Project + Edge passes. Contracts are keyed by `(declared_id, content_hash)`; identity rule per spec §5.2.

**Tech Stack additions (M4 only):**
- `tree-sitter = "0.22"` — parser engine
- `tree-sitter-python = "0.21"` — Python grammar
- `tree-sitter-rust = "0.21"` — Rust grammar
- `cc = "1"` — already pulled by tree-sitter for C compilation; no explicit add needed
- `walkdir = "2"` — recursive directory walking (avoid hand-rolling)
- `serde_yaml = "0.9"` — parse OpenAPI specs (YAML common)

All new deps are mature crates with MSRV ≤ 1.75 (the existing pin). Tree-sitter grammars compile C code via `cc-rs`; macOS + Linux CI handles this out of the box. Windows is out of scope (the project doesn't target it).

**Spec reference:** `docs/superpowers/specs/2026-05-25-prograph-design.md` — §4.1 detectors (this milestone finishes the three-detector quartet started in M2), §5.1 schema (the v3 we ship here matches the full spec schema for contract entities), §5.2 identity rules (contracts: `(declared_id, content_hash)`; mcp_call: `(kind, from, to, tool)`; contract_link: `(from_project, to_contract)`).

**Baseline:** Branch off `main` at the M3 close commit `85fc660`. 78 cargo + 39 pytest passing; CI green; `prograph index` detects package_dep edges across Python+Rust+JS manifests including PEP 735 dependency-groups and `[tool.prograph].aliases`.

**M4 explicitly out of scope (deferred to M5+):**
- **JS source parsing** for MCP detection — no JS MCP servers in the target monorepo; landing JS would multiply tree-sitter footprint without payoff. Defer until a real driver appears.
- **HTTP / REST runtime edges** — too heuristic without registry of routes. Phase 5+.
- **Tree-sitter for non-MCP purposes** (general module-level facts — public symbols, internal imports) — M5+.
- **Vendored-file detection** — Phase 6.
- **MD export + browser UI + MCP stdio server** — M5/M6/M7.

---

## Splitting strategy

This plan is intentionally large (16 tasks). If the user prefers to ship in two passes, the natural split is:

**M4a — Contracts only** (Tasks 1-3, 7, 8, 10-11 partial, 12-13 partial, 14-16). Ships file-system contract detection + the `contract_link` edge kind. Skips tree-sitter and MCP. Useful on its own — surfaces the shared `_cowork_output/observability-contract` instantly.

**M4b — MCP** (Tasks 4-6, 9, the remainder of 10-13, plus 14-16 redo). Ships source parsing + `mcp_call`.

Sequential execution of the unified plan ends in the same state regardless. The plan below lists everything in dependency order; an implementer doing M4a-only can stop after Task 8 + a polish pass.

---

## File Structure (created/modified in M4)

```
prograph/
├── Cargo.toml                                       # MODIFY — add tree-sitter family + walkdir + serde_yaml
├── prograph-core/
│   ├── Cargo.toml                                   # MODIFY — pull workspace deps in
│   ├── src/
│   │   ├── lib.rs                                   # MODIFY — register new modules + extend exports
│   │   ├── facts.rs                                 # MODIFY — ContractFile, McpToolDecl, McpClientUse
│   │   ├── models.rs                                # MODIFY — EdgeKind variants, Contract pyclass
│   │   ├── store.rs                                 # MODIFY — alive_contracts, contract writers
│   │   ├── indexer.rs                               # MODIFY — Contract diff pass, new edge kinds
│   │   ├── parsers/
│   │   │   ├── mod.rs                               # MODIFY — extend ParserOutput shape
│   │   │   ├── python.rs                            # MODIFY — tree-sitter MCP scan + walk .py
│   │   │   ├── rust.rs                              # MODIFY — tree-sitter MCP scan + walk .rs
│   │   │   ├── js.rs                                # MODIFY — leave mcp fact lists empty (deferred)
│   │   │   └── contracts.rs                         # NEW — file-system contract scanner
│   │   ├── detectors/
│   │   │   ├── mod.rs                               # MODIFY — register new detectors
│   │   │   ├── contracts.rs                         # NEW — group ContractFile → contract_link edges
│   │   │   └── mcp.rs                               # NEW — match McpToolDecl ↔ McpClientUse
│   │   ├── ts_queries/                              # NEW — tree-sitter query strings
│   │   │   ├── python_mcp.scm
│   │   │   └── rust_mcp.scm
│   │   └── migrations/
│   │       └── v3.sql                               # NEW
├── prograph/
│   ├── _core.pyi                                    # MODIFY — Contract pyclass + extended EdgeKind
│   ├── __init__.py                                  # MODIFY — re-export Contract
│   └── models.py                                    # MODIFY — Contract pydantic mirror
├── tests/
│   ├── fixtures/
│   │   └── monorepo_mcp/                            # NEW — synthetic MCP server + client + shared contract
│   │       ├── arbiter_like/
│   │       │   ├── Cargo.toml
│   │       │   └── src/lib.rs
│   │       ├── maestro_like/
│   │       │   ├── pyproject.toml
│   │       │   └── src/maestro_like/__init__.py
│   │       ├── spec_runner_like/
│   │       │   └── pyproject.toml
│   │       └── shared_contract/
│   │           └── obs-v1.json
│   ├── unit/
│   │   └── test_models.py                           # MODIFY — Contract round-trip + extended EdgeKind
│   └── integration/
│       ├── test_cli_index_mcp.py                    # NEW — full pipeline against monorepo_mcp
│       └── test_smoke_real.py                       # MODIFY — assert ≥1 contract_link or mcp_call edge
```

No top-level workflow changes. CI continues to use `uv sync --reinstall-package prograph` for the Rust extension rebuild (same as M2+).

---

## Task 1: Schema v3 — contracts + widened CHECK constraints

**Files:**
- Create: `prograph-core/src/migrations/v3.sql`
- Modify: `prograph-core/src/store.rs` (migration registry)

The v3 migration must:
1. Create `contracts` and `contract_files` tables (full spec §5.1).
2. Widen `edges.kind` CHECK from `('package_dep')` to `('package_dep', 'mcp_call', 'contract_link')`.
3. Widen `change_log.entity_kind` CHECK from `('project', 'edge')` to `('project', 'edge', 'contract')`.

SQLite doesn't support `ALTER TABLE ... DROP CONSTRAINT` or `ADD CONSTRAINT` in modern syntax, so widening a CHECK requires the dance: rename → create new with widened constraint → INSERT...SELECT → drop old. The migration script handles both tables.

- [ ] **Step 1: Write `v3.sql`**

`prograph-core/src/migrations/v3.sql`:
```sql
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
```

- [ ] **Step 2: Register in the migration runner**

Edit `prograph-core/src/store.rs`. Find the `MIGRATIONS` constant and append the v3 entry:
```rust
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("migrations/v1.sql")),
    (2, include_str!("migrations/v2.sql")),
    (3, include_str!("migrations/v3.sql")),
];
```

- [ ] **Step 3: Add tests in `store.rs`**

Append to `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn schema_v3_creates_contracts_tables() {
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
        assert!(names.contains(&"contracts".to_string()));
        assert!(names.contains(&"contract_files".to_string()));
        assert_eq!(store.schema_version().unwrap(), 3);
    }

    #[test]
    fn schema_v3_widens_edges_kind_check() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();
        // Insert a snapshot + a project + an mcp_call edge — should succeed under v3.
        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid_a = writer.insert_project(snap, "a", "./a", "python", "{}").unwrap();
        let pid_b = writer.insert_project(snap, "b", "./b", "python", "{}").unwrap();
        writer.insert_edge(snap, "mcp_call", "project", pid_a, "project", pid_b, "{}", "h").unwrap();
        writer.insert_edge(snap, "contract_link", "project", pid_a, "contract", 999, "{}", "h2").unwrap();
        writer.commit().unwrap();
    }

    #[test]
    fn migration_v2_to_v3_preserves_existing_edges() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("g.db");

        // Bootstrap a v2 DB by hand and insert a row in the v2 edges table.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(include_str!("migrations/v1.sql")).unwrap();
            conn.execute_batch(include_str!("migrations/v2.sql")).unwrap();
            conn.execute(
                "INSERT INTO snapshots (ts, monorepo_root, prograph_version) VALUES (?, ?, ?)",
                rusqlite::params!["ts", "/m", "0.1.0"],
            ).unwrap();
            conn.execute(
                "INSERT INTO projects (name, root_path, kind, attrs_json, first_seen, last_seen)
                 VALUES (?, ?, ?, ?, 1, 1)",
                rusqlite::params!["a", "./a", "python", "{}"],
            ).unwrap();
            conn.execute(
                "INSERT INTO projects (name, root_path, kind, attrs_json, first_seen, last_seen)
                 VALUES (?, ?, ?, ?, 1, 1)",
                rusqlite::params!["b", "./b", "python", "{}"],
            ).unwrap();
            conn.execute(
                "INSERT INTO edges (kind, from_kind, from_id, to_kind, to_id, attrs_json, attrs_hash, first_seen, last_seen)
                 VALUES ('package_dep', 'project', 1, 'project', 2, '{}', 'h', 1, 1)",
                [],
            ).unwrap();
        }

        // Open via Store — v3 migration runs.
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 3);

        let edge_count: i64 = store
            .connection()
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .unwrap();
        assert_eq!(edge_count, 1, "existing package_dep edge must survive v2 → v3 migration");
    }
```

- [ ] **Step 4: Run cargo tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core store
```
Expected: 12 store tests pass (9 prior + 3 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 81 tests (78 prior + 3 new).

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/migrations/v3.sql prograph/prograph-core/src/store.rs
git commit -m "prograph: M4 schema v3 — contracts + contract_files + widened CHECK on edges.kind / change_log.entity_kind"
```

---

## Task 2: Workspace deps — tree-sitter family, walkdir, serde_yaml

**Files:**
- Modify: `prograph/Cargo.toml`
- Modify: `prograph-core/Cargo.toml`

- [ ] **Step 1: Add workspace dependencies**

Edit `prograph/Cargo.toml`. Append to `[workspace.dependencies]`:
```toml
tree-sitter = "0.22"
tree-sitter-python = "0.21"
tree-sitter-rust = "0.21"
walkdir = "2"
serde_yaml = "0.9"
sha2 = "0.10"  # already there from M2 — verify no duplicate; otherwise no-op
```

(If `sha2` is already in workspace deps from M2, skip the duplicate line.)

- [ ] **Step 2: Reference them in the crate**

Edit `prograph-core/Cargo.toml`. Append to `[dependencies]`:
```toml
tree-sitter = { workspace = true }
tree-sitter-python = { workspace = true }
tree-sitter-rust = { workspace = true }
walkdir = { workspace = true }
serde_yaml = { workspace = true }
```

- [ ] **Step 3: Confirm cargo build resolves**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo metadata --no-deps --format-version 1 > /dev/null
cargo build --package prograph-core 2>&1 | tail -10
```

Expected: build succeeds. Tree-sitter grammar crates ship C source that `cc-rs` will compile — first run takes ~1 minute. Look for any MSRV violations or transitive dep edition2024 issues; if encountered, apply the same pinning pattern as M1's getrandom or M2's indexmap and document.

Also verify tests:
```sh
cargo test --package prograph-core
```
Expected: still 81 pass (no semantic change yet).

- [ ] **Step 4: Commit**

```sh
git add prograph/Cargo.toml prograph/prograph-core/Cargo.toml prograph/Cargo.lock
git commit -m "prograph: M4 add tree-sitter + walkdir + serde_yaml workspace dependencies"
```

If you had to apply MSRV pins for transitive deps, fold them into the same commit and document in the commit body.

---

## Task 3: Facts extension — `ContractFile`, `McpToolDecl`, `McpClientUse`

**Files:**
- Modify: `prograph-core/src/facts.rs`
- Modify: `prograph-core/src/parsers/mod.rs` (extend `ParserOutput`)

- [ ] **Step 1: Extend `facts.rs`**

Append to `prograph-core/src/facts.rs` (after the existing `ProjectFacts` definition):

```rust
/// A JSON Schema / OpenAPI / .proto file discovered inside a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractFile {
    /// Path relative to the project root, e.g. "schemas/obs-v1.json".
    pub rel_path: String,
    pub kind: ContractKind,
    /// Optional declared identifier — `$id` for JSON Schema, `info.title` for OpenAPI,
    /// `package` name for .proto. None when undeclared.
    pub declared_id: Option<String>,
    /// SHA256 hex of the canonicalized contract content. Used as a fallback identity
    /// key when `declared_id` is None, AND as part of the contract's full identity per
    /// spec §5.2 (declared_id, content_hash).
    pub content_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractKind {
    JsonSchema,
    OpenApi,
    Proto,
}

impl ContractKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContractKind::JsonSchema => "json_schema",
            ContractKind::OpenApi => "openapi",
            ContractKind::Proto => "proto",
        }
    }
}

/// An MCP tool registered by a project (server-side declaration).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolDecl {
    pub tool_name: String,
    /// File path relative to the project root, e.g. "src/server.py".
    pub rel_path: String,
    /// 1-based line number where the declaration starts. Best-effort.
    pub line: u32,
}

/// An MCP tool invocation by a project (client-side usage).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpClientUse {
    pub tool_name: String,
    pub rel_path: String,
    pub line: u32,
}
```

Then extend `ProjectFacts` to carry the new fact lists:

```rust
pub struct ProjectFacts {
    pub project_root: String,
    pub project_name: String,
    pub manifest: Option<Manifest>,
    pub warnings: Vec<ParseWarning>,
    pub parse_status: ParseStatus,
    /// MCP tool declarations found in this project's source (M4+).
    #[serde(default)]
    pub mcp_decls: Vec<McpToolDecl>,
    /// MCP tool usages found in this project's source (M4+).
    #[serde(default)]
    pub mcp_uses: Vec<McpClientUse>,
    /// Contract files found in this project (M4+).
    #[serde(default)]
    pub contracts: Vec<ContractFile>,
}
```

The `#[serde(default)]` on each new field preserves backward-compatibility — projects.attrs_json snapshots from M2/M3 deserialize cleanly (the new fields default to `Vec::new()`).

- [ ] **Step 2: Add inline tests**

Append to `facts.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn contract_kind_as_str_matches_schema_check() {
        assert_eq!(ContractKind::JsonSchema.as_str(), "json_schema");
        assert_eq!(ContractKind::OpenApi.as_str(), "openapi");
        assert_eq!(ContractKind::Proto.as_str(), "proto");
    }

    #[test]
    fn project_facts_round_trip_with_new_fields() {
        let f = ProjectFacts {
            project_root: "./p".into(),
            project_name: "p".into(),
            manifest: None,
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: vec![McpToolDecl {
                tool_name: "decide".into(),
                rel_path: "src/lib.rs".into(),
                line: 42,
            }],
            mcp_uses: vec![],
            contracts: vec![ContractFile {
                rel_path: "schemas/obs.json".into(),
                kind: ContractKind::JsonSchema,
                declared_id: Some("https://example.org/schemas/obs-v1".into()),
                content_hash: "deadbeef".repeat(8),
            }],
        };
        let json = serde_json::to_string(&f).unwrap();
        let back: ProjectFacts = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mcp_decls.len(), 1);
        assert_eq!(back.contracts.len(), 1);
        assert_eq!(back.contracts[0].kind, ContractKind::JsonSchema);
    }

    #[test]
    fn project_facts_back_compat_without_new_fields() {
        // M2/M3 serialized ProjectFacts without mcp_decls/mcp_uses/contracts.
        let json = r#"{
            "project_root": "./p",
            "project_name": "p",
            "manifest": null,
            "warnings": [],
            "parse_status": "Ok"
        }"#;
        let back: ProjectFacts = serde_json::from_str(json).unwrap();
        assert!(back.mcp_decls.is_empty());
        assert!(back.mcp_uses.is_empty());
        assert!(back.contracts.is_empty());
    }
```

- [ ] **Step 3: Extend `ParserOutput` shape in `parsers/mod.rs`**

Edit `prograph-core/src/parsers/mod.rs`. Replace the existing `ParserOutput` struct:
```rust
pub struct ParserOutput {
    pub manifest: Option<Manifest>,
    pub warnings: Vec<ParseWarning>,
    /// MCP server-side tool decls extracted from source. Populated by M4+ parsers.
    pub mcp_decls: Vec<crate::facts::McpToolDecl>,
    /// MCP client-side tool invocations. Populated by M4+ parsers.
    pub mcp_uses: Vec<crate::facts::McpClientUse>,
    /// Contract files found in this project. Populated by M4+ parsers.
    pub contracts: Vec<crate::facts::ContractFile>,
}
```

Every existing `Ok(ParserOutput { manifest: ..., warnings: ... })` construction in `python.rs`, `rust.rs`, and `js.rs` is now incomplete. Fix them by appending three empty-Vec defaults to each construction:

Use a sed-style global search to find the pattern. In each parser file, find every `ParserOutput { manifest: ..., warnings: ... }` literal and add:
```rust
mcp_decls: vec![],
mcp_uses: vec![],
contracts: vec![],
```

The Python/Rust parsers will populate these later in M4 Tasks 4-6. The JS parser leaves them empty (out of M4 scope).

- [ ] **Step 4: Run tests**

```sh
cargo test --package prograph-core
```
Expected: 84 tests (81 prior + 3 new in facts). All existing parser tests must still pass — the `ParserOutput` change is additive.

Verify clean:
```sh
cargo fmt --all -- --check
cargo clippy --package prograph-core --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/facts.rs prograph/prograph-core/src/parsers/mod.rs \
        prograph/prograph-core/src/parsers/python.rs prograph/prograph-core/src/parsers/rust.rs \
        prograph/prograph-core/src/parsers/js.rs
git commit -m "prograph: M4 facts — ContractFile/ContractKind/McpToolDecl/McpClientUse + ParserOutput extensions"
```

---

## Task 4: Models — extend `EdgeKind`, add `Contract` pyclass

**Files:**
- Modify: `prograph-core/src/models.rs`
- Modify: `prograph-core/src/lib.rs`
- Modify: `prograph/_core.pyi`
- Modify: `prograph/models.py`
- Modify: `prograph/__init__.py`
- Modify: `tests/unit/test_models.py`

- [ ] **Step 1: Widen `EdgeKind` in `models.rs`**

Find the `EdgeKind` enum in `prograph-core/src/models.rs`. Currently:
```rust
pub enum EdgeKind {
    PackageDep,
}
```

Extend:
```rust
pub enum EdgeKind {
    PackageDep,
    McpCall,
    ContractLink,
}
```

Also extend the `name()` impl:
```rust
    fn name(&self) -> &'static str {
        match self {
            EdgeKind::PackageDep => "package_dep",
            EdgeKind::McpCall => "mcp_call",
            EdgeKind::ContractLink => "contract_link",
        }
    }
```

- [ ] **Step 2: Add `Contract` pyclass**

Append to `prograph-core/src/models.rs` (after `SnapshotInfo`):
```rust
/// A persisted contract node — JSON Schema / OpenAPI / .proto file shared across projects.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct Contract {
    pub id: i64,
    /// Optional declared identifier (JSON Schema $id, OpenAPI info.title, .proto package).
    pub declared_id: Option<String>,
    pub content_hash: String,
    /// One of "json_schema", "openapi", "proto".
    pub kind: String,
    pub first_seen: i64,
    pub last_seen: i64,
}

#[pymethods]
impl Contract {
    fn __repr__(&self) -> String {
        format!(
            "Contract(id={}, kind={}, declared_id={:?}, hash={}...)",
            self.id,
            self.kind,
            self.declared_id,
            &self.content_hash[..self.content_hash.len().min(8)]
        )
    }
}
```

- [ ] **Step 3: Register the pyclass + extend exports**

In `prograph-core/src/lib.rs`, extend the `pub use models::{...}` line to include `Contract`:
```rust
pub use models::{
    ChangeEvent, ChangeKind, Contract, Edge, EdgeKind, EntityKind, IndexSummary, NodeKind,
    ProjectCandidate, ProjectKind, SnapshotInfo,
};
```

And inside `#[pymodule]`:
```rust
    m.add_class::<Contract>()?;
```

- [ ] **Step 4: Extend `_core.pyi`**

Edit `prograph/_core.pyi`. Find the `EdgeKind` stub and extend its ClassVars:
```python
class EdgeKind:
    PackageDep: ClassVar[EdgeKind]
    McpCall: ClassVar[EdgeKind]
    ContractLink: ClassVar[EdgeKind]
    def name(self) -> str: ...
```

Append after `SnapshotInfo`:
```python
class Contract:
    id: int
    declared_id: str | None
    content_hash: str
    kind: str
    first_seen: int
    last_seen: int
```

- [ ] **Step 5: Extend pydantic mirrors**

Edit `prograph/models.py`. Find the `EdgeKind` pydantic enum and add the variants:
```python
class EdgeKind(str, Enum):
    PACKAGE_DEP = "package_dep"
    MCP_CALL = "mcp_call"
    CONTRACT_LINK = "contract_link"

    @classmethod
    def from_core(cls, value: _core.EdgeKind) -> EdgeKind:
        return cls(value.name())
```

Append a `Contract` BaseModel:
```python
class Contract(BaseModel):
    """A shared contract (JSON Schema / OpenAPI / .proto) referenced by ≥2 projects."""

    model_config = ConfigDict(frozen=True)

    id: int
    declared_id: str | None
    content_hash: str
    kind: str
    first_seen: int
    last_seen: int

    @classmethod
    def from_core(cls, value: _core.Contract) -> Contract:
        return cls(
            id=value.id,
            declared_id=value.declared_id,
            content_hash=value.content_hash,
            kind=value.kind,
            first_seen=value.first_seen,
            last_seen=value.last_seen,
        )
```

- [ ] **Step 6: Re-export from `__init__.py`**

Extend the import and `__all__` in `prograph/__init__.py` to include `Contract`:
```python
from prograph.models import (
    ChangeEvent,
    ChangeKind,
    Contract,
    Edge,
    EdgeKind,
    EntityKind,
    IndexSummary,
    NodeKind,
    ProjectCandidate,
    ProjectKind,
    SnapshotInfo,
)
```

And add `"Contract"` to `__all__` (alphabetical).

- [ ] **Step 7: Add round-trip tests**

Append to `tests/unit/test_models.py`:
```python
def test_edge_kind_round_trip_extended():
    """M4: EdgeKind gained McpCall and ContractLink."""
    assert EdgeKind.from_core(_core.EdgeKind.PackageDep) is EdgeKind.PACKAGE_DEP
    assert EdgeKind.from_core(_core.EdgeKind.McpCall) is EdgeKind.MCP_CALL
    assert EdgeKind.from_core(_core.EdgeKind.ContractLink) is EdgeKind.CONTRACT_LINK


def test_contract_pydantic_mirror_round_trip():
    """M4: Contract pyclass round-trips through pydantic."""
    raw = _core.Contract(
        id=42,
        declared_id="https://example.org/schemas/obs-v1",
        content_hash="a" * 64,
        kind="json_schema",
        first_seen=1,
        last_seen=3,
    )
    # NOTE: _core.Contract has no #[new] — we can't construct it from Python.
    # Instead just verify the pydantic shape via a direct construction.
    from prograph.models import Contract
    c = Contract(
        id=42,
        declared_id="https://example.org/schemas/obs-v1",
        content_hash="a" * 64,
        kind="json_schema",
        first_seen=1,
        last_seen=3,
    )
    assert c.kind == "json_schema"
    assert c.declared_id == "https://example.org/schemas/obs-v1"
```

The Contract pyclass intentionally has no `#[new]` constructor — it's only ever produced by `Store::alive_contracts` (Task 10). The test exercises the pydantic mirror shape; the round-trip from a real `_core.Contract` is exercised in Task 11's integration tests.

Also add `Contract` to the top-of-file imports:
```python
from prograph import (
    ChangeKind,
    Contract,
    EdgeKind,
    EntityKind,
    NodeKind,
    ProjectCandidate,
    ProjectKind,
    _core,
)
```

(Delete the unused `_core.Contract` reference from the test if pyrefly flags it.)

- [ ] **Step 8: Rebuild and run tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv sync --reinstall-package prograph
cargo test --package prograph-core
uv run pytest tests/unit/test_models.py -v
uv run pytest -v
```

Expected:
- cargo: 84 (unchanged from Task 3; no new Rust tests, but no regression).
- models tests: 9 (7 from M3 + 2 new).
- full pytest: 41 (39 prior + 2 new).

Verify clean:
```sh
cargo fmt --all -- --check
cargo clippy --package prograph-core --all-targets -- -D warnings
uv run ruff check .
```

- [ ] **Step 9: Commit**

```sh
git add prograph/prograph-core/src/models.rs prograph/prograph-core/src/lib.rs \
        prograph/prograph/_core.pyi prograph/prograph/models.py prograph/prograph/__init__.py \
        prograph/tests/unit/test_models.py
git commit -m "prograph: M4 models — EdgeKind {McpCall, ContractLink} + Contract pyclass + pydantic mirror"
```

---

## Task 5: tree-sitter query files

**Files:**
- Create: `prograph-core/src/ts_queries/python_mcp.scm`
- Create: `prograph-core/src/ts_queries/rust_mcp.scm`

Tree-sitter queries are S-expression patterns over the parse tree. The .scm files contain the queries used by the Python and Rust source scanners (Tasks 6, 7).

**Detection patterns covered in M4:**

For Python (canonical anthropic / mcp Python SDK):
- `@<expr>.tool()` and `@<expr>.tool` decorators (FastMCP idiom)
- `<expr>.add_tool(name="...", ...)` and `<expr>.register_tool("...", ...)` calls
- Client usage: `<expr>.call_tool("...", ...)` and `<expr>.invoke_tool("...", ...)`

For Rust (heuristic for common patterns):
- `.tool(...)`, `.register_tool(...)` calls with a string literal first arg
- `Tool::new("...")`, `ToolBuilder::new("...")` constructions
- Client usage: `.call_tool("...", ...)` calls

These patterns are best-effort. Real-world MCP server code varies. M4 covers the common idioms; M5+ can add more via config.

- [ ] **Step 1: Write `python_mcp.scm`**

`prograph-core/src/ts_queries/python_mcp.scm`:
```scheme
; Server-side tool declarations
;
; Pattern 1: `@server.tool(...)` or `@server.tool` decorator on a function definition.
; The decorator's last attribute name is `tool`, and we capture the function name.
(decorated_definition
  (decorator
    (call
      function: (attribute attribute: (identifier) @decorator_name)
      (#eq? @decorator_name "tool")))
  definition: (function_definition name: (identifier) @tool_name)) @tool_decl

; Pattern 2: bare attribute decorator like `@server.tool` (no call).
(decorated_definition
  (decorator
    (attribute attribute: (identifier) @decorator_name)
    (#eq? @decorator_name "tool"))
  definition: (function_definition name: (identifier) @tool_name)) @tool_decl_bare

; Pattern 3: `server.add_tool("name", ...)` or `server.register_tool("name", ...)`.
(call
  function: (attribute attribute: (identifier) @method)
  arguments: (argument_list . (string) @tool_name_literal)
  (#match? @method "^(add_tool|register_tool)$")) @tool_decl_call

; Client-side tool invocations
;
; Pattern: `<expr>.call_tool("name", ...)` or `<expr>.invoke_tool("name", ...)`.
(call
  function: (attribute attribute: (identifier) @method)
  arguments: (argument_list . (string) @tool_name_literal)
  (#match? @method "^(call_tool|invoke_tool)$")) @tool_use_call
```

- [ ] **Step 2: Write `rust_mcp.scm`**

`prograph-core/src/ts_queries/rust_mcp.scm`:
```scheme
; Server-side tool declarations
;
; Pattern 1: `.tool("name", ...)`, `.register_tool("name", ...)`, `.add_tool("name", ...)`
; — method-call form where the first arg is a string literal.
(call_expression
  function: (field_expression field: (field_identifier) @method)
  arguments: (arguments . (string_literal) @tool_name_literal)
  (#match? @method "^(tool|register_tool|add_tool)$")) @tool_decl_method

; Pattern 2: `Tool::new("name")` or `ToolBuilder::new("name")` — associated function call.
(call_expression
  function: (scoped_identifier
    path: (identifier) @type
    name: (identifier) @method)
  arguments: (arguments . (string_literal) @tool_name_literal)
  (#match? @type "^(Tool|ToolBuilder|ToolHandler)$")
  (#eq? @method "new")) @tool_decl_assoc

; Client-side tool invocations
;
; Pattern: `.call_tool("name", ...)` or `.invoke_tool("name", ...)` method call.
(call_expression
  function: (field_expression field: (field_identifier) @method)
  arguments: (arguments . (string_literal) @tool_name_literal)
  (#match? @method "^(call_tool|invoke_tool)$")) @tool_use_method
```

- [ ] **Step 3: Commit**

(No tests for query files in isolation; Tasks 6 + 7 exercise them.)

```sh
git add prograph/prograph-core/src/ts_queries/
git commit -m "prograph: M4 tree-sitter query files — python_mcp.scm + rust_mcp.scm"
```

---

## Task 6: Python source scanner — populate `mcp_decls` + `mcp_uses`

**Files:**
- Modify: `prograph-core/src/parsers/python.rs`

Walk every `.py` file under the project root via `walkdir`. For each file, parse with `tree-sitter-python`, run the queries from `python_mcp.scm`, collect `McpToolDecl` and `McpClientUse` facts. Skip well-known noise dirs (`.venv`, `__pycache__`, `node_modules`, `target`, `dist`, `build`).

- [ ] **Step 1: Add the source scan to `python.rs`**

In `prograph-core/src/parsers/python.rs`, add at the top:
```rust
use tree_sitter::{Language, Parser, Query, QueryCursor};
use walkdir::WalkDir;

use crate::facts::{McpClientUse, McpToolDecl};
```

Then add a new private helper function below the existing `parse_pep508` / `strip_extras` helpers:

```rust
/// Walk all .py files under `project_root` and extract MCP tool decls + uses.
/// Errors are silently swallowed per-file (logged as ParseWarning) so one malformed
/// file doesn't abort the whole project scan.
fn scan_python_source(project_root: &Path) -> (Vec<McpToolDecl>, Vec<McpClientUse>, Vec<crate::facts::ParseWarning>) {
    let language: Language = tree_sitter_python::language();
    let query_src = include_str!("../ts_queries/python_mcp.scm");

    let query = match Query::new(&language, query_src) {
        Ok(q) => q,
        Err(e) => {
            return (
                vec![],
                vec![],
                vec![crate::facts::ParseWarning {
                    rel_path: "ts_queries/python_mcp.scm".into(),
                    message: format!("failed to compile tree-sitter query: {}", e),
                }],
            );
        }
    };

    let mut decls = Vec::new();
    let mut uses = Vec::new();
    let mut warnings = Vec::new();

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        warnings.push(crate::facts::ParseWarning {
            rel_path: "<tree-sitter init>".into(),
            message: "failed to initialise tree-sitter-python".into(),
        });
        return (decls, uses, warnings);
    }

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        // Skip noise dirs by name.
        let name = e.file_name().to_string_lossy();
        !matches!(
            name.as_ref(),
            ".venv" | "__pycache__" | "node_modules" | "target" | "dist" | "build" | ".git"
        ) && !name.starts_with('.')
            || e.depth() == 0 // always descend the root itself
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("py") {
            continue;
        }

        let rel_path = entry
            .path()
            .strip_prefix(project_root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");

        let source = match std::fs::read_to_string(entry.path()) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let tree = match parser.parse(&source, None) {
            Some(t) => t,
            None => {
                warnings.push(crate::facts::ParseWarning {
                    rel_path: rel_path.clone(),
                    message: "tree-sitter failed to parse".into(),
                });
                continue;
            }
        };

        let source_bytes = source.as_bytes();
        let mut cursor = QueryCursor::new();

        for m in cursor.matches(&query, tree.root_node(), source_bytes) {
            // Determine the pattern name from the captures present.
            let mut tool_name: Option<String> = None;
            let mut tool_name_from_literal: Option<String> = None;
            let mut tool_name_from_ident: Option<String> = None;
            let mut start_line: u32 = 1;

            for capture in m.captures {
                let cap_name = &query.capture_names()[capture.index as usize];
                let text = capture
                    .node
                    .utf8_text(source_bytes)
                    .unwrap_or("")
                    .to_string();
                start_line = capture.node.start_position().row as u32 + 1;

                if cap_name == "tool_name" {
                    // Function name from @server.tool() decorator pattern (identifier capture).
                    tool_name_from_ident = Some(text);
                } else if cap_name == "tool_name_literal" {
                    // String literal — strip surrounding quotes.
                    let stripped = text
                        .trim_start_matches(['"', '\''])
                        .trim_end_matches(['"', '\''])
                        .to_string();
                    tool_name_from_literal = Some(stripped);
                }
            }

            tool_name = tool_name_from_literal.or(tool_name_from_ident);

            let Some(name) = tool_name else { continue };

            // Decide whether this match is a decl or a use by inspecting which named
            // pattern in the query was matched. We use the @-capture's enclosing
            // pattern-name via the last entry — simplest is to test for the marker
            // capture names we set on the outer alternation.
            let mut is_use = false;
            for capture in m.captures {
                let cap_name = &query.capture_names()[capture.index as usize];
                if cap_name == "tool_use_call" {
                    is_use = true;
                    break;
                }
            }

            if is_use {
                uses.push(McpClientUse {
                    tool_name: name,
                    rel_path: rel_path.clone(),
                    line: start_line,
                });
            } else {
                decls.push(McpToolDecl {
                    tool_name: name,
                    rel_path: rel_path.clone(),
                    line: start_line,
                });
            }
        }
    }

    (decls, uses, warnings)
}
```

- [ ] **Step 2: Wire the scan into `parse()`**

In `prograph-core/src/parsers/python.rs`, find the existing `parse` function. At the very end (after constructing the `Manifest`), call `scan_python_source` and populate the new `ParserOutput` fields:

Replace the final `Ok(ParserOutput { ... })` literal with:
```rust
    let (mcp_decls, mcp_uses, scan_warnings) = scan_python_source(project_root);
    let mut all_warnings = vec![];
    all_warnings.extend(scan_warnings);

    Ok(ParserOutput {
        manifest: Some(Manifest {
            declared_name,
            version: project.version,
            declared_deps,
            aliases,
        }),
        warnings: all_warnings,
        mcp_decls,
        mcp_uses,
        contracts: vec![],
    })
```

Also update the early-return branches (no pyproject.toml / no [project] table / no name) so they include empty `mcp_decls`/`mcp_uses`/`contracts` fields. The compiler will catch missing fields after Task 3's `ParserOutput` extension — fix them all in one pass.

- [ ] **Step 3: Add tests**

Append to the `#[cfg(test)] mod tests` block in `python.rs`:
```rust
    #[test]
    fn scans_fastmcp_tool_decorator() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "server-proj"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("server.py"),
            r#"from mcp.server.fastmcp import FastMCP

server = FastMCP("test")

@server.tool()
def my_tool(x: int) -> int:
    return x + 1
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(names.contains(&"my_tool"), "expected my_tool decl, got: {:?}", names);
    }

    #[test]
    fn scans_call_tool_invocation() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "client-proj"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("client.py"),
            r#"async def run(session):
    result = await session.call_tool("decide", arguments={"x": 1})
    return result
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_uses.iter().map(|u| u.tool_name.as_str()).collect();
        assert!(names.contains(&"decide"), "expected decide use, got: {:?}", names);
    }

    #[test]
    fn skips_venv_and_pycache() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "x"
"#,
        )
        .unwrap();
        // A "trap" file inside .venv that would produce false-positive decls if scanned.
        fs::create_dir_all(dir.path().join(".venv/lib")).unwrap();
        fs::write(
            dir.path().join(".venv/lib/trap.py"),
            r#"@server.tool()
def trap_tool(): pass
"#,
        )
        .unwrap();
        // Real file at the root.
        fs::write(
            dir.path().join("real.py"),
            r#"@server.tool()
def real_tool(): pass
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(names.contains(&"real_tool"));
        assert!(!names.contains(&"trap_tool"), "scanner must skip .venv");
    }

    #[test]
    fn scan_records_line_numbers() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "x"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("s.py"),
            "\n\n\n@server.tool()\ndef widget(): pass\n",
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let decl = out.mcp_decls.iter().find(|d| d.tool_name == "widget").unwrap();
        assert!(decl.line >= 4, "expected line >= 4, got {}", decl.line);
    }
```

- [ ] **Step 4: Run cargo tests**

```sh
cargo test --package prograph-core parsers
```
Expected: 28 parsers tests (24 from M3 + 4 new). The first run pulls + compiles tree-sitter-python (~30-60 seconds).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 88 tests (84 from Task 4 + 4 new).

Verify clean:
```sh
cargo fmt --all -- --check
cargo clippy --package prograph-core --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/parsers/python.rs
git commit -m "prograph: M4 Python source scanner — extracts MCP tool decls + uses via tree-sitter"
```

---

## Task 7: Rust source scanner — populate `mcp_decls` + `mcp_uses`

**Files:**
- Modify: `prograph-core/src/parsers/rust.rs`

Mirror of Task 6 but for `.rs` files using `tree-sitter-rust` and `rust_mcp.scm`.

- [ ] **Step 1: Add `scan_rust_source` to `rust.rs`**

At the top of `prograph-core/src/parsers/rust.rs`, add:
```rust
use tree_sitter::{Language, Parser, Query, QueryCursor};
use walkdir::WalkDir;

use crate::facts::{McpClientUse, McpToolDecl};
```

Then add the scanner function as a private helper at the bottom of the file (before `#[cfg(test)]`):

```rust
/// Walk all .rs files under `project_root` and extract MCP tool decls + uses.
/// Per-file parse errors are swallowed as ParseWarnings so one malformed file doesn't
/// abort the whole project scan.
fn scan_rust_source(project_root: &Path) -> (Vec<McpToolDecl>, Vec<McpClientUse>, Vec<crate::facts::ParseWarning>) {
    let language: Language = tree_sitter_rust::language();
    let query_src = include_str!("../ts_queries/rust_mcp.scm");

    let query = match Query::new(&language, query_src) {
        Ok(q) => q,
        Err(e) => {
            return (
                vec![],
                vec![],
                vec![crate::facts::ParseWarning {
                    rel_path: "ts_queries/rust_mcp.scm".into(),
                    message: format!("failed to compile tree-sitter query: {}", e),
                }],
            );
        }
    };

    let mut decls = Vec::new();
    let mut uses = Vec::new();
    let mut warnings = Vec::new();

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        warnings.push(crate::facts::ParseWarning {
            rel_path: "<tree-sitter init>".into(),
            message: "failed to initialise tree-sitter-rust".into(),
        });
        return (decls, uses, warnings);
    }

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !matches!(
            name.as_ref(),
            "target" | "node_modules" | ".venv" | "dist" | "build" | ".git" | "__pycache__"
        ) && !name.starts_with('.')
            || e.depth() == 0
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }

        let rel_path = entry
            .path()
            .strip_prefix(project_root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");

        let source = match std::fs::read_to_string(entry.path()) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let tree = match parser.parse(&source, None) {
            Some(t) => t,
            None => {
                warnings.push(crate::facts::ParseWarning {
                    rel_path: rel_path.clone(),
                    message: "tree-sitter failed to parse".into(),
                });
                continue;
            }
        };

        let source_bytes = source.as_bytes();
        let mut cursor = QueryCursor::new();

        for m in cursor.matches(&query, tree.root_node(), source_bytes) {
            let mut tool_name: Option<String> = None;
            let mut start_line: u32 = 1;
            let mut is_use = false;

            for capture in m.captures {
                let cap_name = &query.capture_names()[capture.index as usize];
                let text = capture
                    .node
                    .utf8_text(source_bytes)
                    .unwrap_or("")
                    .to_string();
                start_line = capture.node.start_position().row as u32 + 1;

                if cap_name == "tool_name_literal" {
                    // string_literal node text in Rust includes the surrounding quotes —
                    // and may include `r"..."` prefixes. Strip conservatively.
                    let stripped = text
                        .trim_start_matches('r')
                        .trim_start_matches(['"', '\''])
                        .trim_end_matches(['"', '\''])
                        .to_string();
                    tool_name = Some(stripped);
                } else if cap_name == "tool_use_method" {
                    is_use = true;
                }
            }

            let Some(name) = tool_name else { continue };

            if is_use {
                uses.push(McpClientUse {
                    tool_name: name,
                    rel_path: rel_path.clone(),
                    line: start_line,
                });
            } else {
                decls.push(McpToolDecl {
                    tool_name: name,
                    rel_path: rel_path.clone(),
                    line: start_line,
                });
            }
        }
    }

    (decls, uses, warnings)
}
```

The structure is identical to Python's `scan_python_source` (Task 6) — only the language binding, query filename, file extension filter, ignore-dir list, and quote-stripping logic differ. The pattern-name detection here looks for the `@tool_use_method` capture rather than `@tool_use_call`.

- [ ] **Step 2: Wire the scan into `parse()`**

In `prograph-core/src/parsers/rust.rs`, find the existing `parse` function's final `Ok(ParserOutput { ... })`. Call `scan_rust_source` and populate the new fields the same way Task 6 did for Python.

Also update the early-return branches to include empty `mcp_decls`/`mcp_uses`/`contracts` fields (as Task 3 required).

- [ ] **Step 3: Add tests**

Append to `rust.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn scans_rust_tool_method_decl() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "rust-server"
version = "0.1.0"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            r#"pub fn setup(builder: &mut ServerBuilder) {
    builder.tool("decide", |args| Ok(()));
    builder.register_tool("evaluate", |args| Ok(()));
}
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(names.contains(&"decide"), "got: {:?}", names);
        assert!(names.contains(&"evaluate"), "got: {:?}", names);
    }

    #[test]
    fn scans_rust_tool_associated_fn_decl() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "rust-server"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/main.rs"),
            r#"fn build() {
    let t = Tool::new("hello");
    let b = ToolBuilder::new("goodbye");
}
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"goodbye"));
    }

    #[test]
    fn scans_rust_call_tool_invocation() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "rust-client"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            r#"async fn use_tool(client: &Client) {
    let _ = client.call_tool("decide", args).await;
}
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_uses.iter().map(|u| u.tool_name.as_str()).collect();
        assert!(names.contains(&"decide"));
    }

    #[test]
    fn skips_target_directory() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "x"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("target/debug")).unwrap();
        fs::write(
            dir.path().join("target/debug/trap.rs"),
            r#"fn x() { Tool::new("trap"); }"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            r#"fn x() { Tool::new("real"); }"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(names.contains(&"real"));
        assert!(!names.contains(&"trap"), "scanner must skip target/");
    }
```

- [ ] **Step 4: Run tests**

```sh
cargo test --package prograph-core parsers
```
Expected: 32 parsers tests (28 + 4 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 92 tests.

Verify clean.

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/parsers/rust.rs
git commit -m "prograph: M4 Rust source scanner — extracts MCP tool decls + uses via tree-sitter"
```

---

## Task 8: Contracts file scanner

**Files:**
- Create: `prograph-core/src/parsers/contracts.rs`
- Modify: `prograph-core/src/parsers/mod.rs`

File-system scan that walks each project root, classifies any `.json`/`.yaml`/`.yml`/`.proto` file as a candidate contract, extracts the declared id, and computes a content hash. Called from `parse_project` after the language-specific parser runs.

- [ ] **Step 1: Write `contracts.rs`**

`prograph-core/src/parsers/contracts.rs`:
```rust
//! Contract-file scanner — finds JSON Schema, OpenAPI, and .proto files inside a project
//! and classifies them. Pure file-system + sniffing; no AST.

use std::path::Path;

use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::facts::{ContractFile, ContractKind};

/// Walk `project_root` for contract files. Returns the list of detected contracts.
/// Hidden + build-artefact dirs are skipped.
pub fn scan(project_root: &Path) -> Vec<ContractFile> {
    let mut out = Vec::new();

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !matches!(
            name.as_ref(),
            ".venv" | "__pycache__" | "node_modules" | "target" | "dist" | "build" | ".git"
        ) && !name.starts_with('.')
            || e.depth() == 0
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if !matches!(ext, "json" | "yaml" | "yml" | "proto") {
            continue;
        }

        let contents = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let rel_path = path
            .strip_prefix(project_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        let kind_and_id = classify(ext, &contents);
        let Some((kind, declared_id)) = kind_and_id else {
            continue;
        };

        let content_hash = {
            let mut hasher = Sha256::new();
            hasher.update(contents.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        out.push(ContractFile {
            rel_path,
            kind,
            declared_id,
            content_hash,
        });
    }

    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    out
}

/// Decide whether a file is a contract and (if so) extract its declared id.
fn classify(ext: &str, contents: &str) -> Option<(ContractKind, Option<String>)> {
    match ext {
        "proto" => {
            // Find the `package` declaration if present.
            let declared_id = extract_proto_package(contents);
            Some((ContractKind::Proto, declared_id))
        }
        "json" => classify_json(contents),
        "yaml" | "yml" => classify_yaml(contents),
        _ => None,
    }
}

fn classify_json(contents: &str) -> Option<(ContractKind, Option<String>)> {
    let v: serde_json::Value = match serde_json::from_str(contents) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let obj = v.as_object()?;

    // OpenAPI: top-level "openapi" or "swagger" key
    if obj.contains_key("openapi") || obj.contains_key("swagger") {
        let title = obj
            .get("info")
            .and_then(|i| i.as_object())
            .and_then(|i| i.get("title"))
            .and_then(|t| t.as_str())
            .map(String::from);
        return Some((ContractKind::OpenApi, title));
    }

    // JSON Schema: $schema OR $id at top level
    if obj.contains_key("$schema") || obj.contains_key("$id") {
        let id = obj.get("$id").and_then(|v| v.as_str()).map(String::from);
        return Some((ContractKind::JsonSchema, id));
    }

    None
}

fn classify_yaml(contents: &str) -> Option<(ContractKind, Option<String>)> {
    let v: serde_yaml::Value = serde_yaml::from_str(contents).ok()?;
    let map = v.as_mapping()?;

    let has_openapi = map.iter().any(|(k, _)| {
        k.as_str().map(|s| s == "openapi" || s == "swagger").unwrap_or(false)
    });
    if has_openapi {
        let title = map
            .iter()
            .find(|(k, _)| k.as_str() == Some("info"))
            .and_then(|(_, v)| v.as_mapping())
            .and_then(|info| info.iter().find(|(k, _)| k.as_str() == Some("title")))
            .and_then(|(_, v)| v.as_str())
            .map(String::from);
        return Some((ContractKind::OpenApi, title));
    }
    None
}

fn extract_proto_package(contents: &str) -> Option<String> {
    // Lightweight scan for `package x.y.z;` declarations.
    for line in contents.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("package ") {
            let name = rest.trim_end_matches(';').trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn classifies_json_schema() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("schema.json"),
            r#"{"$id": "https://example.org/schemas/obs-v1", "$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object"}"#,
        ).unwrap();
        let contracts = scan(dir.path());
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].kind, ContractKind::JsonSchema);
        assert_eq!(contracts[0].declared_id.as_deref(), Some("https://example.org/schemas/obs-v1"));
    }

    #[test]
    fn classifies_openapi_json() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("api.json"),
            r#"{"openapi": "3.0.0", "info": {"title": "My API", "version": "1.0"}}"#,
        ).unwrap();
        let contracts = scan(dir.path());
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].kind, ContractKind::OpenApi);
        assert_eq!(contracts[0].declared_id.as_deref(), Some("My API"));
    }

    #[test]
    fn classifies_openapi_yaml() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("api.yaml"),
            "openapi: 3.0.0\ninfo:\n  title: Y API\n  version: 1.0\n",
        ).unwrap();
        let contracts = scan(dir.path());
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].kind, ContractKind::OpenApi);
        assert_eq!(contracts[0].declared_id.as_deref(), Some("Y API"));
    }

    #[test]
    fn classifies_proto_with_package() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("svc.proto"),
            "syntax = \"proto3\";\npackage my.service.v1;\nmessage X {}\n",
        ).unwrap();
        let contracts = scan(dir.path());
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].kind, ContractKind::Proto);
        assert_eq!(contracts[0].declared_id.as_deref(), Some("my.service.v1"));
    }

    #[test]
    fn skips_plain_json() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("config.json"),
            r#"{"foo": "bar"}"#, // no $schema, no $id, no openapi
        ).unwrap();
        let contracts = scan(dir.path());
        assert!(contracts.is_empty(), "plain JSON without contract markers must not be classified");
    }

    #[test]
    fn content_hash_is_stable() {
        let dir = TempDir::new().unwrap();
        let contents = r#"{"$schema": "x", "type": "object"}"#;
        fs::write(dir.path().join("a.json"), contents).unwrap();
        let h1 = scan(dir.path())[0].content_hash.clone();

        // Same content in another fixture.
        let dir2 = TempDir::new().unwrap();
        fs::write(dir2.path().join("a.json"), contents).unwrap();
        let h2 = scan(dir2.path())[0].content_hash.clone();

        assert_eq!(h1, h2, "identical content must produce identical hash");
    }

    #[test]
    fn finds_contracts_under_subdirs() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("schemas/v1")).unwrap();
        fs::write(
            dir.path().join("schemas/v1/obs.json"),
            r#"{"$id": "obs-v1", "type": "object"}"#,
        ).unwrap();
        let contracts = scan(dir.path());
        assert_eq!(contracts.len(), 1);
        assert_eq!(contracts[0].rel_path, "schemas/v1/obs.json");
    }
}
```

- [ ] **Step 2: Wire `contracts::scan` into the parsers**

In each of `python.rs`, `rust.rs`, and `js.rs`, replace the `contracts: vec![]` initialization (set in Tasks 3+6+7) with `contracts: super::contracts::scan(project_root)`.

Add `pub mod contracts;` to `parsers/mod.rs`:
```rust
pub mod contracts;
pub mod js;
pub mod python;
pub mod rust;
```

- [ ] **Step 3: Update tests in each parser**

Where M3+ tests of `python.rs`/`rust.rs` set up a project with NO contract files, no test changes are needed — `contracts: vec![]` remains.

Where Task 6's MCP tests set up a project, no contract files are present so `contracts` is empty — no test changes.

For a positive-signal test that the wiring works in Python's parser, append to `python.rs`'s tests:
```rust
    #[test]
    fn parser_picks_up_contracts_in_project() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "x"
"#,
        ).unwrap();
        fs::write(
            dir.path().join("schema.json"),
            r#"{"$id": "x-schema", "$schema": "https://json-schema.org/draft/2020-12/schema"}"#,
        ).unwrap();
        let out = parse(dir.path()).unwrap();
        assert_eq!(out.contracts.len(), 1);
        assert_eq!(out.contracts[0].declared_id.as_deref(), Some("x-schema"));
    }
```

- [ ] **Step 4: Run tests**

```sh
cargo test --package prograph-core
```
Expected: 100 tests (92 + 7 in contracts.rs + 1 wiring test in python.rs).

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/parsers/contracts.rs prograph/prograph-core/src/parsers/mod.rs \
        prograph/prograph-core/src/parsers/python.rs prograph/prograph-core/src/parsers/rust.rs \
        prograph/prograph-core/src/parsers/js.rs
git commit -m "prograph: M4 contracts file scanner — JSON Schema / OpenAPI / .proto sniffing"
```

---

## Task 9: `contracts_detector` — group ContractFile → contract_link edges

**Files:**
- Create: `prograph-core/src/detectors/contracts.rs`
- Modify: `prograph-core/src/detectors/mod.rs`

The detector consumes `Vec<ProjectFacts>`, groups every `ContractFile` by `(declared_id, content_hash)`, creates a synthetic `ContractCandidate` per group, and emits `contract_link` `EdgeCandidate`s from each project that owns a file in the group to the contract.

Identity rules per spec §5.2:
- **Contract identity**: `(declared_id, content_hash)`. Same id + different content = different contract (versioning). Same content + different id = different contract (intentional fork).
- **`contract_link` edge identity**: `(from_project, to_contract)`. No attrs beyond the contract reference; multiple files of the same contract in one project still produce a single edge (use a set to dedupe in the detector).

Since contracts are first-class nodes (not just an attribute of edges), the indexer needs to materialize them BEFORE edges resolve. The detector can't assign DB ids — it emits `ContractCandidate` records that the indexer's persist phase turns into rows and then resolves edges against.

- [ ] **Step 1: Extend `detectors/mod.rs` to support contract candidates**

In `prograph-core/src/detectors/mod.rs`, add a new aggregate type alongside `EdgeCandidate`:

```rust
/// A contract candidate produced by `contracts::detect`. The indexer assigns the DB id
/// when materializing into the `contracts` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractCandidate {
    pub kind: crate::facts::ContractKind,
    pub declared_id: Option<String>,
    pub content_hash: String,
    /// Per-project file occurrences: (project_idx, rel_path).
    pub files: Vec<(usize, String)>,
}
```

Replace the existing `detect_all` to return both edge candidates AND contract candidates:

```rust
/// Detector outputs aggregated for the indexer.
#[derive(Debug, Default)]
pub struct DetectionResult {
    pub edges: Vec<EdgeCandidate>,
    pub contracts: Vec<ContractCandidate>,
}

pub fn detect_all(facts: &[ProjectFacts]) -> DetectionResult {
    let mut result = DetectionResult::default();

    // deps -> package_dep edges
    result.edges.extend(deps::detect(facts));

    // contracts -> contract candidates + contract_link edges (filled after Task 11 wires it)
    let (cc, ce) = contracts::detect(facts);
    result.contracts.extend(cc);
    result.edges.extend(ce);

    // mcp -> mcp_call edges (filled by Task 10)
    result.edges.extend(mcp::detect(facts));

    result
}
```

This changes `detect_all`'s return type from `Vec<EdgeCandidate>` to `DetectionResult`. The indexer (Task 12) needs to be updated to use the new type. Mark the change as a single coordinated step: writing detectors comes first, then indexer adapts.

(`mcp::detect` is added in Task 10. Use a placeholder return `Vec::new()` if the build fails before Task 10. Easier: bring Tasks 9 and 10 together — same commit ordering issue. Or merge them into one cargo commit at the end.)

- [ ] **Step 2: Write `detectors/contracts.rs`**

`prograph-core/src/detectors/contracts.rs`:
```rust
//! Contracts detector — groups ContractFile facts by (declared_id, content_hash)
//! into ContractCandidates, then emits contract_link edges from each owning project
//! to the contract.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use super::{ContractCandidate, EdgeCandidate};
use crate::facts::{ContractFile, ContractKind, ProjectFacts};
use crate::models::{EdgeKind, NodeKind};

/// Group contract files into candidates and emit contract_link edges.
/// Single-owner contracts ARE returned as candidates (so the contract becomes a graph node)
/// but emit NO contract_link edge — only ≥2 owners produces cross-project links.
pub fn detect(facts: &[ProjectFacts]) -> (Vec<ContractCandidate>, Vec<EdgeCandidate>) {
    // Group by (declared_id, content_hash). Use a deterministic key string for stable iteration.
    let mut groups: HashMap<String, ContractCandidate> = HashMap::new();
    for (proj_idx, proj) in facts.iter().enumerate() {
        for cf in &proj.contracts {
            let key = format!(
                "{}|{}",
                cf.declared_id.as_deref().unwrap_or(""),
                cf.content_hash
            );
            let candidate = groups.entry(key).or_insert_with(|| ContractCandidate {
                kind: cf.kind,
                declared_id: cf.declared_id.clone(),
                content_hash: cf.content_hash.clone(),
                files: Vec::new(),
            });
            candidate.files.push((proj_idx, cf.rel_path.clone()));
        }
    }

    let mut contracts: Vec<ContractCandidate> = groups.into_values().collect();
    contracts.sort_by(|a, b| {
        (a.declared_id.as_deref().unwrap_or(""), a.content_hash.as_str())
            .cmp(&(b.declared_id.as_deref().unwrap_or(""), b.content_hash.as_str()))
    });

    let mut edges = Vec::new();
    for (contract_idx, c) in contracts.iter().enumerate() {
        // Distinct project indices that own this contract.
        let mut owners: Vec<usize> = c.files.iter().map(|(p, _)| *p).collect();
        owners.sort();
        owners.dedup();

        if owners.len() < 2 {
            continue; // single-owner contracts produce no link edges
        }

        for &proj_idx in &owners {
            let attrs = serde_json::json!({
                "contract_kind": c.kind.as_str(),
                "declared_id": c.declared_id,
            });
            let attrs_json = serde_json::to_string(&attrs).unwrap();
            let mut hasher = Sha256::new();
            hasher.update(b"contract_link|");
            hasher.update(c.declared_id.as_deref().unwrap_or("").as_bytes());
            hasher.update(b"|");
            hasher.update(c.content_hash.as_bytes());
            let attrs_hash = format!("{:x}", hasher.finalize());

            edges.push(EdgeCandidate {
                kind: EdgeKind::ContractLink,
                from_kind: NodeKind::Project,
                from_idx: proj_idx,
                to_kind: NodeKind::Contract,
                to_idx: contract_idx, // INDEX into ContractCandidates, NOT a project idx
                attrs_json,
                attrs_hash,
            });
        }
    }
    edges.sort_by(|a, b| (a.from_idx, a.to_idx).cmp(&(b.from_idx, b.to_idx)));

    (contracts, edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{ContractFile, ContractKind, ParseStatus, ProjectFacts};

    fn fact_with_contracts(name: &str, contracts: Vec<ContractFile>) -> ProjectFacts {
        ProjectFacts {
            project_root: format!("./{name}"),
            project_name: name.to_string(),
            manifest: None,
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts,
        }
    }

    fn cf(rel_path: &str, declared_id: Option<&str>, content: &str) -> ContractFile {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let content_hash = format!("{:x}", hasher.finalize());
        ContractFile {
            rel_path: rel_path.into(),
            kind: ContractKind::JsonSchema,
            declared_id: declared_id.map(String::from),
            content_hash,
        }
    }

    #[test]
    fn single_owner_contract_produces_node_no_edge() {
        let facts = vec![fact_with_contracts(
            "a",
            vec![cf("schemas/obs.json", Some("obs-v1"), "{...}")],
        )];
        let (contracts, edges) = detect(&facts);
        assert_eq!(contracts.len(), 1);
        assert!(edges.is_empty(), "single-owner contract must not produce a link edge");
    }

    #[test]
    fn two_owners_produces_two_link_edges() {
        let facts = vec![
            fact_with_contracts("a", vec![cf("schemas/obs.json", Some("obs-v1"), "{...}")]),
            fact_with_contracts("b", vec![cf("vendor/obs.json", Some("obs-v1"), "{...}")]),
        ];
        let (contracts, edges) = detect(&facts);
        assert_eq!(contracts.len(), 1);
        assert_eq!(edges.len(), 2, "expected one edge per owner");
        let owners: std::collections::HashSet<_> = edges.iter().map(|e| e.from_idx).collect();
        assert_eq!(owners, [0, 1].iter().copied().collect());
    }

    #[test]
    fn same_id_different_content_are_different_contracts() {
        let facts = vec![
            fact_with_contracts("a", vec![cf("schemas/obs.json", Some("obs-v1"), "v1")]),
            fact_with_contracts("b", vec![cf("schemas/obs.json", Some("obs-v1"), "v2")]),
        ];
        let (contracts, edges) = detect(&facts);
        assert_eq!(contracts.len(), 2, "different content -> different contracts");
        assert!(edges.is_empty(), "no link because neither contract has ≥2 owners");
    }

    #[test]
    fn multiple_files_same_project_dedup_owner() {
        let facts = vec![
            fact_with_contracts(
                "a",
                vec![
                    cf("schemas/obs-1.json", Some("obs-v1"), "{...}"),
                    cf("vendor/obs-2.json", Some("obs-v1"), "{...}"),
                ],
            ),
            fact_with_contracts("b", vec![cf("schemas/obs.json", Some("obs-v1"), "{...}")]),
        ];
        let (contracts, edges) = detect(&facts);
        assert_eq!(contracts.len(), 1);
        // Project a has TWO files but should only contribute ONE link edge.
        assert_eq!(edges.len(), 2);
    }
}
```

- [ ] **Step 3: Register the module**

In `prograph-core/src/detectors/mod.rs`, add `pub mod contracts;` alphabetically:
```rust
pub mod contracts;
pub mod deps;
pub mod mcp; // Task 10
```

(Refer to Task 10 — write a placeholder stub for `mcp.rs` now if the order forces it: `pub fn detect(_facts: &[crate::facts::ProjectFacts]) -> Vec<super::EdgeCandidate> { Vec::new() }`. Task 10 fills it in.)

- [ ] **Step 4: Run tests**

```sh
cargo test --package prograph-core detectors
```
Expected: 13 detectors tests (9 from M3 + 4 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 104 tests.

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/detectors/contracts.rs prograph/prograph-core/src/detectors/mod.rs \
        prograph/prograph-core/src/detectors/mcp.rs
git commit -m "prograph: M4 contracts_detector — group ContractFile → contract_link edges"
```

---

## Task 10: `mcp_detector` — match McpToolDecl ↔ McpClientUse

**Files:**
- Modify: `prograph-core/src/detectors/mcp.rs` (replace the Task 9 stub with the real impl)

The detector builds a `tool_name → server_project_idx` index from all `McpToolDecl`s, then iterates `McpClientUse`s and emits an `mcp_call` `EdgeCandidate` for each match. Self-calls (client and server are the same project) are skipped.

Per spec §5.2 identity rules: `mcp_call` identity is `(kind, from, to, tool)`. The `attrs_hash` is computed over `tool` only. Multiple invocation sites in one client produce a single edge (deduped); evidence rows track the call sites (M5+).

- [ ] **Step 1: Write `mcp.rs`**

Replace `prograph-core/src/detectors/mcp.rs`:
```rust
//! MCP detector — matches McpClientUse.tool_name against McpToolDecl.tool_name across
//! projects and emits mcp_call EdgeCandidates.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use super::EdgeCandidate;
use crate::facts::ProjectFacts;
use crate::models::{EdgeKind, NodeKind};

pub fn detect(facts: &[ProjectFacts]) -> Vec<EdgeCandidate> {
    // tool_name -> project_idx that declares it (first writer wins; collisions noted)
    let mut servers: HashMap<&str, usize> = HashMap::new();
    for (idx, p) in facts.iter().enumerate() {
        for decl in &p.mcp_decls {
            // First-writer-wins. A future "warn on collision" path can mirror deps_detector.
            servers.entry(decl.tool_name.as_str()).or_insert(idx);
        }
    }

    // (consumer_idx, server_idx, tool_name) -> single EdgeCandidate (dedupe multiple
    // call sites in the same client).
    let mut seen: HashMap<(usize, usize, String), EdgeCandidate> = HashMap::new();

    for (consumer_idx, consumer) in facts.iter().enumerate() {
        for use_site in &consumer.mcp_uses {
            let Some(&server_idx) = servers.get(use_site.tool_name.as_str()) else {
                continue; // unknown tool, external dep
            };
            if server_idx == consumer_idx {
                continue; // self-call
            }

            let attrs = serde_json::json!({
                "tool": use_site.tool_name,
            });
            let attrs_json = serde_json::to_string(&attrs).unwrap();
            let mut hasher = Sha256::new();
            hasher.update(b"mcp_call|");
            hasher.update(use_site.tool_name.as_bytes());
            let attrs_hash = format!("{:x}", hasher.finalize());

            seen.entry((consumer_idx, server_idx, use_site.tool_name.clone()))
                .or_insert(EdgeCandidate {
                    kind: EdgeKind::McpCall,
                    from_kind: NodeKind::Project,
                    from_idx: consumer_idx,
                    to_kind: NodeKind::Project,
                    to_idx: server_idx,
                    attrs_json,
                    attrs_hash,
                });
        }
    }

    let mut out: Vec<EdgeCandidate> = seen.into_values().collect();
    out.sort_by(|a, b| {
        (a.from_idx, a.to_idx, &a.attrs_hash).cmp(&(b.from_idx, b.to_idx, &b.attrs_hash))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{McpClientUse, McpToolDecl, ParseStatus, ProjectFacts};

    fn fact(name: &str, decls: &[&str], uses: &[&str]) -> ProjectFacts {
        ProjectFacts {
            project_root: format!("./{name}"),
            project_name: name.to_string(),
            manifest: None,
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: decls
                .iter()
                .map(|n| McpToolDecl {
                    tool_name: n.to_string(),
                    rel_path: "src/lib.rs".into(),
                    line: 1,
                })
                .collect(),
            mcp_uses: uses
                .iter()
                .map(|n| McpClientUse {
                    tool_name: n.to_string(),
                    rel_path: "src/lib.rs".into(),
                    line: 1,
                })
                .collect(),
            contracts: vec![],
        }
    }

    #[test]
    fn matches_client_to_server_by_tool_name() {
        let facts = vec![
            fact("client", &[], &["decide"]),
            fact("server", &["decide"], &[]),
        ];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_idx, 0);
        assert_eq!(edges[0].to_idx, 1);
        assert_eq!(edges[0].kind, EdgeKind::McpCall);
    }

    #[test]
    fn skips_unknown_tools() {
        let facts = vec![
            fact("client", &[], &["unknown_tool"]),
            fact("server", &["decide"], &[]),
        ];
        let edges = detect(&facts);
        assert!(edges.is_empty());
    }

    #[test]
    fn skips_self_calls() {
        let facts = vec![fact("self", &["decide"], &["decide"])];
        let edges = detect(&facts);
        assert!(edges.is_empty());
    }

    #[test]
    fn dedupes_multiple_call_sites_in_same_client() {
        let facts = vec![
            fact("client", &[], &["decide", "decide", "decide"]),
            fact("server", &["decide"], &[]),
        ];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1, "three call sites must produce one edge");
    }

    #[test]
    fn identity_hash_includes_tool_name() {
        let e1 = &detect(&[
            fact("c", &[], &["alpha"]),
            fact("s", &["alpha"], &[]),
        ])[0];
        let e2 = &detect(&[
            fact("c", &[], &["beta"]),
            fact("s", &["beta"], &[]),
        ])[0];
        assert_ne!(e1.attrs_hash, e2.attrs_hash);
    }
}
```

- [ ] **Step 2: Run tests**

```sh
cargo test --package prograph-core detectors
```
Expected: 18 detectors tests (13 + 5 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 109 tests.

- [ ] **Step 3: Commit**

```sh
git add prograph/prograph-core/src/detectors/mcp.rs
git commit -m "prograph: M4 mcp_detector — match McpToolDecl ↔ McpClientUse → mcp_call edges"
```

---

## Task 11: Store extensions — alive_contracts, ContractRow writers

**Files:**
- Modify: `prograph-core/src/store.rs`

The indexer needs methods to load the alive contract set (`alive_contracts`) and write/touch contract rows during the persist transaction.

- [ ] **Step 1: Add methods to `Store`**

In `prograph-core/src/store.rs`, append to `impl Store`:
```rust
    /// Return the alive set of contracts keyed by identity:
    /// "{declared_id_or_empty}|{content_hash}" → (contract_id, kind_str).
    pub fn alive_contracts(&self) -> Result<std::collections::HashMap<String, (i64, String)>> {
        let mut out = std::collections::HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT id, declared_id, content_hash, kind FROM contracts
             WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (id, declared_id, content_hash, kind) = row?;
            let key = format!("{}|{}", declared_id.as_deref().unwrap_or(""), content_hash);
            out.insert(key, (id, kind));
        }
        Ok(out)
    }
```

- [ ] **Step 2: Add methods to `SnapshotWriter`**

Append to `impl<'a> SnapshotWriter<'a>` in `store.rs`:
```rust
    pub fn insert_contract(
        &self,
        snapshot_id: i64,
        declared_id: Option<&str>,
        content_hash: &str,
        kind: &str,
    ) -> Result<i64> {
        self.tx.execute(
            "INSERT INTO contracts (declared_id, content_hash, kind, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![declared_id, content_hash, kind, snapshot_id, snapshot_id],
        )?;
        Ok(self.tx.last_insert_rowid())
    }

    pub fn touch_contract(&self, contract_id: i64, snapshot_id: i64) -> Result<()> {
        self.tx.execute(
            "UPDATE contracts SET last_seen = ? WHERE id = ?",
            rusqlite::params![snapshot_id, contract_id],
        )?;
        Ok(())
    }

    pub fn insert_contract_file(
        &self,
        contract_id: i64,
        project_id: i64,
        rel_path: &str,
        snapshot_id: i64,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR IGNORE INTO contract_files
             (contract_id, project_id, rel_path, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![contract_id, project_id, rel_path, snapshot_id, snapshot_id],
        )?;
        Ok(())
    }

    pub fn touch_contract_file(
        &self,
        contract_id: i64,
        project_id: i64,
        rel_path: &str,
        snapshot_id: i64,
    ) -> Result<()> {
        self.tx.execute(
            "UPDATE contract_files SET last_seen = ?
             WHERE contract_id = ? AND project_id = ? AND rel_path = ?",
            rusqlite::params![snapshot_id, contract_id, project_id, rel_path],
        )?;
        Ok(())
    }
```

- [ ] **Step 3: Update `latest_snapshot_info` to count contracts**

Find the query in `latest_snapshot_info`. Add a count for contracts:
```rust
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, monorepo_root, git_commit, prograph_version,
                    (SELECT COUNT(*) FROM projects WHERE last_seen = s.id) AS n_projects,
                    (SELECT COUNT(*) FROM edges    WHERE last_seen = s.id) AS n_edges,
                    (SELECT COUNT(*) FROM change_log WHERE snapshot_id = s.id) AS n_changes
             FROM snapshots s
             ORDER BY id DESC LIMIT 1",
        )?;
```

This stays the same — `SnapshotInfo.n_edges` includes contract_link edges (they're rows in the `edges` table), so the count is already correct. Contracts themselves count separately, which `SnapshotInfo` doesn't surface yet. If a separate `n_contracts` field is desired, that's an M5 polish.

- [ ] **Step 4: Add tests**

Append to `store.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn alive_contracts_empty_before_any_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("g.db")).unwrap();
        assert!(store.alive_contracts().unwrap().is_empty());
    }

    #[test]
    fn write_contract_then_alive_reflects_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        let writer = store.begin_snapshot().unwrap();
        let snap = writer.insert_snapshot("ts", "/m", None, "0.1.0").unwrap();
        let pid = writer.insert_project(snap, "a", "./a", "python", "{}").unwrap();
        let cid = writer.insert_contract(snap, Some("obs-v1"), "deadbeef", "json_schema").unwrap();
        writer.insert_contract_file(cid, pid, "schemas/obs.json", snap).unwrap();
        writer.commit().unwrap();

        let alive = store.alive_contracts().unwrap();
        assert_eq!(alive.len(), 1);
        assert!(alive.contains_key("obs-v1|deadbeef"));
    }

    #[test]
    fn touch_contract_extends_last_seen() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        let cid_in_snap1 = {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts1", "/m", None, "0.1.0").unwrap();
            let cid = writer
                .insert_contract(snap, Some("x"), "hash", "json_schema")
                .unwrap();
            writer.commit().unwrap();
            cid
        };

        {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts2", "/m", None, "0.1.0").unwrap();
            writer.touch_contract(cid_in_snap1, snap).unwrap();
            writer.commit().unwrap();
        }

        let last_seen: i64 = store
            .connection()
            .query_row(
                "SELECT last_seen FROM contracts WHERE id = ?",
                rusqlite::params![cid_in_snap1],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(last_seen, 2, "touch_contract must extend last_seen to snapshot 2");
    }
```

- [ ] **Step 5: Run tests**

```sh
cargo test --package prograph-core store
```
Expected: 15 store tests (12 + 3 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 112 tests.

- [ ] **Step 6: Commit**

```sh
git add prograph/prograph-core/src/store.rs
git commit -m "prograph: M4 Store — alive_contracts + SnapshotWriter::{insert,touch}_contract{,_file}"
```

---

## Task 12: Indexer — Contract diff pass + new edge kinds

**Files:**
- Modify: `prograph-core/src/indexer.rs`

The indexer now has THREE diff passes (projects, contracts, edges) and must handle the new edge kinds (`mcp_call`, `contract_link`) when persisting. The contracts pass produces a `new_contract_ids: HashMap<String, i64>` that the edge persist loop uses to resolve `contract_link` targets.

This is the most intricate task of M4. Read carefully.

- [ ] **Step 1: Refactor `index_monorepo` to handle the new structure**

In `prograph-core/src/indexer.rs`, locate the section that builds `detect_all`. Replace:
```rust
    let edge_candidates = detectors::detect_all(&facts);
```
with:
```rust
    let detection = detectors::detect_all(&facts);
    let edge_candidates = &detection.edges;
    let contract_candidates = &detection.contracts;
```

(Drop the old name; thread the new variables through.)

- [ ] **Step 2: Add the Contracts diff pass**

After the existing Projects diff pass and BEFORE the Edges diff pass, insert:

```rust
    // Phase 4b: Contracts diff.
    let alive_contracts = store.alive_contracts()?;
    let new_contract_attrs: HashMap<String, String> = contract_candidates
        .iter()
        .map(|c| {
            let key = format!(
                "{}|{}",
                c.declared_id.as_deref().unwrap_or(""),
                c.content_hash
            );
            let attrs = serde_json::json!({
                "kind": c.kind.as_str(),
                "declared_id": c.declared_id,
                "files": c.files.iter().map(|(idx, p)| {
                    serde_json::json!({"project_root": &facts[*idx].project_root, "rel_path": p})
                }).collect::<Vec<_>>(),
            });
            (key, serde_json::to_string(&attrs).unwrap())
        })
        .collect();

    let contract_diff = diff_by_identity(
        &alive_contracts
            .iter()
            .map(|(k, (_, _))| (k.clone(), String::new()))
            .collect(),
        // For contracts, we don't compute attrs_changed — file lists changing is fine
        // and the indexer handles file diff at the contract_files level (Task: future).
        // Treat attrs as constant per identity; only Added/Removed events fire.
        &new_contract_attrs
            .iter()
            .map(|(k, _)| (k.clone(), String::new()))
            .collect(),
    );
```

- [ ] **Step 3: Add the Contracts persist branches**

After the project persist loop and BEFORE the edge persist loop, insert:

```rust
    let mut new_contract_ids: HashMap<String, i64> = HashMap::new();
    for entry in &contract_diff {
        let key = &entry.identity_key;
        match entry.change {
            DiffChange::Added => {
                let c = contract_candidates
                    .iter()
                    .find(|c| {
                        let k = format!(
                            "{}|{}",
                            c.declared_id.as_deref().unwrap_or(""),
                            c.content_hash
                        );
                        &k == key
                    })
                    .unwrap();
                let after_attrs = new_contract_attrs[key].as_str();
                let cid = writer.insert_contract(
                    snap_id,
                    c.declared_id.as_deref(),
                    &c.content_hash,
                    c.kind.as_str(),
                )?;
                new_contract_ids.insert(key.clone(), cid);
                writer.insert_change_log(
                    snap_id, &ts, "contract", cid, "added", None, Some(after_attrs),
                )?;
                n_changes += 1;

                // Materialize contract_files for each owning project.
                for (proj_idx, rel_path) in &c.files {
                    let proj_root = &facts[*proj_idx].project_root;
                    if let Some(&pid) = new_project_ids.get(proj_root) {
                        writer.insert_contract_file(cid, pid, rel_path, snap_id)?;
                    }
                }
            }
            DiffChange::Unchanged => {
                let (cid, _kind) = &alive_contracts[key];
                writer.touch_contract(*cid, snap_id)?;
                new_contract_ids.insert(key.clone(), *cid);

                // Touch existing contract_files; insert new ones if a project just started owning.
                let c = contract_candidates
                    .iter()
                    .find(|c| {
                        let k = format!(
                            "{}|{}",
                            c.declared_id.as_deref().unwrap_or(""),
                            c.content_hash
                        );
                        &k == key
                    })
                    .unwrap();
                for (proj_idx, rel_path) in &c.files {
                    let proj_root = &facts[*proj_idx].project_root;
                    if let Some(&pid) = new_project_ids.get(proj_root) {
                        // Idempotent: INSERT OR IGNORE keeps existing rows, otherwise creates.
                        writer.insert_contract_file(cid.clone(), pid, rel_path, snap_id)?;
                        writer.touch_contract_file(*cid, pid, rel_path, snap_id)?;
                    }
                }
            }
            DiffChange::AttrsChanged => {
                // Not produced for contracts in M4 (attrs are intentionally constant per identity).
                unreachable!("contracts: attrs_changed not expected in M4");
            }
            DiffChange::Removed => {
                let (cid, _) = &alive_contracts[key];
                let before_attrs: String = format!(r#"{{"removed_in_snapshot":{snap_id}}}"#);
                writer.insert_change_log(
                    snap_id, &ts, "contract", *cid, "removed",
                    Some(&before_attrs), None,
                )?;
                n_changes += 1;
            }
        }
    }
```

(There's a small typo to fix: `cid.clone()` on an `i64` — just use `*cid`. Replace as written.)

- [ ] **Step 4: Extend the edge persist loop to handle Contract endpoints**

In the existing edge persist loop, the `Added` branch currently resolves both endpoints from `new_project_ids`. Now `contract_link` edges have a Contract endpoint. Update the resolution:

Find the `DiffChange::Added` branch for edges. Replace its endpoint resolution with:
```rust
            DiffChange::Added => {
                // ... existing identity-key parsing for from_root, to_root, attrs_hash
                // Determine endpoint kinds. For package_dep + mcp_call: both project.
                // For contract_link: from = project, to = contract.
                let (from_kind_str, from_id_opt) = ("project", new_project_ids.get(from_root).copied());
                let (to_kind_str, to_id_opt) = if ec.kind == EdgeKind::ContractLink {
                    ("contract", new_contract_ids.get(to_root).copied())
                } else {
                    ("project", new_project_ids.get(to_root).copied())
                };
                let from_id = match from_id_opt {
                    Some(id) => id,
                    None => continue,
                };
                let to_id = match to_id_opt {
                    Some(id) => id,
                    None => continue,
                };

                let attrs = entry.after_json.as_deref().unwrap_or("{}");
                let kind_str = ec.kind.name();
                let eid = writer.insert_edge(
                    snap_id, kind_str, from_kind_str, from_id, to_kind_str, to_id, attrs, attrs_hash,
                )?;
                writer.insert_change_log(snap_id, &ts, "edge", eid, "added", None, Some(attrs))?;
                n_edges += 1;
                n_changes += 1;
            }
```

This requires the indexer to KNOW which EdgeKind each candidate uses. The `EdgeCandidate.kind` field is already populated. But the identity-key parsing only carries from_root/to_root/attrs_hash — not the kind. We need to either:
- (a) Include the kind in the identity key (change the key shape from `package_dep|<from>|<to>|<hash>` to `<kind>|<from>|<to>|<hash>`)
- (b) Look up the kind from the `edge_candidates` list by attrs_hash

Option (a) is cleaner. Update `new_edge_attrs` construction in indexer.rs:
```rust
    let new_edge_attrs: HashMap<String, String> = edge_candidates
        .iter()
        .map(|c| {
            let from_root = &facts[c.from_idx].project_root;
            // For contract_link, to_idx is into ContractCandidates, NOT facts.
            let to_root = if c.kind == EdgeKind::ContractLink {
                let cc = &contract_candidates[c.to_idx];
                format!(
                    "{}|{}",
                    cc.declared_id.as_deref().unwrap_or(""),
                    cc.content_hash
                )
            } else {
                facts[c.to_idx].project_root.clone()
            };
            let key = format!("{}|{}|{}|{}", c.kind.name(), from_root, to_root, c.attrs_hash);
            (key, c.attrs_json.clone())
        })
        .collect();
```

And update `alive_edges_by_root` construction in indexer.rs to use the new key shape:
```rust
    let alive_edges_by_root: HashMap<String, (i64, String)> = alive_edges
        .iter()
        .filter_map(|(raw_key, (id, attrs))| {
            // raw_key format from Store::alive_edges: "kind|from_kind|from_id|to_kind|to_id|attrs_hash"
            let parts: Vec<&str> = raw_key.split('|').collect();
            if parts.len() != 6 {
                return None;
            }
            let kind = parts[0];
            let from_kind = parts[1];
            let from_id: i64 = parts[2].parse().ok()?;
            let to_kind = parts[3];
            let to_id: i64 = parts[4].parse().ok()?;
            let attrs_hash = parts[5];

            let from_key = match from_kind {
                "project" => project_id_to_root.get(&from_id).map(|s| s.to_string())?,
                _ => return None, // M4: from is always project
            };
            let to_key = match to_kind {
                "project" => project_id_to_root.get(&to_id).map(|s| s.to_string())?,
                "contract" => {
                    // Reverse-lookup the contract identity by DB id.
                    alive_contracts
                        .iter()
                        .find(|(_, (cid, _))| *cid == to_id)
                        .map(|(k, _)| k.clone())?
                }
                _ => return None,
            };
            let key = format!("{}|{}|{}|{}", kind, from_key, to_key, attrs_hash);
            Some((key, (*id, attrs.clone())))
        })
        .collect();
```

(`project_id_to_root` is already built earlier in indexer.rs. The contract reverse-lookup is O(n_contracts) per edge — acceptable at M4 scale.)

- [ ] **Step 5: Update existing indexer tests**

Several existing tests assume the edge identity key format is `package_dep|<from>|<to>|<hash>`. They probably still pass because the kind is just prepended — but verify. If any break, adapt the identity-key parsing in test helpers.

The change from `package_dep|...` to `<kind>|...` in the diff key is breaking for any pre-M4 snapshot stored under the old key. But the alive_edges Store query produces keys at runtime, so there's no on-disk format change — only an in-memory key shape change. Existing v3 DBs work unchanged.

- [ ] **Step 6: Run cargo tests**

```sh
cargo test --package prograph-core
```
Expected: ≥112 tests still pass. Failures here indicate the refactor needs adjustment.

If the test `project_removal_cascades_edge_removed_event` (from M2 Task 12 fix) breaks because the identity key now starts with `package_dep|` not the old `package_dep|<from>|...`, adapt the test's assertions accordingly. The semantics haven't changed.

Verify clean:
```sh
cargo fmt --all -- --check
cargo clippy --package prograph-core --all-targets -- -D warnings
```

- [ ] **Step 7: Commit**

```sh
git add prograph/prograph-core/src/indexer.rs
git commit -m "prograph: M4 indexer — Contract diff pass + EdgeKind-aware identity keys"
```

---

## Task 13: `monorepo_mcp` fixture

**Files:**
- Create: ~8 files under `tests/fixtures/monorepo_mcp/`

A fixture exercising MCP + contracts cross-language:
- `arbiter_like/` (Rust) — registers tool `decide` via `.register_tool("decide", ...)` in `src/lib.rs`
- `maestro_like/` (Python) — invokes tool `decide` via `session.call_tool("decide", ...)` in `src/maestro_like/__init__.py`; also publishes its own MCP tool `report_outcome` via `@server.tool()`
- `spec_runner_like/` (Python) — invokes `report_outcome` via `await client.call_tool("report_outcome", ...)`
- `shared_contract/` (docs-only kind, with JSON Schema file) — single owner
- Two more projects, both having a `schemas/obs-v1.json` file with identical content + `$id` so the contract detector creates a `contract_link`

Expected edges: 3 `mcp_call` (Maestro→arbiter for decide, spec_runner→Maestro for report_outcome — Wait actually the arbiter_like project has no Python so the python ast scan misses it but the Rust scan catches it. And Maestro→arbiter's decide is the Python client → Rust server pattern that requires BOTH scanners.) + ≥1 `contract_link` (two projects share the obs schema).

Let me make the fixture clearer. Simpler shape:

- `py_server/` (Python, declares `decide` via `@server.tool()`)
- `py_client/` (Python, invokes `decide`)
- `rust_server/` (Rust, declares `evaluate` via `.tool("evaluate", ...)`)
- `py_dual_client/` (Python, invokes both `decide` AND `evaluate`)
- `shared_a/` (any kind, has `schemas/obs-v1.json` with `$id` and `$schema`)
- `shared_b/` (any kind, has `schemas/obs-v1.json` (same content))

Expected: 3 `mcp_call` (py_client→py_server, py_dual_client→py_server, py_dual_client→rust_server) + 2 `contract_link` (shared_a→contract, shared_b→contract).

- [ ] **Step 1: Create the 6 projects with the exact file contents below**

`tests/fixtures/monorepo_mcp/py_server/pyproject.toml`:
```toml
[project]
name = "py-server"
version = "0.1.0"
requires-python = ">=3.11"
```

`tests/fixtures/monorepo_mcp/py_server/server.py`:
```python
from mcp.server.fastmcp import FastMCP

server = FastMCP("py-server")

@server.tool()
def decide(query: str) -> dict:
    return {"answer": "yes"}
```

`tests/fixtures/monorepo_mcp/py_client/pyproject.toml`:
```toml
[project]
name = "py-client"
version = "0.1.0"
requires-python = ">=3.11"
```

`tests/fixtures/monorepo_mcp/py_client/client.py`:
```python
async def run(session):
    result = await session.call_tool("decide", arguments={"query": "x"})
    return result
```

`tests/fixtures/monorepo_mcp/rust_server/Cargo.toml`:
```toml
[package]
name = "rust-server"
version = "0.1.0"
edition = "2021"
```

`tests/fixtures/monorepo_mcp/rust_server/src/lib.rs`:
```rust
pub fn setup(builder: &mut ServerBuilder) {
    builder.tool("evaluate", |args| Ok(()));
}
```

`tests/fixtures/monorepo_mcp/py_dual_client/pyproject.toml`:
```toml
[project]
name = "py-dual"
version = "0.1.0"
requires-python = ">=3.11"
```

`tests/fixtures/monorepo_mcp/py_dual_client/client.py`:
```python
async def run(session):
    a = await session.call_tool("decide", arguments={})
    b = await session.call_tool("evaluate", arguments={})
    return (a, b)
```

`tests/fixtures/monorepo_mcp/shared_a/pyproject.toml`:
```toml
[project]
name = "shared-a"
version = "0.1.0"
requires-python = ">=3.11"
```

`tests/fixtures/monorepo_mcp/shared_a/schemas/obs-v1.json`:
```json
{
  "$id": "obs-v1",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "ts": {"type": "string"},
    "kind": {"type": "string"}
  }
}
```

`tests/fixtures/monorepo_mcp/shared_b/pyproject.toml`:
```toml
[project]
name = "shared-b"
version = "0.1.0"
requires-python = ">=3.11"
```

`tests/fixtures/monorepo_mcp/shared_b/schemas/obs-v1.json` — **exact byte-for-byte copy** of `shared_a/schemas/obs-v1.json` (same content → same content_hash → contracts detector groups them).

- [ ] **Step 2: Sanity-scan the fixture**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv sync --reinstall-package prograph
uv run python -c "
from prograph._core import scan_monorepo
for c in sorted(scan_monorepo('tests/fixtures/monorepo_mcp'), key=lambda x: x.name):
    print(c.name, c.kind.name())
"
```

Expected: 6 projects.

- [ ] **Step 3: Commit**

```sh
git add prograph/tests/fixtures/monorepo_mcp/
git commit -m "prograph: M4 monorepo_mcp fixture — MCP server/client + shared JSON Schema contract"
```

---

## Task 14: Integration test against `monorepo_mcp`

**Files:**
- Create: `tests/integration/test_cli_index_mcp.py`

- [ ] **Step 1: Write the e2e tests**

`tests/integration/test_cli_index_mcp.py`:
```python
"""End-to-end integration test against monorepo_mcp fixture."""

import json
import shutil
import sqlite3
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_mcp"


@pytest.fixture
def fresh_mcp_fixture(tmp_path: Path) -> Path:
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst)
    return dst


def _run(args: list[str]) -> dict:
    result = runner.invoke(app, [*args, "--json"])
    assert result.exit_code == 0, result.stdout + result.stderr
    return json.loads(result.stdout)


def _edges(db: Path) -> list[tuple[str, str, str, str]]:
    """Returns list of (from_name, to_kind, to_name_or_id, edge_kind) for the latest snapshot."""
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT
            (SELECT name FROM projects WHERE id = e.from_id) as from_name,
            e.to_kind,
            CASE
                WHEN e.to_kind = 'project' THEN (SELECT name FROM projects WHERE id = e.to_id)
                WHEN e.to_kind = 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.to_id)
            END as to_name,
            e.kind
        FROM edges e
        WHERE e.last_seen = (SELECT MAX(id) FROM snapshots)
        ORDER BY e.kind, from_name, to_name
        """
    ).fetchall()
    conn.close()
    return rows


def test_mcp_index_detects_mcp_calls_and_contract_links(fresh_mcp_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_mcp_fixture)])
    summary = _run(["index", "--monorepo", str(fresh_mcp_fixture)])

    assert summary["n_projects"] == 6, summary

    rows = _edges(fresh_mcp_fixture / ".prograph" / "graph.db")

    mcp_rows = [r for r in rows if r[3] == "mcp_call"]
    contract_rows = [r for r in rows if r[3] == "contract_link"]

    assert len(mcp_rows) == 3, f"expected 3 mcp_call edges, got {mcp_rows}"
    assert len(contract_rows) == 2, f"expected 2 contract_link edges, got {contract_rows}"


def test_mcp_edge_attrs_carry_tool_name(fresh_mcp_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_mcp_fixture)])
    _run(["index", "--monorepo", str(fresh_mcp_fixture)])

    conn = sqlite3.connect(fresh_mcp_fixture / ".prograph" / "graph.db")
    rows = conn.execute(
        """
        SELECT json_extract(e.attrs_json, '$.tool')
        FROM edges e
        WHERE e.kind = 'mcp_call' AND e.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()
    tools = {r[0] for r in rows}
    assert tools == {"decide", "evaluate"}, tools


def test_mcp_contract_node_created_with_declared_id(fresh_mcp_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_mcp_fixture)])
    _run(["index", "--monorepo", str(fresh_mcp_fixture)])

    conn = sqlite3.connect(fresh_mcp_fixture / ".prograph" / "graph.db")
    rows = conn.execute(
        """
        SELECT declared_id, kind FROM contracts
        WHERE last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()
    assert ("obs-v1", "json_schema") in rows


def test_mcp_idempotent_reindex(fresh_mcp_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_mcp_fixture)])
    _run(["index", "--monorepo", str(fresh_mcp_fixture)])
    summary = _run(["index", "--monorepo", str(fresh_mcp_fixture)])
    assert summary["n_changes"] == 0, "second index on unchanged state should produce zero changes"
```

- [ ] **Step 2: Run**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run pytest tests/integration/test_cli_index_mcp.py -v
```
Expected: 4 passed.

Full suite:
```sh
uv run pytest -v
```
Expected: 45 (41 from Task 4 + 4 new).

- [ ] **Step 3: Commit**

```sh
git add prograph/tests/integration/test_cli_index_mcp.py
git commit -m "prograph: M4 e2e — MCP + contracts on monorepo_mcp (3 mcp_call + 2 contract_link)"
```

---

## Task 15: Real-monorepo smoke

**Files:**
- Modify: `tests/integration/test_smoke_real.py`

After M4, the real `all_ai_orchestrators/` should produce at least one of: a `contract_link` edge (from `_cowork_output/observability-contract` shared between Maestro + spec-runner), or an `mcp_call` edge (if arbiter's Rust code matches our patterns + Maestro's Python code invokes a tool). Tighten the assertion accordingly.

- [ ] **Step 1: Update the smoke test**

In `tests/integration/test_smoke_real.py`, find the existing assertion `assert summary["n_edges"] >= 1` (added in M3) and extend with a kind-breakdown check:

```python
    # M4: now expect at least one MCP or contract edge in addition to package_dep.
    import sqlite3
    paths_db = REAL_MONOREPO / ".prograph" / "graph.db"
    if paths_db.exists():
        conn = sqlite3.connect(paths_db)
        kind_counts = dict(conn.execute(
            "SELECT kind, COUNT(*) FROM edges "
            "WHERE last_seen = (SELECT MAX(id) FROM snapshots) GROUP BY kind"
        ).fetchall())
        conn.close()
        has_mcp_or_contract = (
            kind_counts.get("mcp_call", 0) > 0 or kind_counts.get("contract_link", 0) > 0
        )
        # We don't *require* this — the real monorepo may not have explicit shared schemas
        # or MCP patterns the detector recognises. Log it instead.
        if not has_mcp_or_contract:
            import warnings as _w
            _w.warn(
                f"M4 smoke: real monorepo has only package_dep edges. kind_counts={kind_counts}",
                stacklevel=2,
            )
```

This is a soft assertion (warns, doesn't fail). The intent: M4 detection patterns are heuristic enough that the real monorepo may or may not match. If patterns match, great. If not, log it for human follow-up — don't break CI.

- [ ] **Step 2: Run**

```sh
uv run pytest -m realmonorepo -v
```
Expected: 1 passed. The warning (if any) lands in stdout.

- [ ] **Step 3: Commit**

```sh
git add prograph/tests/integration/test_smoke_real.py
git commit -m "prograph: M4 real-monorepo smoke — log MCP/contract edge counts (soft assertion)"
```

---

## Task 16: README + CLAUDE.md updates + M4 close

**Files:**
- Modify: `prograph/README.md`
- Modify: `prograph/CLAUDE.md`
- Modify: this plan file (check DoD)

- [ ] **Step 1: Update README**

Replace the Status line:
```markdown
**Status:** M4 — Contracts + MCP detectors. `prograph index` now detects **three edge kinds**: `package_dep` (M2/M3), `mcp_call` (Python + Rust tree-sitter source scan), and `contract_link` (shared JSON Schema / OpenAPI / .proto files between ≥2 projects). Tree-sitter parsing introduced; module-level facts beyond MCP land in M5. Browser UI + MCP stdio server: M6/M7.
```

Add an MCP-edge-types note to Usage:
```markdown
### Detected edge kinds (M4)

| Kind | Source | Identity |
|---|---|---|
| `package_dep` | Manifest deps (`[project].dependencies`, `[dependencies]`, `dependencies`...) | `(from, to, dep_name)` — version_req in attrs |
| `mcp_call` | `@server.tool()` decorator / `.tool("name", ...)` registration on the server side; `.call_tool("name", ...)` on the client side. Detection: Python source via tree-sitter; Rust source via tree-sitter. | `(from, to, tool)` |
| `contract_link` | JSON Schema / OpenAPI / .proto files with matching `$id` (or content hash) across ≥2 projects. | `(from_project, to_contract)` |

Edges are aggregated regardless of source: a single `prograph index` produces all three kinds in one snapshot.
```

Update the limitations section:
```markdown
### M4 limitations (intentional — addressed in later milestones)

- **MCP detection is heuristic.** Patterns match the common anthropic/mcp Python SDK and common Rust MCP idioms. Code using different framework names won't be detected; custom patterns will land via M7+ config.
- **No JS MCP detection.** JS tree-sitter integration deferred — no driver in the target monorepo.
- **Contract files must be discoverable on disk.** A contract published only via `pyproject.toml` declaration (without a file) isn't detected.
- **No HTTP / REST runtime edges.** Phase 5+.
- **No MD export / browser UI / MCP stdio server** — M5/M6/M7.
```

- [ ] **Step 2: Update CLAUDE.md**

Replace the "Architecture (M3 state)" section header with "Architecture (M4 state)" and update the components list:

```markdown
## Architecture (M4 state)

Two-layer build:

- **`prograph-core` (Rust crate via PyO3):**
  - `discovery` — project classification + monorepo walk (M1)
  - `parsers/python` — `pyproject.toml` + tree-sitter Python source MCP scan (M2+M3+M4)
  - `parsers/rust` — `Cargo.toml` + tree-sitter Rust source MCP scan (M3+M4)
  - `parsers/js` — `package.json` parsing (M3; MCP source scan deferred)
  - `parsers/contracts` — file-system JSON Schema / OpenAPI / .proto scanner (M4)
  - `ts_queries/` — tree-sitter query files (`python_mcp.scm`, `rust_mcp.scm`)
  - `detectors/deps` — package-dep matching with aliases (M2+M3)
  - `detectors/contracts` — ContractFile grouping → contract_link edges (M4)
  - `detectors/mcp` — McpToolDecl ↔ McpClientUse matching → mcp_call edges (M4)
  - `diff` — added/removed/attrs_changed classifier (M2)
  - `lock` — RAII FS exclusive lock (M2)
  - `indexer` — pipeline orchestrator with three diff passes (projects + contracts + edges) (M4)
  - `store` — SQLite schema v3 + transactional snapshot writer with contract methods
  - `models` — Rust pyclasses (`ProjectKind`, `ProjectCandidate`, `Edge`, `Contract`, `ChangeEvent`, `SnapshotInfo`, `IndexSummary`, `NodeKind`, `EdgeKind`, `ChangeKind`, `EntityKind`)
  - `facts` — `Manifest` (with `aliases`), `DepRequirement`, `ProjectFacts` (with `mcp_decls`, `mcp_uses`, `contracts`), `ContractFile`, `ContractKind`, `McpToolDecl`, `McpClientUse`, `ParseStatus`, `ParseWarning`
  - `errors` — `PrographError` with PyErr mapping
  - `migrations/v1.sql`, `migrations/v2.sql`, `migrations/v3.sql`
- **`prograph` (Python package):** `cli.py` (`init`, `index`, `status`, `--version`), `models.py` (pydantic mirrors), `paths.py`.
```

Replace "What is NOT in M3" with:
```markdown
## What is NOT in M4

- JS MCP source scanning — deferred.
- HTTP / REST runtime edges — Phase 5+.
- Module-level facts beyond MCP (public symbols, internal imports) — M5.
- MD export / browser UI / MCP stdio server — M5/M6/M7.

(See `docs/superpowers/plans/` for individual milestone plans.)
```

Add to "Tooling pins worth knowing":
```markdown
- `tree-sitter`, `tree-sitter-python`, `tree-sitter-rust` compile C source via `cc-rs`. First build takes ~60s; subsequent builds are fast (no source changes).
```

- [ ] **Step 3: Run full local gate**

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

Expected: every command exits 0. Cargo ≥112; pytest ≥45; realmonorepo 1.

- [ ] **Step 4: Check the DoD boxes below**

Mark each `- [ ]` in "Definition of Done (M4)" as `- [x]` with achieved counts.

- [ ] **Step 5: Final commit**

```sh
git add prograph/README.md prograph/CLAUDE.md \
        prograph/docs/superpowers/plans/2026-05-26-prograph-m4-contracts-mcp.md
git commit -m "prograph: M4 close — docs updated, full gate green, DoD checked"
```

---

## Definition of Done (M4)

- [x] `cargo test --all-targets` passes (112 tests).
- [x] `uv run pytest -v` passes (45 tests; 1 deselected).
- [x] `uv run pytest -m realmonorepo -v` passes (real monorepo produced 2 contract_link + 2 package_dep edges; no mcp_call → soft-warn).
- [x] Schema v3 (`contracts`, `contract_files`, widened `edges.kind` + `change_log.entity_kind` CHECK) applies cleanly over an existing v2 database and preserves existing edges.
- [x] `Manifest.aliases`, `ContractFile`, `McpToolDecl`, `McpClientUse` are public types in `facts.rs`; `ProjectFacts` carries the new fact lists with `#[serde(default)]` back-compat.
- [x] `EdgeKind` widens to `{PackageDep, McpCall, ContractLink}` in Rust + pydantic mirror + `.pyi` stub.
- [x] `Contract` pyclass + pydantic mirror exist and round-trip.
- [x] Python parser walks `.py` files via tree-sitter and produces `McpToolDecl`/`McpClientUse` for the canonical `@server.tool()` / `.call_tool("...")` patterns. Skip rules cover `.venv`, `__pycache__`, `target`, `dist`, `build`, `.git`, hidden dirs.
- [x] Rust parser walks `.rs` files via tree-sitter and produces `McpToolDecl`/`McpClientUse` for `.tool("...")` / `Tool::new("...")` / `.call_tool("...")` patterns. Skips `target/`.
- [x] Contract scanner discovers JSON Schema / OpenAPI / .proto files and classifies them with `declared_id` extraction; content hash stable across copies.
- [x] `contracts_detector` produces contract nodes for ≥1 owner; emits `contract_link` edges only for ≥2 owners.
- [x] `mcp_detector` matches client uses to server decls by `tool_name`; emits one `mcp_call` edge per (from, to, tool) regardless of multiple call sites.
- [x] `Store::alive_contracts` + `SnapshotWriter::{insert,touch}_contract{,_file}` work and round-trip.
- [x] Indexer runs three diff passes (projects + contracts + edges); persists contracts BEFORE edges so `contract_link` endpoints resolve; identity-key shape widens to `<kind>|<from>|<to>|<hash>` for all edge kinds.
- [x] `monorepo_mcp` fixture produces 6 projects, 3 `mcp_call` edges, 2 `contract_link` edges, 0 changes on re-index.
- [x] Real-monorepo smoke runs and logs the kind breakdown.
- [x] CI workflow continues to pass with no changes required.
- [x] All commits follow the `prograph: M4 ...` prefix convention.

## What is NOT done in M4 (handled in subsequent milestones)

- **M5** — Module-level facts beyond MCP (public symbols, internal imports) via tree-sitter expansion; MD exporter + golden tests + Obsidian-friendly per-project files.
- **M6** — Browser UI (FastAPI + static + d3/cytoscape) + REST API.
- **M7** — MCP stdio server for AI agents + configurable detection patterns.
- **M8** — JS MCP detection, HTTP/REST runtime edges, real-monorepo CI matrix, performance baselines, workspace auto-discovery.
