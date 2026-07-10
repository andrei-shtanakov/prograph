# Declared Edges Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Projects declare file-based integrations (`[tool.prograph] reads/writes`) in their manifests; prograph renders them as first-class `declared` edges with manifest evidence, and reports declarations whose target path vanished as `stale_declaration` drift.

**Architecture:** `DeclaredPath` is a parser-produced fact with source location. A new `detectors/declared.rs` pipeline owns validation → target resolution (segment-aware longest-match) → `EdgeCandidate` production → filesystem stale checks → warnings, returned as one `DeclaredDetection` struct. Migration v10 rebuilds two CHECK constraints. The persist phase's evidence lookup is fixed to match on full edge identity (latent bug for existing kinds).

**Tech Stack:** Rust (PyO3 0.29), SQLite (rusqlite), Python 3.11+ (typer/FastAPI), maturin, pytest, cargo test.

**Spec:** `docs/superpowers/specs/2026-07-10-prograph-declared-edges-design.md` — the authoritative requirements; re-read the relevant section before each task.

## Global Constraints

- Ruff line length **100**; pyrefly via explicit globs `uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'` — never bare.
- After ANY Rust edit: `uv run maturin develop` before pytest/CLI (Python imports the compiled `.so`).
- `prograph/_core.pyi` is hand-maintained — update with every PyO3 surface change.
- Declarations use `/` as separator on every platform. Reject BEFORE normalize: absolute (leading `/` or stringly `^[A-Za-z]:`), `..` segments, `\` anywhere, empty. Then strip leading `./`, trailing `/`.
- Broken declarations are ParseWarnings / detection warnings — NEVER hard errors.
- Edge direction is always declarer → target; `mode` (`read`/`write`) is an attribute.
- New string constants exactly: edge kind `declared`, drift kind `stale_declaration`, drift entity kind `declared_path`.
- Commit messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: `DeclaredPath` fact + Python parser (tolerant extraction)

**Files:**
- Modify: `prograph-core/src/facts.rs` (new enum + struct near `McpToolDecl` ~line 76; field in `ProjectFacts` ~line 183)
- Modify: `prograph-core/src/parsers/mod.rs:34-45` (`ParserOutput` field)
- Modify: `prograph-core/src/parsers/python.rs` (extraction fn + wire into `parse`)
- Test: inline `#[cfg(test)]` in `python.rs`

**Interfaces:**
- Produces: `facts::DeclaredMode { Read, Write }`, `facts::DeclaredPath { mode, path: String, source_path: String, line: u32, snippet: Option<String> }` (u32 matches sibling facts like `McpToolDecl.line`; the spec's i64 applies at the `EvidenceLocation` boundary — cast there), `ProjectFacts.declared_paths: Vec<DeclaredPath>` and `ParserOutput.declared_paths: Vec<DeclaredPath>` (both `#[serde(default)]` where serde applies).
- Produces: `python::extract_declared_paths(contents: &str, source_path: &str, warnings: &mut Vec<ParseWarning>) -> Vec<DeclaredPath>` — reused verbatim by Task 2's Cargo variant via a shared helper (see Step 3).
- IMPORTANT: paths are stored RAW here (trimmed of whitespace only). Validation/normalization is the detector's job (Task 4) — the parser must not silently drop what the detector wants to warn about.

- [ ] **Step 1: Write failing tests**

Append to `mod tests` in `prograph-core/src/parsers/python.rs`:

```rust
    #[test]
    fn extracts_declared_reads_and_writes_with_lines() {
        let toml = r#"[project]
name = "dispatcher"
version = "1.0"

[tool.prograph]
reads = ["proctor/config/proctor.yaml", "proctor/data/state.db"]
writes = ["prograph-vault/derived/"]
"#;
        let mut warnings = Vec::new();
        let dp = extract_declared_paths(toml, "pyproject.toml", &mut warnings);
        assert!(warnings.is_empty());
        assert_eq!(dp.len(), 3);
        assert_eq!(dp[0].mode, crate::facts::DeclaredMode::Read);
        assert_eq!(dp[0].path, "proctor/config/proctor.yaml");
        assert_eq!(dp[0].source_path, "pyproject.toml");
        assert_eq!(dp[0].line, 6, "reads entries live on line 6");
        assert!(dp[0].snippet.as_deref().unwrap().contains("proctor/config"));
        assert_eq!(dp[2].mode, crate::facts::DeclaredMode::Write);
        assert_eq!(dp[2].path, "prograph-vault/derived/");
        assert_eq!(dp[2].line, 7);
    }

    #[test]
    fn malformed_reads_warns_but_does_not_break_manifest_parse() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "p"
version = "1.0"
dependencies = ["requests"]

[tool.prograph]
reads = "not-a-list"
"#,
        )
        .unwrap();
        let out = parse(dir.path()).unwrap();
        // Manifest itself parsed fine — deps intact.
        let m = out.manifest.expect("manifest must survive broken declarations");
        assert_eq!(m.declared_name, "p");
        assert!(!m.deps.is_empty());
        // Declarations skipped with a warning.
        assert!(out.declared_paths.is_empty());
        assert!(out.warnings.iter().any(|w| w.message.contains("reads")));
    }

    #[test]
    fn non_string_items_warn_and_skip_only_those_items() {
        let toml = "[tool.prograph]\nreads = [\"ok/path\", 42]\n";
        let mut warnings = Vec::new();
        let dp = extract_declared_paths(toml, "pyproject.toml", &mut warnings);
        assert_eq!(dp.len(), 1);
        assert_eq!(dp[0].path, "ok/path");
        assert_eq!(warnings.len(), 1);
    }
```

Check the existing `ParseWarning` struct shape (`grep -n "struct ParseWarning" prograph-core/src/facts.rs`) and construct warnings the way `python.rs` already does — adapt the `w.message` field access if the field is named differently.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path prograph-core/Cargo.toml extracts_declared malformed_reads non_string_items 2>&1 | tail -5`
Expected: compile error — `extract_declared_paths` not found.

- [ ] **Step 3: Implement**

In `prograph-core/src/facts.rs`, after `McpToolDecl` (~line 82):

```rust
/// M12: file-based integration declared in a manifest (`[tool.prograph] reads/writes`
/// in pyproject.toml, `[package.metadata.prograph]` in Cargo.toml).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeclaredMode {
    Read,
    Write,
}

impl DeclaredMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// A single declared path with its manifest source location — captured by the
/// parser in one pass so evidence and stale checks never re-scan the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredPath {
    pub mode: DeclaredMode,
    /// Workspace-relative path exactly as declared (whitespace-trimmed).
    /// Validation/normalization happens in `detectors::declared`.
    pub path: String,
    /// Manifest file relative to the project root ("pyproject.toml" | "Cargo.toml").
    pub source_path: String,
    /// 1-based line of the entry in the manifest. Best-effort text scan.
    pub line: u32,
    pub snippet: Option<String>,
}
```

Add to `ProjectFacts` (after `intent`, keeping the M-comment style):

```rust
    /// M12: file-based integrations declared in the manifest.
    #[serde(default)]
    pub declared_paths: Vec<DeclaredPath>,
```

Add to `ParserOutput` in `parsers/mod.rs`:

```rust
    /// M12: declared file-based integrations (`[tool.prograph] reads/writes`).
    pub declared_paths: Vec<crate::facts::DeclaredPath>,
```

Fix every `ParserOutput { ... }` literal (grep for `ParserOutput {`) to add `declared_paths: vec![]` — including the `_ =>` fallback arm in `parse_project` and the JS parser if it builds the struct literally. The indexer (`indexer.rs` Phase 2) copies parser output into `ProjectFacts` — add `declared_paths: out.declared_paths` there (grep `mcp_decls: out.mcp_decls` to find the spot).

In `parsers/python.rs`, the tolerant extractor — deliberately NOT the typed-deserialize path (`PyprojectToolPrograph` stays untouched for aliases/exclude; a non-list `reads` must not fail the whole typed parse, so declarations go through untyped `toml::Value`):

```rust
/// M12: extract `[tool.prograph] reads/writes` (or `[package.metadata.prograph]` —
/// the caller picks the table) tolerantly. Malformed shapes warn and skip; they
/// never fail the manifest parse.
pub fn extract_declared_from_table(
    table: Option<&toml::Value>,
    contents: &str,
    source_path: &str,
    warnings: &mut Vec<ParseWarning>,
) -> Vec<crate::facts::DeclaredPath> {
    use crate::facts::{DeclaredMode, DeclaredPath};
    let mut out = Vec::new();
    let Some(table) = table else { return out };
    for (key, mode) in [("reads", DeclaredMode::Read), ("writes", DeclaredMode::Write)] {
        let Some(value) = table.get(key) else { continue };
        let Some(items) = value.as_array() else {
            warnings.push(parse_warning(source_path, format!("`{key}` must be a list of strings")));
            continue;
        };
        for item in items {
            let Some(path) = item.as_str() else {
                warnings.push(parse_warning(source_path, format!("`{key}` items must be strings")));
                continue;
            };
            let path = path.trim().to_string();
            let (line, snippet) = find_manifest_line(contents, &path);
            out.push(DeclaredPath {
                mode,
                path,
                source_path: source_path.to_string(),
                line,
                snippet,
            });
        }
    }
    out
}

/// Best-effort 1-based line of the first manifest line containing `needle`.
fn find_manifest_line(contents: &str, needle: &str) -> (u32, Option<String>) {
    for (i, ln) in contents.lines().enumerate() {
        if ln.contains(needle) {
            return ((i + 1) as u32, Some(ln.trim().to_string()));
        }
    }
    (1, None)
}

pub fn extract_declared_paths(
    contents: &str,
    source_path: &str,
    warnings: &mut Vec<ParseWarning>,
) -> Vec<crate::facts::DeclaredPath> {
    let value: toml::Value = match toml::from_str(contents) {
        Ok(v) => v,
        Err(_) => return Vec::new(), // whole-file TOML errors are reported by the main parse
    };
    let table = value.get("tool").and_then(|t| t.get("prograph"));
    extract_declared_from_table(table, contents, source_path, warnings)
}
```

`parse_warning(...)` — construct however `python.rs` builds its existing `ParseWarning`s (find one and copy the constructor shape; if there is a helper, use it, otherwise build the struct literally). Wire into `python::parse`: after the manifest is read, call `extract_declared_paths(&contents, "pyproject.toml", &mut warnings)` and set the field on the returned `ParserOutput`.

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path prograph-core/Cargo.toml extracts_declared malformed_reads non_string_items 2>&1 | tail -5`
Expected: 3 passed. Then the whole crate: `cargo test --all-targets 2>&1 | tail -3` — all green (struct-literal fixes everywhere).

- [ ] **Step 5: fmt, clippy, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add prograph-core/src/facts.rs prograph-core/src/parsers/mod.rs prograph-core/src/parsers/python.rs prograph-core/src/indexer.rs
git commit -m "feat(core): DeclaredPath fact + tolerant [tool.prograph] reads/writes extraction

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Cargo parser + Mixed-project union

**Files:**
- Modify: `prograph-core/src/parsers/rust.rs` (extraction from `[package.metadata.prograph]`)
- Modify: `prograph-core/src/parsers/mod.rs:76-86` (`parse_mixed` union)
- Test: inline tests in `rust.rs` and `mod.rs`

**Interfaces:**
- Consumes: `python::extract_declared_from_table(table, contents, source_path, warnings)` (Task 1).
- Produces: `rust::parse` populates `declared_paths` from `Cargo.toml`; `parse_mixed` returns the UNION of Python + Rust declarations (per-entry `source_path` preserved) even though the canonical manifest stays Python.

- [ ] **Step 1: Write failing tests**

In `rust.rs` tests (follow the file's existing TempDir fixture style):

```rust
    #[test]
    fn extracts_declared_paths_from_cargo_metadata() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "watcher"
version = "0.1.0"

[package.metadata.prograph]
reads = ["maestro/out/plan.json"]
"#,
        )
        .unwrap();
        let out = parse(dir.path()).unwrap();
        assert_eq!(out.declared_paths.len(), 1);
        assert_eq!(out.declared_paths[0].path, "maestro/out/plan.json");
        assert_eq!(out.declared_paths[0].source_path, "Cargo.toml");
        assert_eq!(out.declared_paths[0].line, 6);
    }
```

In `mod.rs` tests:

```rust
    #[test]
    fn mixed_project_unions_declared_paths_from_both_manifests() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"m\"\nversion = \"1.0\"\n[tool.prograph]\nreads = [\"a/x\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"m-core\"\nversion = \"0.1.0\"\n[package.metadata.prograph]\nreads = [\"b/y\"]\n",
        )
        .unwrap();
        let out = parse_project(dir.path(), crate::models::ProjectKind::Mixed).unwrap();
        assert_eq!(out.manifest.as_ref().unwrap().declared_name, "m", "canonical stays Python");
        let mut paths: Vec<(&str, &str)> = out
            .declared_paths
            .iter()
            .map(|d| (d.path.as_str(), d.source_path.as_str()))
            .collect();
        paths.sort();
        assert_eq!(paths, vec![("a/x", "pyproject.toml"), ("b/y", "Cargo.toml")]);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path prograph-core/Cargo.toml extracts_declared_paths_from_cargo mixed_project_unions 2>&1 | tail -5`
Expected: FAIL (rust parser produces empty declared_paths; mixed drops Cargo's).

- [ ] **Step 3: Implement**

In `rust.rs::parse`, after the Cargo.toml contents are read (the parser already parses it as TOML — reuse the parsed `toml::Value` if one exists, else parse contents into one):

```rust
    let value: toml::Value = toml::from_str(&contents).unwrap_or(toml::Value::Table(Default::default()));
    let table = value
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("prograph"));
    let declared_paths = crate::parsers::python::extract_declared_from_table(
        table, &contents, "Cargo.toml", &mut warnings,
    );
```

(Adapt variable names to the function's actual locals; set the field on the returned `ParserOutput`.)

In `parse_mixed` (`mod.rs`), union instead of early-return-drops:

```rust
fn parse_mixed(root: &Path) -> Result<ParserOutput> {
    let py = python::parse(root)?;
    if py.manifest.is_some() {
        // Canonical output stays Python, but [package.metadata.prograph] in the
        // co-located Cargo.toml must not be silently ignored (spec: Mixed merges
        // declared_paths from BOTH manifests).
        let mut out = py;
        if let Ok(rs) = rust::parse(root) {
            out.declared_paths.extend(rs.declared_paths);
        }
        return Ok(out);
    }
    let rs = rust::parse(root)?;
    if rs.manifest.is_some() {
        return Ok(rs);
    }
    js::parse(root)
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --all-targets 2>&1 | tail -3`
Expected: all green (note: `rust::parse` inside mixed now runs its whole pipeline — if that measurably slows existing tests or double-reports warnings, keep ONLY `declared_paths` from the secondary parse, which the code above already does implicitly by extending just that field).

- [ ] **Step 5: fmt, clippy, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add prograph-core/src/parsers/rust.rs prograph-core/src/parsers/mod.rs
git commit -m "feat(core): Cargo [package.metadata.prograph] declarations + Mixed union

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Enums + migration v10 + `_core.pyi`

**Files:**
- Modify: `prograph-core/src/models.rs:93-111` (`EdgeKind::Declared`)
- Modify: `prograph-core/src/drift.rs:18-50` (`DriftKind::StaleDeclaration`, `EntityKind::DeclaredPath`)
- Create: `prograph-core/src/migrations/v10.sql`
- Modify: `prograph-core/src/store.rs:9-19` (MIGRATIONS array)
- Modify: `prograph/_core.pyi` (EdgeKind stub member)
- Test: inline store test

**Interfaces:**
- Produces: `EdgeKind::Declared` with `name() == "declared"`; `DriftKind::StaleDeclaration.as_str() == "stale_declaration"`; `EntityKind::DeclaredPath.as_str() == "declared_path"`; DB schema v10 accepting all three strings.

- [ ] **Step 1: Write the failing store test**

In `prograph-core/src/store.rs` tests (find the test module; it has helpers that open a fresh store):

```rust
    #[test]
    fn v10_accepts_declared_edge_and_stale_declaration_drift() {
        // Fresh store runs the full migration chain — schema_version must be 10
        // and the widened CHECKs must accept the new kind strings.
        let dir = tempfile::TempDir::new().unwrap();
        let store = Store::open(&dir.path().join("g.db")).unwrap();
        let v: i64 = store
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 10);
        store
            .conn()
            .execute_batch(
                "INSERT INTO snapshots (ts, monorepo_root, prograph_version, n_projects, n_edges, n_changes)
                 VALUES ('t', '/x', '0', 0, 0, 0);
                 INSERT INTO projects (name, root_path, kind, first_seen, last_seen)
                 VALUES ('a', './a', 'python', 1, 1), ('b', './b', 'python', 1, 1);
                 INSERT INTO edges (kind, from_kind, from_id, to_kind, to_id, attrs_json, attrs_hash, first_seen, last_seen)
                 VALUES ('declared', 'project', 1, 'project', 2, '{}', 'h1', 1, 1);
                 INSERT INTO drift_findings (snapshot_id, project_id, kind, entity_kind, entity_name, source_path, source_line, confidence, first_seen, last_seen)
                 VALUES (1, 1, 'stale_declaration', 'declared_path', 'b/x.db', 'pyproject.toml', 3, 'high', 1, 1);",
            )
            .expect("v10 CHECKs must accept the new kinds");
    }
```

Adapt column lists to the REAL schemas: read `prograph-core/src/migrations/v1.sql` (snapshots, projects), `v3.sql` (edges), `v9.sql` (drift_findings) first and fix the INSERT column lists/`conn()` accessor to match what the store test module actually exposes (other tests in the module show the pattern — copy it).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path prograph-core/Cargo.toml v10_accepts 2>&1 | tail -5`
Expected: FAIL — user_version is 9 / CHECK rejects `'declared'`.

- [ ] **Step 3: Implement**

`models.rs` — add the variant and `name()` arm:

```rust
pub enum EdgeKind {
    PackageDep,
    McpCall,
    ContractLink,
    Declared,
}
// ... in name():
            EdgeKind::Declared => "declared",
```

Grep for every exhaustive `match` over `EdgeKind` (`cargo build` will list them as errors) — add `Declared` arms mapping to/from the string `"declared"` (there is at least a from-string parser in store.rs or models.rs; find it via the compile errors).

`drift.rs`:

```rust
pub enum DriftKind {
    Missing,
    Extra,
    StaleTodo,
    StaleDeclaration,
}
// as_str(): Self::StaleDeclaration => "stale_declaration",

pub enum EntityKind {
    PublicSymbol,
    McpTool,
    Contract,
    Todo,
    DeclaredPath,
}
// as_str(): Self::DeclaredPath => "declared_path",
```

`migrations/v10.sql` — table rebuild (SQLite cannot ALTER a CHECK). **First read `v6.sql`** for the house rebuild pattern (pragmas, index recreation); then write v10 following it. The target definitions:

```sql
-- v10: declared edges (M12). Two CHECK constraints widen:
--   edges.kind          += 'declared'
--   drift_findings.kind += 'stale_declaration', entity_kind += 'declared_path'
-- SQLite cannot ALTER a CHECK -> rebuild both tables (pattern per v6).

CREATE TABLE edges_v10 (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL CHECK (kind IN ('package_dep', 'mcp_call', 'contract_link', 'declared')),
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
INSERT INTO edges_v10 SELECT * FROM edges;
DROP TABLE edges;
ALTER TABLE edges_v10 RENAME TO edges;
-- Recreate every index that v1..v9 defined on edges (grep the earlier .sql files
-- for "ON edges" and copy each CREATE INDEX verbatim).
```

Same shape for `drift_findings` (copy the v9 definition, widen the two CHECKs, preserve the UNIQUE constraint, recreate its indexes). Append `PRAGMA user_version = 10;` if that is how earlier migrations bump the version — check how v9 does it (grep `user_version` across `migrations/*.sql` and `store.rs`; replicate the mechanism exactly).

`store.rs` MIGRATIONS array: add `(10, include_str!("migrations/v10.sql")),`.

`prograph/_core.pyi`: find `class EdgeKind` and add the `Declared` member mirroring the existing members' style.

- [ ] **Step 4: Run tests + rebuild**

```bash
cargo test --all-targets 2>&1 | tail -3     # all green incl. v10_accepts
uv run maturin develop
uv run pytest -x -q 2>&1 | tail -3          # existing Python suite green
```

- [ ] **Step 5: fmt, clippy, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add prograph-core/src/models.rs prograph-core/src/drift.rs prograph-core/src/migrations/v10.sql prograph-core/src/store.rs prograph/_core.pyi
git commit -m "feat(core): EdgeKind::Declared + stale_declaration drift kind + schema v10

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: `detectors/declared.rs` — validation, resolution, edges, stale, warnings

**Files:**
- Create: `prograph-core/src/detectors/declared.rs`
- Modify: `prograph-core/src/detectors/mod.rs` (module decl + `DetectionResult.warnings: Vec<String>`)
- Test: inline in `declared.rs`

**Interfaces:**
- Consumes: `ProjectFacts.declared_paths` (Task 1/2), `EdgeKind::Declared`, `DriftKind::StaleDeclaration`, `EntityKind::DeclaredPath` (Task 3), `EdgeCandidate`/`EvidenceLocation` (existing, `detectors/mod.rs:14-39`), `DriftFinding` (drift.rs:7).
- Produces:

```rust
pub struct DeclaredDetection {
    pub edges: Vec<EdgeCandidate>,
    /// (declaring-project index, finding) — the indexer resolves the project id.
    pub stale: Vec<(usize, crate::drift::DriftFinding)>,
    pub warnings: Vec<String>,
}
pub fn detect_declared(facts: &[ProjectFacts], monorepo_root: &Path) -> DeclaredDetection
```

- Also produces: `DetectionResult` gains `pub warnings: Vec<String>` (empty from existing detectors; deliberately NOT the deps.rs thread-local drain — new code returns data).

- [ ] **Step 1: Write failing tests**

`prograph-core/src/detectors/declared.rs` skeleton with tests first:

```rust
//! M12: declared edges — file-based integrations declared in manifests.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{DeclaredMode, DeclaredPath, ProjectFacts};

    fn fact(root: &str, name: &str, decls: Vec<DeclaredPath>) -> ProjectFacts {
        ProjectFacts {
            project_root: root.to_string(),
            project_name: name.to_string(),
            declared_paths: decls,
            ..Default::default()
        }
    }

    fn decl(mode: DeclaredMode, path: &str) -> DeclaredPath {
        DeclaredPath {
            mode,
            path: path.to_string(),
            source_path: "pyproject.toml".to_string(),
            line: 7,
            snippet: Some(format!("reads = [\"{path}\"]")),
        }
    }

    fn setup(paths_on_disk: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        for p in paths_on_disk {
            let full = dir.path().join(p);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, "x").unwrap();
        }
        dir
    }

    #[test]
    fn resolves_edge_with_evidence_and_attrs() {
        let dir = setup(&["proctor/data/state.db"]);
        let facts = vec![
            fact("./dispatcher", "dispatcher", vec![decl(DeclaredMode::Read, "proctor/data/state.db")]),
            fact("./proctor", "proctor", vec![]),
        ];
        let det = detect_declared(&facts, dir.path());
        assert!(det.warnings.is_empty());
        assert!(det.stale.is_empty());
        assert_eq!(det.edges.len(), 1);
        let e = &det.edges[0];
        assert_eq!(e.from_idx, 0);
        assert_eq!(e.to_idx, 1);
        assert!(e.attrs_json.contains("\"mode\":\"read\""));
        assert!(e.attrs_json.contains("proctor/data/state.db"));
        assert_eq!(e.evidence.len(), 1);
        assert_eq!(e.evidence[0].rel_path, "pyproject.toml");
        assert_eq!(e.evidence[0].line, 7);
    }

    #[test]
    fn segment_aware_prefix_rejects_lookalike() {
        let dir = setup(&["proctor2/data/x"]);
        let facts = vec![
            fact("./dispatcher", "dispatcher", vec![decl(DeclaredMode::Read, "proctor2/data/x")]),
            fact("./proctor", "proctor", vec![]),
        ];
        let det = detect_declared(&facts, dir.path());
        assert!(det.edges.is_empty(), "proctor2 must not match project proctor");
        assert_eq!(det.warnings.len(), 1);
    }

    #[test]
    fn longest_match_wins_for_nested_members() {
        let dir = setup(&["atp-platform/packages/atp-sdk/x.json"]);
        let facts = vec![
            fact("./maestro", "maestro", vec![decl(DeclaredMode::Read, "atp-platform/packages/atp-sdk/x.json")]),
            fact("./atp-platform", "atp-platform", vec![]),
            fact("./atp-platform/packages/atp-sdk", "atp-sdk", vec![]),
        ];
        let det = detect_declared(&facts, dir.path());
        assert_eq!(det.edges.len(), 1);
        assert_eq!(det.edges[0].to_idx, 2, "nested member wins over its parent");
    }

    #[test]
    fn invalid_paths_warn_without_edges() {
        let dir = setup(&[]);
        let bad = ["/abs/x", "C:\\win\\x", "a/../b", "with\\backslash", ""];
        let decls = bad.iter().map(|p| decl(DeclaredMode::Read, p)).collect();
        let facts = vec![fact("./d", "d", decls), fact("./a", "a", vec![])];
        let det = detect_declared(&facts, dir.path());
        assert!(det.edges.is_empty());
        assert_eq!(det.warnings.len(), bad.len());
    }

    #[test]
    fn self_reference_warns() {
        let dir = setup(&["d/own.db"]);
        let facts = vec![fact("./d", "d", vec![decl(DeclaredMode::Read, "d/own.db")])];
        let det = detect_declared(&facts, dir.path());
        assert!(det.edges.is_empty());
        assert_eq!(det.warnings.len(), 1);
    }

    #[test]
    fn missing_target_file_is_stale_declaration() {
        let dir = setup(&[]); // nothing on disk
        std::fs::create_dir_all(dir.path().join("proctor")).unwrap();
        let facts = vec![
            fact("./dispatcher", "dispatcher", vec![decl(DeclaredMode::Read, "proctor/gone.db")]),
            fact("./proctor", "proctor", vec![]),
        ];
        let det = detect_declared(&facts, dir.path());
        assert_eq!(det.edges.len(), 1, "edge still emitted — the DECLARATION exists");
        assert_eq!(det.stale.len(), 1);
        let (idx, f) = &det.stale[0];
        assert_eq!(*idx, 0, "stale attributed to the DECLARING project");
        assert_eq!(f.kind, crate::drift::DriftKind::StaleDeclaration);
        assert_eq!(f.entity_name, "proctor/gone.db");
        assert_eq!(f.source_path, "pyproject.toml");
        assert_eq!(f.source_line, 7);
    }

    #[test]
    fn directory_target_with_or_without_trailing_slash_is_not_stale() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("vault/derived")).unwrap();
        for declared in ["vault/derived/", "vault/derived"] {
            let facts = vec![
                fact("./p", "p", vec![decl(DeclaredMode::Write, declared)]),
                fact("./vault", "vault", vec![]),
            ];
            let det = detect_declared(&facts, dir.path());
            assert_eq!(det.edges.len(), 1);
            assert!(det.stale.is_empty(), "existing dir must not be stale ({declared})");
        }
    }

    #[test]
    fn two_declarations_two_edges_distinct_hashes() {
        let dir = setup(&["proctor/a", "proctor/b"]);
        let facts = vec![
            fact("./d", "d", vec![
                decl(DeclaredMode::Read, "proctor/a"),
                decl(DeclaredMode::Read, "proctor/b"),
            ]),
            fact("./proctor", "proctor", vec![]),
        ];
        let det = detect_declared(&facts, dir.path());
        assert_eq!(det.edges.len(), 2);
        assert_ne!(det.edges[0].attrs_hash, det.edges[1].attrs_hash);
    }
}
```

Note `ProjectFacts` needs `Default` for the `..Default::default()` spread — check whether it derives `Default`; if not, add `#[derive(Default)]`-compatible defaults or construct fully (crib the full-literal from another detector's tests). `DriftFinding` fields must be public and constructible — check drift.rs; if `DriftFinding` lacks `PartialEq` for the assert, compare fields individually.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path prograph-core/Cargo.toml declared 2>&1 | tail -5`
Expected: compile error — `detect_declared` missing.

- [ ] **Step 3: Implement**

Above the tests in `declared.rs`:

```rust
use std::path::Path;

use crate::detectors::{EdgeCandidate, EvidenceLocation};
use crate::drift::{Confidence, DriftFinding, DriftKind, EntityKind};
use crate::facts::ProjectFacts;
use crate::models::{EdgeKind, NodeKind};

/// Result of the declared-edges pipeline: edges to persist, stale findings
/// (keyed by declaring-project index), and human-readable warnings.
#[derive(Debug, Default)]
pub struct DeclaredDetection {
    pub edges: Vec<EdgeCandidate>,
    pub stale: Vec<(usize, DriftFinding)>,
    pub warnings: Vec<String>,
}

/// Validate a declared path. Returns the normalized path or a rejection reason.
/// Order matters: reject BEFORE normalizing so `/proctor/x` can't sneak through.
fn validate(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Err("empty path".into());
    }
    if path.contains('\\') {
        return Err("backslash separators are not supported (use '/')".into());
    }
    if path.starts_with('/') {
        return Err("absolute paths are not allowed".into());
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("absolute (drive-letter) paths are not allowed".into());
    }
    if path.split('/').any(|seg| seg == "..") {
        return Err("`..` segments are not allowed".into());
    }
    let p = path.strip_prefix("./").unwrap_or(path);
    Ok(p.trim_end_matches('/').to_string())
}

/// Segment-aware "is `root` a path-prefix of `path`". Roots come from
/// ProjectFacts.project_root ("./proctor") — normalize the "./" off first.
fn is_segment_prefix(root: &str, path: &str) -> bool {
    path == root || path.starts_with(&format!("{root}/"))
}

/// Longest segment-aware match among project roots. Returns the facts index.
fn resolve_target(facts: &[ProjectFacts], path: &str) -> Option<usize> {
    facts
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let root = f.project_root.strip_prefix("./").unwrap_or(&f.project_root);
            is_segment_prefix(root, path).then_some((i, root.len()))
        })
        .max_by_key(|&(_, len)| len)
        .map(|(i, _)| i)
}

pub fn detect_declared(facts: &[ProjectFacts], monorepo_root: &Path) -> DeclaredDetection {
    let mut det = DeclaredDetection::default();
    for (from_idx, fact) in facts.iter().enumerate() {
        for d in &fact.declared_paths {
            let norm = match validate(&d.path) {
                Ok(p) => p,
                Err(why) => {
                    det.warnings.push(format!(
                        "{}: declared path '{}' rejected: {}",
                        fact.project_name, d.path, why
                    ));
                    continue;
                }
            };
            let Some(to_idx) = resolve_target(facts, &norm) else {
                det.warnings.push(format!(
                    "{}: declared path '{}' matches no tracked project",
                    fact.project_name, norm
                ));
                continue;
            };
            if to_idx == from_idx {
                det.warnings.push(format!(
                    "{}: declared path '{}' points at the declaring project itself",
                    fact.project_name, norm
                ));
                continue;
            }
            let attrs = serde_json::json!({ "mode": d.mode.as_str(), "path": norm });
            let attrs_json = serde_json::to_string(&attrs).unwrap();
            let attrs_hash = edge_attrs_hash("declared", &attrs_json);
            det.edges.push(EdgeCandidate {
                kind: EdgeKind::Declared,
                from_kind: NodeKind::Project,
                from_idx,
                to_kind: NodeKind::Project,
                to_idx,
                attrs_json,
                attrs_hash,
                evidence: vec![EvidenceLocation {
                    project_idx: from_idx,
                    rel_path: d.source_path.clone(),
                    line: d.line as i64,
                    snippet: d.snippet.clone(),
                }],
            });
            // Stale check: the declaration resolved, but does the path exist?
            if !monorepo_root.join(&norm).exists() {
                det.stale.push((
                    from_idx,
                    DriftFinding {
                        kind: DriftKind::StaleDeclaration,
                        entity_kind: EntityKind::DeclaredPath,
                        entity_name: norm.clone(),
                        source_path: d.source_path.clone(),
                        source_line: d.line,
                        confidence: Confidence::High,
                        detail: Some(format!(
                            "declared {} target no longer exists on disk",
                            d.mode.as_str()
                        )),
                    },
                ));
            }
        }
    }
    det
}
```

`edge_attrs_hash(...)`: find how `deps.rs`/`mcp.rs` compute `attrs_hash` (grep `attrs_hash` in `detectors/`) and REUSE that helper — if it's local to a module, promote it to `detectors/mod.rs` and have both callers use it. The hash input must include the kind prefix (the existing scheme does — mirror it exactly). Note the spec's identity requirement: hash over mode+path (attrs_json here contains exactly those two, so hashing attrs_json with kind prefix satisfies it).

In `detectors/mod.rs`: `pub mod declared;`, add `#[derive(Default)] pub struct DeclaredDetection {...}` OR define it in `declared.rs` and re-export — keep it next to `detect_declared` in `declared.rs` (one file, one responsibility) with `pub use declared::{detect_declared, DeclaredDetection};` in mod.rs if other modules need it. Add `pub warnings: Vec<String>` to `DetectionResult` (initialize empty in `detect_all`; existing detectors don't populate it).

Check `DriftFinding` field types against drift.rs:7 (`source_line: u32`, `detail: Option<String>`) — the code above assumes them; fix mismatches in the test/impl, not by changing drift.rs.

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path prograph-core/Cargo.toml declared 2>&1 | tail -5` → 8 passed; then `cargo test --all-targets 2>&1 | tail -3` → all green.

- [ ] **Step 5: fmt, clippy, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
git add prograph-core/src/detectors/
git commit -m "feat(core): declared-edges detector — validation, resolution, stale, warnings

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Indexer wiring + full-identity evidence lookup fix

**Files:**
- Modify: `prograph-core/src/indexer.rs` (call site after `detect_all` ~line 60; evidence lookup ~line 418-424; drift persist ~line 574; warning merge)
- Test: inline in `indexer.rs`

**Interfaces:**
- Consumes: `detect_declared(&facts, monorepo_root) -> DeclaredDetection` (Task 4).
- Produces: `declared` edges persisted with per-edge evidence; stale findings in `drift_findings`; warnings counted in `IndexSummary.n_warnings`. Evidence lookup matches on FULL identity `kind|from_root|to_endpoint|attrs_hash` — fixing the latent attrs_hash-only bug for all kinds.

- [ ] **Step 1: Write failing tests**

In `indexer.rs` tests:

```rust
    #[test]
    fn declared_edge_persisted_with_evidence_and_stale_drift() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();
        fs::create_dir_all(dir.path().join("proctor/data")).unwrap();
        fs::write(dir.path().join("proctor/pyproject.toml"), "[project]\nname=\"proctor\"\nversion=\"1\"\n").unwrap();
        fs::write(dir.path().join("proctor/data/state.db"), "x").unwrap();
        fs::write(
            dir.path().join("dispatcher/pyproject.toml"),
            "[project]\nname=\"dispatcher\"\nversion=\"1\"\n[tool.prograph]\nreads=[\"proctor/data/state.db\", \"proctor/data/gone.db\"]\n",
        )
        .map_err(|_| ()).ok(); // dispatcher dir must exist first:
        // (create_dir_all THEN write — fix ordering when transcribing)
        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        let summary = index_monorepo(dir.path(), &mut store, None).unwrap();
        assert_eq!(summary.n_projects, 2);
        // one edge per declaration (both resolve to proctor):
        let n_declared: i64 = store.conn().query_row(
            "SELECT COUNT(*) FROM edges WHERE kind='declared'", [], |r| r.get(0)).unwrap();
        assert_eq!(n_declared, 2);
        // each declared edge carries manifest evidence:
        let n_ev: i64 = store.conn().query_row(
            "SELECT COUNT(*) FROM edge_evidence WHERE edge_id IN (SELECT id FROM edges WHERE kind='declared')",
            [], |r| r.get(0)).unwrap();
        assert_eq!(n_ev, 2);
        // gone.db -> stale_declaration attributed to dispatcher:
        let (kind, name): (String, String) = store.conn().query_row(
            "SELECT kind, entity_name FROM drift_findings WHERE kind='stale_declaration'",
            [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(kind, "stale_declaration");
        assert_eq!(name, "proctor/data/gone.db");
    }

    #[test]
    fn same_path_two_declarers_each_edge_keeps_own_evidence() {
        // Two projects declare the SAME mode+path -> identical attrs_hash.
        // The old attrs_hash-only lookup would give both edges the first
        // project's evidence; full-identity lookup must keep them apart.
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();
        for (name, extra) in [("reader-a", ""), ("reader-b", ""), ("proctor", "")] {
            fs::create_dir_all(dir.path().join(name)).unwrap();
            let decl = if name.starts_with("reader") {
                "[tool.prograph]\nreads=[\"proctor/state.db\"]\n"
            } else { extra };
            fs::write(
                dir.path().join(name).join("pyproject.toml"),
                format!("[project]\nname=\"{name}\"\nversion=\"1\"\n{decl}"),
            ).unwrap();
        }
        fs::write(dir.path().join("proctor/state.db"), "x").unwrap();
        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store, None).unwrap();
        // Each declared edge's evidence must point at ITS OWN project's manifest.
        let rows: Vec<(i64, i64)> = store.conn()
            .prepare("SELECT e.from_id, ev.project_id FROM edges e JOIN edge_evidence ev ON ev.edge_id = e.id WHERE e.kind='declared'").unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
            .collect::<std::result::Result<_, _>>().unwrap();
        assert_eq!(rows.len(), 2);
        for (from_id, ev_project) in rows {
            assert_eq!(from_id, ev_project, "evidence must belong to the declaring project");
        }
    }
```

(Transcribe carefully: create each project directory before writing its pyproject; the first test's comment marks the ordering trap. Use the module's existing `Store::open`/`conn()` access pattern.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path prograph-core/Cargo.toml declared_edge_persisted same_path_two 2>&1 | tail -5`
Expected: FAIL — no declared edges persisted (detector not wired).

- [ ] **Step 3: Implement**

In `index_monorepo`, right after `detect_all` (~line 60 area of Phase 3):

```rust
    let declared = crate::detectors::declared::detect_declared(&facts, monorepo_root);
    warning_count += declared.warnings.len() as i64;
    let mut edge_candidates = detection.edges;
    edge_candidates.extend(declared.edges);
```

(Adapt to the real local names — `edge_candidates` currently binds from `detection.edges` directly; make it `mut` and extend.)

**Evidence lookup fix** (~line 418): replace the attrs_hash-only `.find` with a full-identity match, and delete the stale "so it's unique" comment:

```rust
        // Locate the corresponding EdgeCandidate so we can persist its evidence (M7).
        // Match on the FULL identity — two edges may share attrs_hash (e.g. two
        // projects declaring the same mode+path), so attrs_hash alone is ambiguous.
        let evidence = edge_candidates
            .iter()
            .find(|c| {
                c.attrs_hash == attrs_hash
                    && c.kind.name() == kind
                    && facts[c.from_idx].project_root == from_root
                    && candidate_to_endpoint(c, &facts, &contract_candidates) == to_endpoint
            })
            .map(|c| c.evidence.clone())
            .unwrap_or_default();
```

`candidate_to_endpoint(...)` — extract the SAME endpoint-string computation the identity-key builder uses at ~line 110-130 (`new_edge_attrs` map) into a small helper fn so both sites share it; do NOT duplicate the format string. Look at how the key builder derives `<to_endpoint>` for project vs contract targets and lift that code verbatim into the helper.

**Stale persist** — in the M11 drift persist section (~line 574), after the per-fact `detect_all` loop, add:

```rust
    for (from_idx, f) in &declared.stale {
        let Some(&pid) = new_project_ids.get(&facts[*from_idx].project_root) else {
            continue;
        };
        writer.insert_drift_finding(
            snap_id,
            pid,
            f.kind.as_str(),
            f.entity_kind.as_str(),
            &f.entity_name,
            &f.source_path,
            f.source_line as i64,
            f.confidence.as_str(),
            f.detail.as_deref(),
        )?;
    }
```

Also merge `detection.warnings` (the new empty field from Task 4) into `warning_count` for symmetry: `warning_count += detection.warnings.len() as i64;`.

- [ ] **Step 4: Run tests**

Run: `cargo test --all-targets 2>&1 | tail -3`
Expected: all green — including the two new tests AND every pre-existing evidence test (the full-identity fix must not regress `edge_evidence_persisted_for_mcp_call` etc.).

- [ ] **Step 5: fmt, clippy, rebuild, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
uv run maturin develop && uv run pytest -x -q 2>&1 | tail -3
git add prograph-core/src/indexer.rs prograph-core/src/detectors/mod.rs
git commit -m "feat(core): wire declared detector into indexer; evidence lookup by full edge identity

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: MCP + CLI drift surfaces

**Files:**
- Modify: `prograph/mcp_server.py:244` (find_edges kind enum + description ~line 259; find_drifts kind enum if it enumerates)
- Modify: `prograph/cli.py:480` (`--kind` help), `prograph/cli.py:520` (render loop)
- Test: `tests/integration/test_mcp_find_drifts.py` (append) and `tests/integration/test_cli_drift.py` (append)

**Interfaces:**
- Consumes: compiled `_core` with `declared`/`stale_declaration` (Tasks 3-5; run `uv run maturin develop` first if the `.so` predates Task 5's commit).
- Produces: `find_edges` accepts `kind="declared"`; `prograph drift --kind stale_declaration` filters; text output renders a fourth group.

- [ ] **Step 1: Write failing tests**

Append to `tests/integration/test_cli_drift.py` (reuse its fixture/runner conventions — read the file first):

```python
def test_drift_kind_stale_declaration_accepted(tmp_path: Path) -> None:
    """--kind stale_declaration filters; a fresh declared-and-deleted path shows up."""
    (tmp_path / "owner").mkdir()
    (tmp_path / "owner" / "pyproject.toml").write_text(
        '[project]\nname="owner"\nversion="1"\n'
    )
    (tmp_path / "reader").mkdir()
    (tmp_path / "reader" / "pyproject.toml").write_text(
        '[project]\nname="reader"\nversion="1"\n'
        '[tool.prograph]\nreads=["owner/missing.db"]\n'
    )
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    result = runner.invoke(
        app, ["drift", "--monorepo", str(tmp_path), "--kind", "stale_declaration", "--json"]
    )
    assert result.exit_code == 0, result.output
    findings = _json.loads(result.stdout)
    assert any(f["entity_name"] == "owner/missing.db" for f in findings)

    text = runner.invoke(app, ["drift", "--monorepo", str(tmp_path)])
    assert "stale_declaration" in text.stdout.lower() or "stale declaration" in text.stdout.lower()
```

Append to `tests/integration/test_mcp_find_drifts.py` a `kind="stale_declaration"` acceptance case, and to the find_edges MCP test file (grep `find_edges` under `tests/integration/`) a `kind="declared"` acceptance case — both following each file's existing call pattern (these test the schema/plumbing accept the value; empty result lists are fine where the fixture has no declarations).

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/integration/test_cli_drift.py -v -k stale_declaration 2>&1 | tail -5`
Expected: FAIL — either `--kind` rejects the value or the text render loop never prints the group.

- [ ] **Step 3: Implement**

`cli.py` ~480: help becomes `"Filter: missing | extra | stale_todo | stale_declaration"`. ~520: `for k in ("missing", "extra", "stale_todo", "stale_declaration"):`. If the drift command validates `--kind` against a literal set elsewhere, extend it (grep `stale_todo` across `prograph/` for any third site).

`mcp_server.py` ~244: `"enum": ["package_dep", "mcp_call", "contract_link", "declared"]`; extend the description at ~259 with `", declared (the declarer's manifest [tool.prograph] entry)"`. Grep `stale_todo` in the same file — if `find_drifts` enumerates kinds, extend enum + description too.

- [ ] **Step 4: Run tests**

Run: `uv run pytest tests/integration/test_cli_drift.py tests/integration/test_mcp_find_drifts.py tests/integration/test_cli_mcp.py -q 2>&1 | tail -3`
Expected: green.

- [ ] **Step 5: Lint, typecheck, commit**

```bash
uv run ruff format prograph/cli.py prograph/mcp_server.py tests/integration/
uv run ruff check . && uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/cli.py prograph/mcp_server.py tests/integration/
git commit -m "feat(cli,mcp): declared edge kind + stale_declaration drift in filters and schemas

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: MD export suffixes + golden

**Files:**
- Modify: `prograph/export/render.py` (`_render_outbound` ~line 361; `_inbound_attr_suffix` — grep for it)
- Test: `tests/unit/test_export_render.py` (append)

**Interfaces:**
- Consumes: edges with `kind == "declared"`, `attrs = {"mode": "read"|"write", "path": "..."}`.
- Produces: outbound line suffix `` · read `proctor/data/state.db` `` (resp. `write`); inbound mirror via `_inbound_attr_suffix`.

- [ ] **Step 1: Write failing test**

Append to `tests/unit/test_export_render.py` (read its imports/fixtures first; it builds `OutboundEdge`-like objects — follow the existing suffix tests for package_dep):

```python
def test_declared_edge_suffix_shows_mode_and_path() -> None:
    e = OutboundEdge(
        target_name="proctor",
        target_slug="proctor",
        kind="declared",
        attrs={"mode": "read", "path": "proctor/data/state.db"},
    )
    line = _render_outbound(e)
    assert "· read `proctor/data/state.db`" in line
    assert "→" in line  # declared is directional, not ↔
```

(Adapt the constructor to `OutboundEdge`'s real fields — copy from the neighbouring package_dep suffix test.)

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/unit/test_export_render.py -v -k declared 2>&1 | tail -5`
Expected: FAIL — bare suffix.

- [ ] **Step 3: Implement**

In `_render_outbound` add an arm after `contract_link`:

```python
    elif e.kind == "declared":
        mode = e.attrs.get("mode")
        path = e.attrs.get("path")
        if mode and path:
            suffix = f" · {mode} `{path}`"
```

Mirror in `_inbound_attr_suffix` (same arm — read the function to see its shape and keep the two consistent). The arrow: `_render_outbound` line 361 uses `↔` only for contract_link — `declared` correctly falls into the `→` default; verify, don't change.

- [ ] **Step 4: Run + golden check**

```bash
uv run pytest tests/unit/test_export_render.py -q 2>&1 | tail -3
uv run pytest tests/integration/test_cli_export_md.py -q 2>&1 | tail -3
```
Golden fixtures contain no declared edges (fixtures gain them only in Task 9 if at all) — goldens must pass UNCHANGED here. If a golden diff appears, you broke an existing suffix path; fix the code, do not regenerate.

- [ ] **Step 5: Lint, commit**

```bash
uv run ruff format prograph/export/render.py tests/unit/test_export_render.py
uv run ruff check . && uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/export/render.py tests/unit/test_export_render.py
git commit -m "feat(export): declared-edge suffix (mode + path) in MD cards

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 8: Browser UI — edge style + drift panel group

**Files:**
- Modify: `prograph/web_static/graph.js:3-13` (KIND_COLORS/KIND_LINESTYLES) + the cytoscape style block (~line 52) for `line-dash-pattern`
- Modify: `prograph/web_static/app.js:232-241` (drift groups/labels/order)
- Test: `tests/unit/test_web_static.py` (append static asserts)

**Interfaces:**
- Consumes: `/api/graph` edges with `kind: "declared"`; `/api/drifts` findings with `kind: "stale_declaration"`.
- Produces: declared edges violet `#8a6fc8`, dashed with pattern `[2, 4]` (distinct from contract orange dashed and removed-status dotted); drift side panel renders a fourth group.

- [ ] **Step 1: Write failing static tests**

Append to `tests/unit/test_web_static.py`:

```python
def test_graph_js_styles_declared_edges() -> None:
    graph_js = (STATIC_DIR / "graph.js").read_text(encoding="utf-8")
    assert "declared: '#8a6fc8'" in graph_js
    assert "declared: 'dashed'" in graph_js
    assert "line-dash-pattern" in graph_js


def test_app_js_renders_stale_declaration_drift_group() -> None:
    app_js = (STATIC_DIR / "app.js").read_text(encoding="utf-8")
    assert "stale_declaration" in app_js
    assert "Stale declarations" in app_js
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/unit/test_web_static.py -v -k "declared or stale_declaration" 2>&1 | tail -5`
Expected: 2 FAIL.

- [ ] **Step 3: Implement**

`graph.js` constants:

```js
const KIND_COLORS = {
    package_dep: '#888',
    mcp_call: '#0fa3b1',
    contract_link: '#d8862c',
    declared: '#8a6fc8',
};

const KIND_LINESTYLES = {
    package_dep: 'solid',
    mcp_call: 'solid',
    contract_link: 'dashed',
    declared: 'dashed',
};
```

In the cytoscape edge style block (find where `'line-style'` is set as a function of the edge, ~line 97): add a sibling `'line-dash-pattern'` mapper returning `[2, 4]` for `kind === 'declared'` and cytoscape's default `[6, 3]` otherwise, e.g.:

```js
                    'line-dash-pattern': (e) =>
                        (e.data('kind') === 'declared' ? [2, 4] : [6, 3]),
```

(Place it next to the existing `'line-style'` entry; verify the surrounding selector applies to edges generally, not only one kind.)

`app.js` drift section (~232-241):

```js
        const groups = { missing: [], extra: [], stale_todo: [], stale_declaration: [] };
        // labels:
            stale_declaration: 'Stale declarations (declared path no longer exists)',
        // render order:
        ['missing', 'extra', 'stale_todo', 'stale_declaration'].forEach((k) => {
```

All DOM stays through `el()` — no innerHTML (enforced by the existing test).

- [ ] **Step 4: Run tests**

Run: `uv run pytest tests/unit/test_web_static.py -q 2>&1 | tail -3`
Expected: green (incl. the innerHTML guard).

- [ ] **Step 5: Commit**

```bash
uv run ruff check .
git add prograph/web_static/graph.js prograph/web_static/app.js tests/unit/test_web_static.py
git commit -m "feat(ui): declared edges dashed violet with dash pattern; stale-declaration drift group

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 9: E2E integration test + full gates

**Files:**
- Create: `tests/integration/test_declared_edges.py`

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write the E2E test**

```python
"""M12 declared edges — end-to-end: manifest declaration → edge → evidence → drift → API."""

import json as _json
from pathlib import Path

from fastapi.testclient import TestClient
from typer.testing import CliRunner

from prograph.cli import app
from prograph.web_app import build_app

runner = CliRunner()


def _setup(root: Path) -> None:
    (root / "proctor" / "data").mkdir(parents=True)
    (root / "proctor" / "pyproject.toml").write_text('[project]\nname="proctor"\nversion="1"\n')
    (root / "proctor" / "data" / "state.db").write_text("x")
    (root / "dispatcher").mkdir()
    (root / "dispatcher" / "pyproject.toml").write_text(
        '[project]\nname="dispatcher"\nversion="1"\n'
        "[tool.prograph]\n"
        'reads=["proctor/data/state.db", "proctor/data/vanished.db"]\n'
        'writes=["proctor/inbox/"]\n'
    )
    (root / "proctor" / "inbox").mkdir()
    runner.invoke(app, ["init", "--monorepo", str(root)])
    result = runner.invoke(app, ["index", "--monorepo", str(root), "--json"])
    assert result.exit_code == 0, result.output


def test_declared_edges_in_graph_api(tmp_path: Path) -> None:
    _setup(tmp_path)
    client = TestClient(build_app(tmp_path))
    with client:
        graph = client.get("/api/graph").json()
        declared = [e for e in graph["edges"] if e["kind"] == "declared"]
        assert len(declared) == 3  # 2 reads + 1 write, all resolving to proctor
        edge_detail = client.get(f"/api/edges/{declared[0]['edge_id']}").json()
        assert edge_detail["attrs"]["mode"] in ("read", "write")
        assert edge_detail["attrs"]["path"].startswith("proctor/")
        assert edge_detail["evidence"], "declared edge must carry manifest evidence"
        assert edge_detail["evidence"][0]["rel_path"] == "pyproject.toml"


def test_stale_declaration_in_drifts_api(tmp_path: Path) -> None:
    _setup(tmp_path)
    client = TestClient(build_app(tmp_path))
    with client:
        drifts = client.get("/api/drifts", params={"project": "dispatcher", "kind": "stale_declaration"}).json()
        names = {d["entity_name"] for d in drifts}
        assert "proctor/data/vanished.db" in names


def test_md_card_shows_declared_suffix(tmp_path: Path) -> None:
    _setup(tmp_path)
    result = runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.output
    card = (tmp_path / ".prograph" / "projects" / "dispatcher.md").read_text()
    assert "· read `proctor/data/state.db`" in card
```

(Adapt `/api/drifts` param names and edge-detail shapes to the real endpoints — read `web_app.py`'s `/api/drifts` and `/api/edges/{id}` handlers first and fix the assertions to their actual JSON.)

- [ ] **Step 2: Run**

Run: `uv run pytest tests/integration/test_declared_edges.py -v 2>&1 | tail -8`
Expected: PASS. Any failure here is an integration gap from Tasks 1-8 — fix the responsible layer, not the test (unless the test guessed an API shape wrong).

- [ ] **Step 3: Full gates**

```bash
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test --all-targets 2>&1 | tail -3
uv run maturin develop
uv run ruff check . && uv run ruff format --check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
uv run pytest -q > /tmp/pt.txt 2>&1; echo "exit=$?"; tail -3 /tmp/pt.txt
```
All green, exit=0.

- [ ] **Step 4: Commit**

```bash
git add tests/integration/test_declared_edges.py
git commit -m "test: declared edges end-to-end (graph API, evidence, drift, MD card)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 10: Rollout — declare real integrations, clean up the drift-checker allowlist

**Files (OUTSIDE this repo — parent monorepo + sibling repos; nothing here is committed to prograph):**
- Modify: `/Users/Andrei_Shtanakov/labs/all_ai_orchestrators/dispatcher/pyproject.toml` (add `[tool.prograph] reads`)
- Modify: `/Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph/pyproject.toml` (add `[tool.prograph] writes`)
- Modify: `/Users/Andrei_Shtanakov/labs/all_ai_orchestrators/devtools/graph-registry-allowlist.toml` (remove superseded entries)

**Interfaces:**
- Consumes: the released feature (Tasks 1-9 merged into the branch).

- [ ] **Step 1: Declare dispatcher's file reads**

Inspect `dispatcher/core/collectors/` to list the ACTUAL paths read (proctor collector reads `proctor/config/proctor.yaml`, `proctor/data/state.db`, plus logs — verify the exact log path in the collector source; also check the other collectors for their watched projects' paths). Then append to `dispatcher/pyproject.toml`:

```toml
[tool.prograph]
reads = [
  "proctor/config/proctor.yaml",
  "proctor/data/state.db",
  # + the verified log path(s) and the other collectors' inputs
]
```

dispatcher is its own git repo: commit there with an explanatory message.

- [ ] **Step 2: Declare prograph's vault writes**

In `prograph/pyproject.toml` (this repo, `[tool.prograph]` section already exists with `exclude`):

```toml
writes = ["prograph-vault/derived/"]
```

Commit in prograph.

- [ ] **Step 3: Reindex the monorepo, verify edges**

```bash
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators
uv run --project prograph prograph index --monorepo . --json
uv run --project prograph python3 -c "
from prograph import _core
db = '.prograph/graph.db'
pid = _core.project_by_name(db, 'proctor')
d = _core.describe_project(db, pid)
print('proctor inbound:', [(e.kind, e.source_name) for e in d.inbound])
"
```
Expected: proctor inbound now includes `('declared', 'dispatcher')` — proctor is no longer isolated. Also `stale_declaration` findings should be empty (`uv run --project prograph prograph drift --monorepo . --kind stale_declaration`) — if any fire, the declared paths in Step 1 were wrong; fix the declarations.

- [ ] **Step 4: Remove superseded allowlist entries, verify the checker**

In `devtools/graph-registry-allowlist.toml` remove the `dispatcher/proctor` and `dispatcher/*` entries (keep `Maestro/spec-runner` — runtime CLI, still undetectable). Run:

```bash
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/devtools && make graph-drift
```
Expected: exit 0 — the dispatcher pairs now arrive via real graph edges. If UNDETECTED pairs remain (e.g. dispatcher↔Maestro obs consumption not yet declared), either declare those reads too (preferred) or keep a NARROW allowlist entry per pair with an updated reason. Commit devtools.

- [ ] **Step 5: Refresh the vault export**

```bash
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators
uv run --project prograph prograph index --monorepo . --export-md --json
rsync -a --delete .prograph/projects/ prograph-vault/derived/projects/
cp .prograph/index.md prograph-vault/derived/graph/index.md
cd prograph-vault && git add derived/ && git commit -m "derived: refresh with declared edges (M12)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
