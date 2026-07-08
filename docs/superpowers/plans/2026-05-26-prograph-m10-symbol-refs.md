# prograph M10 — Cross-Project Symbol References Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After M10, prograph captures **where in the code** cross-project dependencies are exercised. Today (after M9) we know "Maestro depends on atp-platform via `package_dep`" — but not which files/lines actually consume which symbols. M10 closes that gap. A new auxiliary table `cross_project_symbol_refs` records: "Maestro's `maestro/clients/arbiter.py:18` imports `atp_platform.sdk.MaestroATPAdapter` from project atp-platform". A new MCP tool `find_symbol_references` answers "if I change `MaestroAPI`, who calls it?". The browser UI side panel gets an "Inbound references" section per project. Refactor impact analysis becomes a one-tool-call away for AI agents.

**Architecture:**
- **No new edge kind.** Existing 3 edge kinds (`package_dep`, `mcp_call`, `contract_link`) stay — they describe coarse relationships between projects. Symbol references are **auxiliary data citing specific source-lines** inside those relationships, not separate graph edges. This keeps the graph readable (10-30 edges in a realistic monorepo) while making the underlying code citations queryable.
- **Schema v7 adds one table** — `cross_project_symbol_refs` with `(from_project_id, from_module_id, line, to_project_id, to_module_path, to_symbol_name, first_seen, last_seen)`. Temporal like every other table; `from_module_path` deferred to `from_module_id` lookup (no denorm).
- **Resolver layer**, language-specific:
  - **Python**: dotted module name → publisher project lookup (via M3's name index + aliases) + file-path inside publisher (`atp_platform.sdk` → `atp_platform/sdk.py` or `atp_platform/sdk/__init__.py`).
  - **Rust**: crate name from `use <crate>::a::b::Symbol` → project lookup via Cargo.toml `[package].name`. Symbol-path-inside-crate resolution is best-effort (we match against the crate's top-level `public_symbols` from M9).
  - **JS**: skip in M10. Cross-project JS symbol resolution depends on package.json `exports` maze and the target monorepo has no JS MCP/cross-deps. M11+ if needed.
- **Parser changes**: drop the "internal imports only" filter that M9 introduced — collect ALL imports per source file. Internal vs external classification moves to the indexer (which has the full project list to do publisher matching).
- **Indexer resolves at persist time**. Each source-file import that resolves to an in-monorepo publisher produces one `cross_project_symbol_refs` row.

**Tech Stack additions (M10 only):**
- None. Resolution is pure Rust string processing + lookups against existing M3/M9 facts. No new tree-sitter grammars; M9's `*_symbols.scm` queries already capture imports.

**Spec reference:** Original brainstorming session positioned this as a stretch goal under "AI codebase memory" — answering "what depends on X" at sub-package granularity. The design spec doesn't explicitly mandate it; M10 is a usage-driven feature on top of M9.

**Baseline:** Branch off `main` at the M9 close commit. All gates green from M1-M9.

**M10 explicitly out of scope (deferred to M11+ or never):**
- **JS cross-project symbol refs** — no JS driver in scope; package.json `exports` maze.
- **Method/attribute-level resolution** — M10 resolves to module + top-level symbol granularity, not `obj.method_call`. Sufficient for the target use case (refactor impact analysis at the symbol level).
- **`pub use` re-export tracing in Rust** — if Maestro imports `atp_platform_sdk::Client` and atp-platform-sdk has `pub use crate::internal::Client`, the resolver lands on `Client` in the SDK crate but doesn't follow the re-export chain inside it. M11+.
- **Type signatures + docstrings** — still deferred from M9. Orthogonal milestone.
- **HTTP / REST runtime edges, WebSocket, offline bundle, Playwright, auth/TLS, mobile** — still deferred from M8.

---

## File Structure (created/modified in M10)

```
prograph/
├── prograph-core/
│   ├── src/
│   │   ├── lib.rs                                  # MODIFY — register new pyclasses + PyO3 wrappers
│   │   ├── facts.rs                                # MODIFY — ExternalImport (collected pre-resolve)
│   │   ├── models.rs                               # MODIFY — SymbolReference + InboundRefRow pyclasses
│   │   ├── store.rs                                # MODIFY — alive_symbol_refs, find_refs_to_*, describe_project ext
│   │   ├── indexer.rs                              # MODIFY — symbol-ref resolution + persistence
│   │   ├── parsers/
│   │   │   ├── python.rs                           # MODIFY — emit ALL imports, not internal-only
│   │   │   ├── rust.rs                             # MODIFY — emit ALL use_declarations
│   │   │   └── mod.rs                              # MODIFY — Module shape unchanged; new ExternalImport list per module
│   │   ├── resolvers/                              # NEW
│   │   │   ├── mod.rs                              # NEW — dispatch
│   │   │   ├── python.rs                           # NEW — dotted-name → publisher project + module path
│   │   │   └── rust.rs                             # NEW — crate name → project + symbol lookup
│   │   └── migrations/
│   │       └── v7.sql                              # NEW
├── prograph/
│   ├── _core.pyi                                   # MODIFY — stubs
│   ├── __init__.py                                 # MODIFY — re-exports
│   ├── models.py                                   # MODIFY — pydantic mirrors
│   ├── mcp_server.py                               # MODIFY — find_symbol_references tool
│   ├── web_app.py                                  # MODIFY — /api/symbol_refs endpoint
│   ├── export/render.py                            # MODIFY — "Inbound references" section
│   └── web_static/app.js                           # MODIFY — side panel: inbound refs list
├── tests/
│   ├── fixtures/
│   │   └── monorepo_symbol_refs/                   # NEW
│   ├── unit/
│   │   ├── test_resolver_python.py                 # NEW (pyrefly + pydantic-level)
│   │   └── test_resolver_rust.py                   # NEW
│   └── integration/
│       ├── test_symbol_refs_python.py              # NEW
│       ├── test_symbol_refs_rust.py                # NEW
│       └── test_mcp_find_symbol_references.py      # NEW (async)
```

No browser-UI breaking changes; new section added to side panel under existing structure.

---

## Task 1: Schema v7 — `cross_project_symbol_refs` table

**Files:**
- Create: `prograph-core/src/migrations/v7.sql`
- Modify: `prograph-core/src/store.rs`

- [ ] **Step 1: Write `v7.sql`**

`prograph-core/src/migrations/v7.sql`:
```sql
-- prograph schema v7 — cross-project symbol references.
-- Auxiliary data, NOT a new edge kind. One row per resolved import:line cite.
-- Indexed for both directions: refs FROM a project (M10's outbound list) and
-- refs TO a project (M10's inbound references — answers "who calls my symbol?").

CREATE TABLE IF NOT EXISTS cross_project_symbol_refs (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    from_project_id INTEGER NOT NULL REFERENCES projects(id),
    from_module_id  INTEGER NOT NULL REFERENCES modules(id),
    line            INTEGER NOT NULL,
    to_project_id   INTEGER NOT NULL REFERENCES projects(id),
    /// Module path inside the target project, e.g. "atp_platform.sdk" or "crate::policy".
    to_module_path  TEXT NOT NULL,
    /// Symbol name imported. NULL if the import was module-level only (e.g.
    /// `import atp_platform.sdk` rather than `from atp_platform.sdk import X`).
    to_symbol_name  TEXT,
    first_seen      INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen       INTEGER NOT NULL REFERENCES snapshots(id),
    UNIQUE(from_module_id, line, to_project_id, to_module_path, to_symbol_name)
);

CREATE INDEX IF NOT EXISTS idx_cpsr_last_seen  ON cross_project_symbol_refs(last_seen);
CREATE INDEX IF NOT EXISTS idx_cpsr_from_proj  ON cross_project_symbol_refs(from_project_id);
CREATE INDEX IF NOT EXISTS idx_cpsr_to_proj    ON cross_project_symbol_refs(to_project_id);
CREATE INDEX IF NOT EXISTS idx_cpsr_to_symbol  ON cross_project_symbol_refs(to_project_id, to_symbol_name);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (7, datetime('now'));
```

(Note: the `///` doc-style comment inside SQL is fine — SQLite treats `--` and `/* */` as comments; the `///` style above is just a label, not a SQL construct. Use `--` style for SQL inline comments.)

Replace `///` with `--` in the SQL:
```sql
    to_module_path  TEXT NOT NULL,
    -- Module path inside the target project, e.g. "atp_platform.sdk" or "crate::policy".
    to_symbol_name  TEXT,
    -- Symbol name imported. NULL if the import was module-level only (e.g.
    -- `import atp_platform.sdk` rather than `from atp_platform.sdk import X`).
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
    (6, include_str!("migrations/v6.sql")),
    (7, include_str!("migrations/v7.sql")),
];
```

- [ ] **Step 3: Test**

Append to `store.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn schema_v7_creates_symbol_refs_table() {
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
        assert!(names.contains(&"cross_project_symbol_refs".to_string()));
        assert_eq!(store.schema_version().unwrap(), 7);
    }
```

- [ ] **Step 4: Run + commit**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core store
```

```sh
git add prograph/prograph-core/src/migrations/v7.sql prograph/prograph-core/src/store.rs
git commit -m "prograph: M10 schema v7 — cross_project_symbol_refs table"
```

---

## Task 2: Facts — `ExternalImport` (un-filtered import collection)

**Files:**
- Modify: `prograph-core/src/facts.rs`
- Modify: `prograph-core/src/parsers/mod.rs`

M9 collected `InternalImport` (filtered by package prefix). M10 needs ALL imports per module so the indexer's resolver can decide which target each one points to. Add a parallel `ExternalImport` list — module gains a second field.

(Alternative considered: just unfilter `InternalImport` and rename. Rejected: would break the v6 `internal_imports` table semantics. Cleaner to add a NEW parser-side list that doesn't get persisted to `internal_imports`; only persisted to v7 if resolved to an in-monorepo target.)

- [ ] **Step 1: Add `ExternalImport`**

In `prograph-core/src/facts.rs`, append:
```rust
/// An import that the parser couldn't classify as definitely-internal. The indexer's
/// resolver layer (M10) decides whether each one points at an in-monorepo project,
/// at which point a `cross_project_symbol_refs` row is written. External-to-the-
/// monorepo imports (stdlib, PyPI, crates.io, npm) are silently dropped at resolve time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalImport {
    /// Dotted (Python) or `::`-separated (Rust) path. Always includes the leading
    /// segment (the "package name") — that's how the resolver locates the publisher.
    pub target_path: String,
    /// If the import explicitly named a symbol (`from x.y import Z`), this is `Z`.
    /// For module-only imports (`import x.y`), this is None.
    pub target_symbol: Option<String>,
    pub line: u32,
}
```

Extend `Module` to carry both:
```rust
pub struct Module {
    pub rel_path: String,
    pub language: String,
    #[serde(default)]
    pub public_symbols: Vec<PublicSymbol>,
    #[serde(default)]
    pub internal_imports: Vec<InternalImport>,
    /// M10: imports the parser couldn't classify as internal — resolver decides.
    #[serde(default)]
    pub external_imports: Vec<ExternalImport>,
}
```

- [ ] **Step 2: Add test**

In `facts.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn external_import_round_trips() {
        let e = ExternalImport {
            target_path: "atp_platform.sdk".into(),
            target_symbol: Some("MaestroATPAdapter".into()),
            line: 18,
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ExternalImport = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn module_back_compat_without_external_imports() {
        let json = r#"{
            "rel_path": "x.py",
            "language": "python"
        }"#;
        let m: Module = serde_json::from_str(json).unwrap();
        assert!(m.external_imports.is_empty());
    }
```

- [ ] **Step 3: Run + commit**

```sh
cargo test --package prograph-core facts
```

```sh
git add prograph/prograph-core/src/facts.rs
git commit -m "prograph: M10 facts — ExternalImport + Module.external_imports field"
```

---

## Task 3: Python parser — collect external imports

**Files:**
- Modify: `prograph-core/src/parsers/python.rs`

M9's `scan_python_modules` filtered `import_target` matches by package-prefix and emitted only `InternalImport`. M10 keeps the internal classification but ALSO emits every match as an `ExternalImport` (full target + symbol if extracted from `from ... import ...`).

The tree-sitter query in `python_symbols.scm` (M9) captures three import patterns:
- `import_simple` — `import foo.bar` → target_path="foo.bar", target_symbol=None
- `import_from` — `from foo.bar import x, y, z` → emit one external_import per imported symbol
- `import_from_relative` — `from .relative import x` → keep as internal_import only (relative imports stay inside the project by definition)

For pattern 2 (`import_from`), the existing query captures the `module_name` but NOT the imported names. We need to extend the query OR walk the tree to extract the `import_list` children.

Simplest: extend the query.

- [ ] **Step 1: Update `python_symbols.scm`**

In `prograph-core/src/ts_queries/python_symbols.scm`, replace the `import_from` pattern with one that captures the imported_name nodes too:

Replace:
```scheme
(import_from_statement
  module_name: (dotted_name) @import_target) @import_from
```

With:
```scheme
; `from foo.bar import x, y` — capture both module_name AND each imported name.
(import_from_statement
  module_name: (dotted_name) @import_target
  name: (dotted_name) @import_symbol) @import_from
```

(`dotted_name` is the type for both module and symbol — the field selectors `module_name:` and `name:` disambiguate.)

If `tree-sitter-python` doesn't expose `name:` as a field (versions vary), the fallback is to capture all `dotted_name` children of an `import_from_statement` and post-process in Rust. Test the query and pick the working form.

- [ ] **Step 2: Update `scan_python_modules` to emit `ExternalImport` from all import captures**

In `prograph-core/src/parsers/python.rs`, locate the per-match loop inside `scan_python_modules`. Replace the import-handling block (the `if is_import { ... }`) with:

```rust
            if is_import {
                if let Some(target) = import_target.clone() {
                    let is_relative = target.starts_with('.');
                    let is_internal = is_relative
                        || target == pkg_prefix
                        || target.starts_with(&format!("{pkg_prefix}."));

                    if is_internal {
                        internal_imports.push(crate::facts::InternalImport {
                            target_path: target.clone(),
                            line,
                        });
                    }

                    // M10: also emit ExternalImport for every import (resolver decides
                    // later whether it points at an in-monorepo project). Relative
                    // imports are never external by construction — skip them.
                    if !is_relative {
                        external_imports.push(crate::facts::ExternalImport {
                            target_path: target,
                            target_symbol: imported_symbol.clone(),
                            line,
                        });
                    }
                }
                continue;
            }
```

Add `import_symbol` capture handling in the same loop (alongside `import_target`):
```rust
                match cap_name.as_str() {
                    "symbol_name" => symbol_name = Some(text),
                    "import_target" => {
                        import_target = Some(text);
                        is_import = true;
                    }
                    "import_symbol" => {
                        imported_symbol = Some(text);
                    }
                    // ... existing arms ...
                }
```

Declare the variables at the top of the per-match block:
```rust
            let mut symbol_name: Option<String> = None;
            let mut import_target: Option<String> = None;
            let mut imported_symbol: Option<String> = None;
            let mut line: u32 = 1;
            let mut kind_hint: Option<crate::facts::SymbolKind> = None;
            let mut is_import = false;
```

Also declare `let mut external_imports: Vec<crate::facts::ExternalImport> = Vec::new();` at the per-module level (alongside `public_symbols` and `internal_imports`).

Sort external_imports for determinism:
```rust
        external_imports.sort_by(|a, b| (a.line, &a.target_path, &a.target_symbol).cmp(&(b.line, &b.target_path, &b.target_symbol)));
```

Include `external_imports` in the `Module` construction:
```rust
        modules.push(crate::facts::Module {
            rel_path,
            language: "python".into(),
            public_symbols,
            internal_imports,
            external_imports,
        });
```

- [ ] **Step 3: Test**

Append to `python.rs`'s tests:
```rust
    #[test]
    fn scans_external_imports_with_symbol() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("pyproject.toml"), r#"[project]
name = "consumer"
"#).unwrap();
        fs::write(dir.path().join("api.py"), r#"from atp_platform.sdk import MaestroATPAdapter, ToolClient
import requests
"#).unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "api.py").unwrap();

        let ext: Vec<_> = module.external_imports.iter().collect();
        // Two from-imports + one bare import = three external_imports total.
        // (`from x.y import a, b` produces 2 rows — one per symbol.)
        assert!(ext.len() >= 3, "expected ≥3 external imports, got {:?}", ext);

        let adapter = ext.iter().find(|e|
            e.target_path == "atp_platform.sdk" && e.target_symbol.as_deref() == Some("MaestroATPAdapter")
        );
        assert!(adapter.is_some(), "missing atp_platform.sdk::MaestroATPAdapter import");

        let requests_imp = ext.iter().find(|e|
            e.target_path == "requests" && e.target_symbol.is_none()
        );
        assert!(requests_imp.is_some(), "missing bare `import requests`");
    }

    #[test]
    fn relative_imports_dont_become_external() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("pyproject.toml"), r#"[project]
name = "consumer"
"#).unwrap();
        fs::write(dir.path().join("api.py"), r#"from .util import helper
"#).unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "api.py").unwrap();
        assert!(module.external_imports.is_empty(), "relative imports must NOT appear in external_imports");
    }
```

- [ ] **Step 4: Run + commit**

```sh
cargo test --package prograph-core parsers::python
```

```sh
git add prograph/prograph-core/src/parsers/python.rs prograph/prograph-core/src/ts_queries/python_symbols.scm
git commit -m "prograph: M10 Python parser collects ExternalImport (target_path + target_symbol per from-import)"
```

---

## Task 4: Rust parser — collect external `use` declarations

**Files:**
- Modify: `prograph-core/src/parsers/rust.rs`
- Modify: `prograph-core/src/ts_queries/rust_symbols.scm`

Similar to Python: M9's `rust_symbols.scm` filtered `use_declaration`s to those starting with `crate::`. M10 captures ALL `use` statements; the indexer decides which target crate they point at.

- [ ] **Step 1: Update `rust_symbols.scm`**

Replace the existing `use_declaration` captures (the M9 internal ones) with a more general capture that pulls every `use_declaration`, plus a separate capture marker that distinguishes the leading path segment for resolution:

```scheme
; All use_declarations. M10's indexer-side resolver decides which point at
; another in-monorepo project.
;
; Pattern 1: `use foo::bar::Baz;`
(use_declaration
  argument: (scoped_identifier
    path: (_) @use_root_path
    name: (identifier) @use_symbol)) @use_simple

; Pattern 2: `use foo::bar::{Baz, Qux};`
(use_declaration
  argument: (scoped_use_list
    path: (_) @use_root_path
    list: (use_list (use_as_clause? (_) @use_symbol)+))) @use_list
```

(Tree-sitter-rust grammar specifics: `path:` field returns the path before the final `::name`. The grammar may use `scoped_identifier` for simple uses and `scoped_use_list` for list uses. Verify by reading the grammar's node-types.json or by experimentation during implementation.)

If the grammar doesn't have these exact node names, fall back to:
```scheme
(use_declaration) @use_any
```
And parse the use_declaration's text in Rust post-processing.

- [ ] **Step 2: Update `scan_rust_modules`**

In `prograph-core/src/parsers/rust.rs`, replace the M9 use_declaration handling. Inside the per-match loop:

```rust
                    "use_simple" | "use_list" | "use_any" => {
                        // Get the full use_declaration text and parse it manually for
                        // robustness against grammar version drift.
                        if let Some(parent) = capture.node.parent() {
                            let raw = parent.utf8_text(source_bytes).unwrap_or("");
                            let cleaned = raw
                                .trim()
                                .trim_start_matches("use")
                                .trim()
                                .trim_end_matches(';')
                                .trim();
                            line = parent.start_position().row as u32 + 1;

                            // Parse a use path. Forms:
                            //   foo::bar::Baz                                 → external, symbol=Baz, root=foo
                            //   crate::foo::bar::Baz                          → internal, root=crate
                            //   foo::bar::{Baz, Qux}                          → external, multiple symbols
                            //   self::foo / super::foo                        → internal-ish, skip
                            let (root, rest) = match cleaned.split_once("::") {
                                Some((r, rest)) => (r.trim(), rest.trim()),
                                None => (cleaned, ""),
                            };
                            let root = root.to_string();

                            let is_internal = root == "crate" || root == "self" || root == "super";
                            if is_internal {
                                if root == "crate" {
                                    internal_imports.push(crate::facts::InternalImport {
                                        target_path: cleaned.to_string(),
                                        line,
                                    });
                                }
                                // self/super uses don't cross project boundaries — drop.
                            } else {
                                // External use. Emit one ExternalImport per symbol in the list,
                                // or one for the simple case.
                                let symbols = parse_use_list_symbols(rest);
                                if symbols.is_empty() {
                                    external_imports.push(crate::facts::ExternalImport {
                                        target_path: root.clone(),
                                        target_symbol: None,
                                        line,
                                    });
                                } else {
                                    for sym in symbols {
                                        external_imports.push(crate::facts::ExternalImport {
                                            target_path: root.clone(),
                                            target_symbol: Some(sym),
                                            line,
                                        });
                                    }
                                }
                            }
                        }
                    }
```

Add the helper `parse_use_list_symbols` at the bottom of `rust.rs`:
```rust
/// Given the tail of a `use` declaration after the root segment (e.g. `bar::Baz` or
/// `bar::{Baz, Qux}`), return the list of imported leaf symbol names.
///
/// Best-effort and conservative — only handles the common shapes.
fn parse_use_list_symbols(rest: &str) -> Vec<String> {
    if rest.is_empty() {
        return Vec::new();
    }
    // List form: `bar::{Baz, Qux}` → split on the inner braces.
    if let Some(brace_pos) = rest.find('{') {
        if let Some(end_brace) = rest.rfind('}') {
            if end_brace > brace_pos {
                let inside = &rest[brace_pos + 1..end_brace];
                return inside
                    .split(',')
                    .map(|s| s.trim().trim_end_matches(';').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }
    // Simple form: `bar::Baz` → take the last segment.
    rest.rsplit("::")
        .next()
        .map(|s| vec![s.trim().to_string()])
        .unwrap_or_default()
}
```

Declare `external_imports` at the per-module level in the existing loop. Add sort for determinism.

- [ ] **Step 3: Test**

```rust
    #[test]
    fn scans_external_use_with_symbol() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), r#"[package]
name = "c"
"#).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), r#"use atp_platform_sdk::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::policy::Decider;
use self::helper;
"#).unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "src/lib.rs").unwrap();
        let ext: Vec<_> = module.external_imports.iter().collect();

        let client = ext.iter().find(|e|
            e.target_path == "atp_platform_sdk" && e.target_symbol.as_deref() == Some("Client")
        );
        assert!(client.is_some(), "missing atp_platform_sdk::Client");

        let deser = ext.iter().find(|e|
            e.target_path == "serde" && e.target_symbol.as_deref() == Some("Deserialize")
        );
        let ser = ext.iter().find(|e|
            e.target_path == "serde" && e.target_symbol.as_deref() == Some("Serialize")
        );
        assert!(deser.is_some() && ser.is_some(), "serde list use should produce 2 entries");

        // self::, crate::, super:: don't become external.
        assert!(!ext.iter().any(|e| e.target_path == "crate" || e.target_path == "self"));
    }
```

- [ ] **Step 4: Run + commit**

```sh
cargo test --package prograph-core parsers::rust
```

```sh
git add prograph/prograph-core/src/parsers/rust.rs prograph/prograph-core/src/ts_queries/rust_symbols.scm
git commit -m "prograph: M10 Rust parser collects ExternalImport (parse_use_list_symbols handles {a,b} form)"
```

---

## Task 5: Python resolver — dotted-name → publisher project + module path

**Files:**
- Create: `prograph-core/src/resolvers/mod.rs`
- Create: `prograph-core/src/resolvers/python.rs`
- Modify: `prograph-core/src/lib.rs`

The resolver takes an `ExternalImport` + the global project list (with names + aliases from M3) + per-project module list (from M9) and decides:
- Which in-monorepo project is the publisher?
- What's the module path inside that project?
- (Best-effort) does the imported symbol exist in the publisher's `public_symbols`?

For Python: dash↔underscore normalisation on package name (`atp-platform` ↔ `atp_platform`), prefix match on the target_path.

- [ ] **Step 1: Write `resolvers/mod.rs`**

`prograph-core/src/resolvers/mod.rs`:
```rust
//! Resolver layer — turns parser-side `ExternalImport`s into `cross_project_symbol_refs` rows.
//!
//! Per language, the resolver answers: given an external import like
//! `atp_platform.sdk::MaestroATPAdapter`, which in-monorepo project (if any) is the publisher,
//! and what's the module path + symbol name inside that project?

pub mod python;
pub mod rust;

/// A resolved cross-project symbol reference, ready to be persisted into
/// `cross_project_symbol_refs`. The indexer fills in `from_module_id` + project ids
/// when writing the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRef {
    /// Index into `Vec<ProjectFacts>` for the source project.
    pub from_project_idx: usize,
    /// Path of the source module within the project (matches `Module.rel_path`).
    pub from_module_rel_path: String,
    pub from_line: u32,
    /// Index into `Vec<ProjectFacts>` for the resolved publisher project.
    pub to_project_idx: usize,
    /// Module path inside the publisher project — for Python this is the dotted target_path
    /// minus the publisher's package prefix (e.g. `atp_platform.sdk` → `sdk`). For Rust:
    /// `crate_name::a::b` → `a::b`. Always the path INSIDE the publisher.
    pub to_module_path: String,
    pub to_symbol_name: Option<String>,
}

/// Build a publisher-name → project_idx lookup from a slice of ProjectFacts. Honors
/// each project's Manifest.aliases AND applies dash→underscore normalisation for Python
/// (a project named "atp-platform" matches an import of "atp_platform").
pub(crate) fn build_publisher_index(
    facts: &[crate::facts::ProjectFacts],
) -> std::collections::HashMap<String, usize> {
    let mut out = std::collections::HashMap::new();
    for (idx, p) in facts.iter().enumerate() {
        let Some(m) = &p.manifest else { continue };

        let mut names: Vec<String> = Vec::new();
        names.push(m.declared_name.clone());
        names.extend(m.aliases.iter().cloned());

        // Python-style alternate: dashes → underscores.
        let underscored: Vec<String> = names
            .iter()
            .filter(|n| n.contains('-'))
            .map(|n| n.replace('-', "_"))
            .collect();
        names.extend(underscored);

        for name in names {
            out.entry(name).or_insert(idx);
        }
    }
    out
}
```

- [ ] **Step 2: Write `resolvers/python.rs`**

`prograph-core/src/resolvers/python.rs`:
```rust
//! Python resolver — dotted-name target → publisher project + sub-module path.

use std::collections::HashMap;

use super::ResolvedRef;
use crate::facts::ProjectFacts;

/// Resolve every external Python import in `facts` against the publisher index.
/// Returns one `ResolvedRef` per (line, target_path, target_symbol) that maps to
/// an in-monorepo publisher.
pub fn resolve(
    facts: &[ProjectFacts],
    publishers: &HashMap<String, usize>,
) -> Vec<ResolvedRef> {
    let mut out = Vec::new();

    for (from_idx, p) in facts.iter().enumerate() {
        for module in &p.modules {
            if module.language != "python" {
                continue;
            }
            for ext in &module.external_imports {
                let Some(to_idx) = resolve_dotted(&ext.target_path, publishers) else {
                    continue;
                };
                if to_idx == from_idx {
                    continue;  // self-reference shouldn't happen but guard anyway
                }
                // Compute the module path INSIDE the publisher:
                // `atp_platform.sdk` with publisher "atp_platform" or "atp-platform"
                // → "sdk".
                let to_module_path = strip_publisher_prefix(
                    &ext.target_path,
                    &facts[to_idx],
                );

                out.push(ResolvedRef {
                    from_project_idx: from_idx,
                    from_module_rel_path: module.rel_path.clone(),
                    from_line: ext.line,
                    to_project_idx: to_idx,
                    to_module_path,
                    to_symbol_name: ext.target_symbol.clone(),
                });
            }
        }
    }

    out.sort_by(|a, b| {
        (a.from_project_idx, &a.from_module_rel_path, a.from_line)
            .cmp(&(b.from_project_idx, &b.from_module_rel_path, b.from_line))
    });
    out
}

/// Find the publisher idx for an external import target. Splits the path on '.',
/// then looks up progressively shorter prefixes (longest-first) until a hit.
/// Handles `atp_platform.sdk.foo` → publisher "atp_platform".
fn resolve_dotted(target: &str, publishers: &HashMap<String, usize>) -> Option<usize> {
    let parts: Vec<&str> = target.split('.').collect();
    for prefix_len in (1..=parts.len()).rev() {
        let candidate = parts[..prefix_len].join(".");
        if let Some(&idx) = publishers.get(&candidate) {
            return Some(idx);
        }
        // Also try the joined-prefix-with-dashes form, in case the publisher's
        // declared name has dashes ("atp-platform-sdk") and the import has dots.
        let dashed = candidate.replace('_', "-");
        if let Some(&idx) = publishers.get(&dashed) {
            return Some(idx);
        }
    }
    None
}

/// Strip the publisher's package prefix from the dotted target path. The publisher
/// project's name might be "atp-platform" or "atp_platform" (M3 aliases handle the
/// dash↔underscore variants). Match the longest publisher name that's a dotted prefix
/// of `target`, then return the remainder.
fn strip_publisher_prefix(target: &str, publisher: &ProjectFacts) -> String {
    let Some(manifest) = &publisher.manifest else {
        return target.to_string();
    };
    let mut names: Vec<String> = Vec::new();
    names.push(manifest.declared_name.clone());
    names.extend(manifest.aliases.iter().cloned());
    names.push(manifest.declared_name.replace('-', "_"));
    names.extend(manifest.aliases.iter().map(|a| a.replace('-', "_")));
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));

    for name in &names {
        if target == name {
            return String::new();
        }
        let with_dot = format!("{}.", name);
        if let Some(rest) = target.strip_prefix(&with_dot) {
            return rest.to_string();
        }
    }
    target.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{DepRequirement, ExternalImport, Manifest, Module, ParseStatus};

    fn proj(name: &str, modules: Vec<Module>) -> ProjectFacts {
        ProjectFacts {
            project_root: format!("./{name}"),
            project_name: name.to_string(),
            manifest: Some(Manifest {
                declared_name: name.to_string(),
                version: None,
                declared_deps: vec![],
                aliases: vec![],
            }),
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules,
        }
    }

    fn module(rel_path: &str, externals: Vec<ExternalImport>) -> Module {
        Module {
            rel_path: rel_path.to_string(),
            language: "python".into(),
            public_symbols: vec![],
            internal_imports: vec![],
            external_imports: externals,
        }
    }

    #[test]
    fn resolves_dotted_to_publisher() {
        let facts = vec![
            proj("consumer", vec![module("api.py", vec![
                ExternalImport {
                    target_path: "atp_platform.sdk".into(),
                    target_symbol: Some("Client".into()),
                    line: 1,
                },
            ])]),
            proj("atp_platform", vec![]),
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].from_project_idx, 0);
        assert_eq!(refs[0].to_project_idx, 1);
        assert_eq!(refs[0].to_module_path, "sdk");
        assert_eq!(refs[0].to_symbol_name.as_deref(), Some("Client"));
    }

    #[test]
    fn resolves_via_alias_with_dash_underscore() {
        let mut atp = proj("atp-platform", vec![]);
        if let Some(m) = &mut atp.manifest {
            m.aliases.push("atp-platform-sdk".to_string());
        }
        let facts = vec![
            proj("consumer", vec![module("api.py", vec![
                ExternalImport {
                    target_path: "atp_platform_sdk.client".into(),
                    target_symbol: None,
                    line: 5,
                },
            ])]),
            atp,
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].to_module_path, "client");
    }

    #[test]
    fn drops_stdlib_imports() {
        let facts = vec![
            proj("consumer", vec![module("api.py", vec![
                ExternalImport {
                    target_path: "os.path".into(),
                    target_symbol: Some("join".into()),
                    line: 1,
                },
            ])]),
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert!(refs.is_empty());
    }

    #[test]
    fn module_path_is_empty_when_import_targets_top_level() {
        let facts = vec![
            proj("consumer", vec![module("api.py", vec![
                ExternalImport {
                    target_path: "atp_platform".into(),
                    target_symbol: Some("Client".into()),
                    line: 1,
                },
            ])]),
            proj("atp_platform", vec![]),
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].to_module_path, "");  // top-level import
    }
}
```

- [ ] **Step 3: Register the module**

In `prograph-core/src/lib.rs`, add `mod resolvers;` alongside the other module declarations (alphabetical):
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
mod resolvers;  // NEW
mod store;
```

- [ ] **Step 4: Run + commit**

```sh
cargo test --package prograph-core resolvers
```
Expected: 4 passed.

```sh
git add prograph/prograph-core/src/resolvers/mod.rs prograph/prograph-core/src/resolvers/python.rs \
        prograph/prograph-core/src/lib.rs
git commit -m "prograph: M10 Python resolver — dotted-name → publisher project + sub-module path"
```

---

## Task 6: Rust resolver — crate name → publisher project + symbol-or-module path

**Files:**
- Create: `prograph-core/src/resolvers/rust.rs`

Rust resolution is symmetric to Python but uses `::` separators and resolves the root segment (crate name) against `Manifest.declared_name`. Crate names in Cargo follow snake_case convention (`atp_platform_sdk`), matching against the publisher's declared name.

- [ ] **Step 1: Write `resolvers/rust.rs`**

`prograph-core/src/resolvers/rust.rs`:
```rust
//! Rust resolver — `use foo::a::b::Baz` → publisher project "foo" + inside-crate path "a::b" + symbol "Baz".

use std::collections::HashMap;

use super::ResolvedRef;
use crate::facts::ProjectFacts;

pub fn resolve(
    facts: &[ProjectFacts],
    publishers: &HashMap<String, usize>,
) -> Vec<ResolvedRef> {
    let mut out = Vec::new();

    for (from_idx, p) in facts.iter().enumerate() {
        for module in &p.modules {
            if module.language != "rust" {
                continue;
            }
            for ext in &module.external_imports {
                // Root segment is the crate name.
                let Some((root, rest)) = ext.target_path.split_once("::") else {
                    // Bare `use foo;` — root IS the whole target_path.
                    if let Some(&to_idx) = publishers.get(ext.target_path.as_str()) {
                        if to_idx != from_idx {
                            out.push(ResolvedRef {
                                from_project_idx: from_idx,
                                from_module_rel_path: module.rel_path.clone(),
                                from_line: ext.line,
                                to_project_idx: to_idx,
                                to_module_path: String::new(),
                                to_symbol_name: ext.target_symbol.clone(),
                            });
                        }
                    }
                    continue;
                };

                let Some(&to_idx) = publishers.get(root) else {
                    continue;
                };
                if to_idx == from_idx {
                    continue;
                }

                // For Rust, `rest` is the path inside the crate. If target_symbol
                // is set (e.g. from `use foo::a::b::{Baz}`), then `rest` already
                // contains the symbol — peel it off so to_module_path is the
                // containing module, not the symbol itself.
                let to_module_path = if ext.target_symbol.is_some() {
                    rest.rsplit_once("::")
                        .map(|(prefix, _last)| prefix.to_string())
                        .unwrap_or_else(|| String::new())
                } else {
                    rest.to_string()
                };

                out.push(ResolvedRef {
                    from_project_idx: from_idx,
                    from_module_rel_path: module.rel_path.clone(),
                    from_line: ext.line,
                    to_project_idx: to_idx,
                    to_module_path,
                    to_symbol_name: ext.target_symbol.clone(),
                });
            }
        }
    }

    out.sort_by(|a, b| {
        (a.from_project_idx, &a.from_module_rel_path, a.from_line)
            .cmp(&(b.from_project_idx, &b.from_module_rel_path, b.from_line))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{ExternalImport, Manifest, Module, ParseStatus};

    fn proj(name: &str, modules: Vec<Module>) -> ProjectFacts {
        ProjectFacts {
            project_root: format!("./{name}"),
            project_name: name.to_string(),
            manifest: Some(Manifest {
                declared_name: name.to_string(),
                version: None,
                declared_deps: vec![],
                aliases: vec![],
            }),
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules,
        }
    }

    fn module(rel_path: &str, externals: Vec<ExternalImport>) -> Module {
        Module {
            rel_path: rel_path.to_string(),
            language: "rust".into(),
            public_symbols: vec![],
            internal_imports: vec![],
            external_imports: externals,
        }
    }

    #[test]
    fn resolves_rust_use_via_crate_name() {
        let facts = vec![
            proj("consumer", vec![module("src/lib.rs", vec![
                ExternalImport {
                    target_path: "atp_platform_sdk::client".into(),
                    target_symbol: Some("Client".into()),
                    line: 3,
                },
            ])]),
            proj("atp_platform_sdk", vec![]),
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].to_module_path, "");  // symbol stripped off (only one segment after crate)
        assert_eq!(refs[0].to_symbol_name.as_deref(), Some("Client"));
    }

    #[test]
    fn module_path_strips_trailing_symbol() {
        let facts = vec![
            proj("consumer", vec![module("src/lib.rs", vec![
                ExternalImport {
                    target_path: "atp_platform_sdk::api::v2::Client".into(),
                    target_symbol: Some("Client".into()),
                    line: 1,
                },
            ])]),
            proj("atp_platform_sdk", vec![]),
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert_eq!(refs[0].to_module_path, "api::v2");
    }

    #[test]
    fn drops_stdlib_uses() {
        let facts = vec![
            proj("consumer", vec![module("src/lib.rs", vec![
                ExternalImport {
                    target_path: "std::collections".into(),
                    target_symbol: Some("HashMap".into()),
                    line: 1,
                },
            ])]),
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert!(refs.is_empty());
    }
}
```

- [ ] **Step 2: Register in `resolvers/mod.rs`**

In `prograph-core/src/resolvers/mod.rs`, ensure `pub mod rust;` is declared at the top (it is in the Task 5 listing).

- [ ] **Step 3: Run + commit**

```sh
cargo test --package prograph-core resolvers
```
Expected: 7 passed (4 python + 3 rust).

```sh
git add prograph/prograph-core/src/resolvers/rust.rs
git commit -m "prograph: M10 Rust resolver — use foo::a::b::Baz → publisher 'foo' + inside-crate path 'a::b'"
```

---

## Task 7: Indexer persists `cross_project_symbol_refs`

**Files:**
- Modify: `prograph-core/src/indexer.rs`
- Modify: `prograph-core/src/store.rs`

After all projects + modules are persisted in the snapshot, run the resolvers, then write rows into `cross_project_symbol_refs`. Each row references the previously-inserted `module_id` (from the M9 modules persist phase).

- [ ] **Step 1: Add `SnapshotWriter::insert_symbol_ref`**

In `prograph-core/src/store.rs`, append to `impl<'a> SnapshotWriter<'a>`:
```rust
    pub fn insert_symbol_ref(
        &self,
        snapshot_id: i64,
        from_project_id: i64,
        from_module_id: i64,
        line: i64,
        to_project_id: i64,
        to_module_path: &str,
        to_symbol_name: Option<&str>,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO cross_project_symbol_refs
             (from_project_id, from_module_id, line, to_project_id, to_module_path, to_symbol_name, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?, ?,
                     COALESCE((SELECT first_seen FROM cross_project_symbol_refs
                               WHERE from_module_id=? AND line=? AND to_project_id=? AND to_module_path=?
                                 AND COALESCE(to_symbol_name, '') = COALESCE(?, '')), ?),
                     ?)",
            rusqlite::params![
                from_project_id, from_module_id, line, to_project_id, to_module_path, to_symbol_name,
                from_module_id, line, to_project_id, to_module_path, to_symbol_name, snapshot_id,
                snapshot_id
            ],
        )?;
        Ok(())
    }
```

- [ ] **Step 2: Hook resolver into the indexer**

In `prograph-core/src/indexer.rs`, locate the section that persists modules (M9 — inside the project persist loop). After the entire project persist loop completes (so all modules across all projects have been inserted and their ids are known), add a new resolver pass:

```rust
    // M10: resolve cross-project symbol references AFTER all modules are persisted.
    let publishers = crate::resolvers::build_publisher_index(&facts);
    let mut all_refs: Vec<crate::resolvers::ResolvedRef> = Vec::new();
    all_refs.extend(crate::resolvers::python::resolve(&facts, &publishers));
    all_refs.extend(crate::resolvers::rust::resolve(&facts, &publishers));

    // Look up (project_idx, rel_path) → module_id from the per-project module ids.
    // We need a fresh query against the DB since `insert_module` was called within
    // this transaction — re-query via the writer's connection.
    let module_id_for: std::collections::HashMap<(usize, String), i64> = {
        let mut m = std::collections::HashMap::new();
        for (idx, fact) in facts.iter().enumerate() {
            for module in &fact.modules {
                // Re-query the module's id. The module was just inserted via
                // `writer.insert_module` in the project persist loop. Use a direct
                // SQL query (read-only inside the same transaction).
                let from_project_id = match new_project_ids.get(&fact.project_root) {
                    Some(&pid) => pid,
                    None => continue,
                };
                let mid: Option<i64> = writer
                    .conn()
                    .query_row(
                        "SELECT id FROM modules WHERE project_id = ? AND rel_path = ?",
                        rusqlite::params![from_project_id, module.rel_path],
                        |r| r.get(0),
                    )
                    .ok();
                if let Some(mid) = mid {
                    m.insert((idx, module.rel_path.clone()), mid);
                }
            }
        }
        m
    };

    for r in &all_refs {
        let from_pid = facts
            .get(r.from_project_idx)
            .and_then(|f| new_project_ids.get(&f.project_root))
            .copied();
        let to_pid = facts
            .get(r.to_project_idx)
            .and_then(|f| new_project_ids.get(&f.project_root))
            .copied();
        let from_mid = module_id_for.get(&(r.from_project_idx, r.from_module_rel_path.clone())).copied();

        if let (Some(from_pid), Some(to_pid), Some(from_mid)) = (from_pid, to_pid, from_mid) {
            writer.insert_symbol_ref(
                snap_id,
                from_pid,
                from_mid,
                r.from_line as i64,
                to_pid,
                &r.to_module_path,
                r.to_symbol_name.as_deref(),
            )?;
        }
    }
```

Note: `writer.conn()` doesn't exist yet — add a thin accessor to `SnapshotWriter`:
```rust
    /// Read-only access to the transaction's underlying connection. Use only for
    /// lookup queries within the same transaction; do NOT use for writes outside
    /// the writer's methods.
    pub(crate) fn conn(&self) -> &rusqlite::Connection {
        &self.tx
    }
```

(`rusqlite::Transaction` derefs to `Connection` automatically — the explicit accessor exists for clarity.)

- [ ] **Step 3: Test**

In `prograph-core/src/indexer.rs`'s `#[cfg(test)] mod tests`, append:
```rust
    #[test]
    fn symbol_refs_persist_across_projects() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();

        // Publisher project: atp_platform with a sdk module.
        fs::create_dir_all(dir.path().join("atp_platform/atp_platform/sdk")).unwrap();
        fs::write(dir.path().join("atp_platform/pyproject.toml"), r#"[project]
name = "atp_platform"
"#).unwrap();
        fs::write(dir.path().join("atp_platform/atp_platform/__init__.py"), "").unwrap();
        fs::write(dir.path().join("atp_platform/atp_platform/sdk/__init__.py"), r#"class Client:
    pass
"#).unwrap();

        // Consumer project: uses atp_platform.sdk.Client.
        fs::create_dir_all(dir.path().join("consumer/consumer")).unwrap();
        fs::write(dir.path().join("consumer/pyproject.toml"), r#"[project]
name = "consumer"
"#).unwrap();
        fs::write(dir.path().join("consumer/consumer/__init__.py"), "").unwrap();
        fs::write(dir.path().join("consumer/consumer/api.py"), r#"from atp_platform.sdk import Client
"#).unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        let n_refs: i64 = store.connection().query_row(
            "SELECT COUNT(*) FROM cross_project_symbol_refs WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
            [], |r| r.get(0),
        ).unwrap();
        assert!(n_refs >= 1, "expected ≥1 symbol ref persisted, got {}", n_refs);

        let target: (String, String, Option<String>) = store.connection().query_row(
            "SELECT p2.name, ref.to_module_path, ref.to_symbol_name
             FROM cross_project_symbol_refs ref
             JOIN projects p2 ON p2.id = ref.to_project_id
             WHERE ref.last_seen = (SELECT MAX(id) FROM snapshots)
             LIMIT 1",
            [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ).unwrap();
        assert_eq!(target.0, "atp_platform");
        assert_eq!(target.1, "sdk");
        assert_eq!(target.2.as_deref(), Some("Client"));
    }
```

- [ ] **Step 4: Run + commit**

```sh
cargo test --package prograph-core
```

```sh
git add prograph/prograph-core/src/indexer.rs prograph/prograph-core/src/store.rs
git commit -m "prograph: M10 indexer — resolve external imports + persist cross_project_symbol_refs"
```

---

## Task 8: Store queries + PyO3 wrappers

**Files:**
- Modify: `prograph-core/src/store.rs`
- Modify: `prograph-core/src/models.rs`
- Modify: `prograph-core/src/lib.rs`
- Modify: `prograph/_core.pyi`

Two query helpers + their pyclass + pydantic mirrors:
- `Store::refs_to_symbol(project_name, symbol_name?) → Vec<SymbolRefRow>` — answers "who calls my symbol?"
- `Store::refs_from_project(project_name) → Vec<SymbolRefRow>` — answers "who do I call?"

- [ ] **Step 1: Add `SymbolRefRow` pyclass**

In `prograph-core/src/models.rs`, append:
```rust
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct SymbolRefRow {
    pub from_project_name: String,
    pub from_module_rel_path: String,
    pub line: i64,
    pub to_project_name: String,
    pub to_module_path: String,
    pub to_symbol_name: Option<String>,
}

#[pymethods]
impl SymbolRefRow {
    fn __repr__(&self) -> String {
        format!(
            "SymbolRefRow({}/{}:{} → {}::{}::{:?})",
            self.from_project_name, self.from_module_rel_path, self.line,
            self.to_project_name, self.to_module_path, self.to_symbol_name
        )
    }
}
```

Extend `pub use models::{...}` to include `SymbolRefRow`. Register inside `#[pymodule]`:
```rust
    m.add_class::<SymbolRefRow>()?;
```

- [ ] **Step 2: Add the Store queries**

Append to `impl Store`:
```rust
    /// Return symbol refs pointing AT a project (optionally filtered by symbol name).
    /// Answers "who imports my X?"
    pub fn refs_to_symbol(
        &self,
        project_name: &str,
        symbol_name: Option<&str>,
    ) -> Result<Vec<crate::models::SymbolRefRow>> {
        let mut sql = String::from(
            "SELECT p1.name, m.rel_path, ref.line, p2.name, ref.to_module_path, ref.to_symbol_name
             FROM cross_project_symbol_refs ref
             JOIN projects p1 ON p1.id = ref.from_project_id
             JOIN modules m ON m.id = ref.from_module_id
             JOIN projects p2 ON p2.id = ref.to_project_id
             WHERE p2.name = ? AND ref.last_seen = (SELECT MAX(id) FROM snapshots)",
        );
        if symbol_name.is_some() {
            sql.push_str(" AND ref.to_symbol_name = ?");
        }
        sql.push_str(" ORDER BY p1.name, m.rel_path, ref.line");

        let mut stmt = self.conn.prepare(&sql)?;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(project_name.to_string())];
        if let Some(s) = symbol_name {
            params.push(Box::new(s.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

        let rows = stmt.query_map(param_refs.as_slice(), |r| {
            Ok(crate::models::SymbolRefRow {
                from_project_name: r.get(0)?,
                from_module_rel_path: r.get(1)?,
                line: r.get(2)?,
                to_project_name: r.get(3)?,
                to_module_path: r.get(4)?,
                to_symbol_name: r.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// Return symbol refs originating FROM a project. Answers "who do I import from?"
    pub fn refs_from_project(
        &self,
        project_name: &str,
    ) -> Result<Vec<crate::models::SymbolRefRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT p1.name, m.rel_path, ref.line, p2.name, ref.to_module_path, ref.to_symbol_name
             FROM cross_project_symbol_refs ref
             JOIN projects p1 ON p1.id = ref.from_project_id
             JOIN modules m ON m.id = ref.from_module_id
             JOIN projects p2 ON p2.id = ref.to_project_id
             WHERE p1.name = ? AND ref.last_seen = (SELECT MAX(id) FROM snapshots)
             ORDER BY p2.name, ref.to_module_path, m.rel_path, ref.line",
        )?;
        let rows = stmt.query_map(rusqlite::params![project_name], |r| {
            Ok(crate::models::SymbolRefRow {
                from_project_name: r.get(0)?,
                from_module_rel_path: r.get(1)?,
                line: r.get(2)?,
                to_project_name: r.get(3)?,
                to_module_path: r.get(4)?,
                to_symbol_name: r.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }
```

- [ ] **Step 3: PyO3 wrappers + .pyi**

In `prograph-core/src/lib.rs`:
```rust
#[pyfunction]
#[pyo3(name = "refs_to_symbol")]
fn py_refs_to_symbol(
    db_path: &str,
    project_name: &str,
    symbol_name: Option<&str>,
) -> PyResult<Vec<SymbolRefRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.refs_to_symbol(project_name, symbol_name)?)
}

#[pyfunction]
#[pyo3(name = "refs_from_project")]
fn py_refs_from_project(
    db_path: &str,
    project_name: &str,
) -> PyResult<Vec<SymbolRefRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.refs_from_project(project_name)?)
}
```

Register both:
```rust
    m.add_function(wrap_pyfunction!(py_refs_to_symbol, m)?)?;
    m.add_function(wrap_pyfunction!(py_refs_from_project, m)?)?;
```

Extend `prograph/_core.pyi`:
```python
def refs_to_symbol(db_path: str, project_name: str, symbol_name: str | None = None) -> list[SymbolRefRow]: ...
def refs_from_project(db_path: str, project_name: str) -> list[SymbolRefRow]: ...

class SymbolRefRow:
    from_project_name: str
    from_module_rel_path: str
    line: int
    to_project_name: str
    to_module_path: str
    to_symbol_name: str | None
```

- [ ] **Step 4: Pydantic mirror**

In `prograph/models.py`, append:
```python
class SymbolRefRow(BaseModel):
    model_config = ConfigDict(frozen=True)
    from_project_name: str
    from_module_rel_path: str
    line: int
    to_project_name: str
    to_module_path: str
    to_symbol_name: str | None

    @classmethod
    def from_core(cls, value: _core.SymbolRefRow) -> SymbolRefRow:
        return cls(
            from_project_name=value.from_project_name,
            from_module_rel_path=value.from_module_rel_path,
            line=value.line,
            to_project_name=value.to_project_name,
            to_module_path=value.to_module_path,
            to_symbol_name=value.to_symbol_name,
        )
```

Update `prograph/__init__.py` re-exports.

- [ ] **Step 5: Run + commit**

```sh
uv sync --reinstall-package prograph
cargo test --package prograph-core
uv run pytest -v
```

```sh
git add prograph/prograph-core/src/store.rs prograph/prograph-core/src/models.rs \
        prograph/prograph-core/src/lib.rs \
        prograph/prograph/_core.pyi prograph/prograph/models.py prograph/prograph/__init__.py
git commit -m "prograph: M10 Store::refs_to_symbol + refs_from_project + SymbolRefRow pyclass/pydantic"
```

---

## Task 9: New MCP tool — `find_symbol_references`

**Files:**
- Modify: `prograph/mcp_server.py`

A new MCP tool that AI agents call to answer "who uses my X?". The tool returns SymbolRefRow JSON.

- [ ] **Step 1: Add the dispatch**

In `prograph/mcp_server.py`, in `_dispatch`:
```python
    if name == "find_symbol_references":
        project_name = args.get("project_name")
        if not project_name or not isinstance(project_name, str):
            return {"error": "missing required string arg 'project_name'"}
        symbol_name = args.get("symbol_name")
        if symbol_name is not None and not isinstance(symbol_name, str):
            return {"error": "'symbol_name' must be a string when present"}
        direction = args.get("direction", "inbound")  # "inbound" | "outbound"

        from prograph.models import SymbolRefRow

        if direction == "inbound":
            rows = _core.refs_to_symbol(db_path, project_name, symbol_name)
        elif direction == "outbound":
            if symbol_name is not None:
                return {"error": "symbol_name filter not supported for direction=outbound (yet)"}
            rows = _core.refs_from_project(db_path, project_name)
        else:
            return {"error": f"invalid direction: {direction} (use 'inbound' or 'outbound')"}

        return [SymbolRefRow.from_core(r).model_dump(mode="json") for r in rows]
```

In `_tool_definitions`:
```python
        Tool(
            name="find_symbol_references",
            description=(
                "Find cross-project source-line references. 'inbound' (default): who imports "
                "the given project's symbols (optionally filtered by symbol_name). 'outbound': "
                "which other projects this project imports from. Returns SymbolRefRow records "
                "with from_module_rel_path + line + to_module_path + to_symbol_name."
            ),
            inputSchema={
                "type": "object",
                "properties": {
                    "project_name": {
                        "type": "string",
                        "description": "Target project name.",
                    },
                    "symbol_name": {
                        "type": "string",
                        "description": "Optional symbol filter (inbound only).",
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["inbound", "outbound"],
                        "default": "inbound",
                    },
                },
                "required": ["project_name"],
            },
        ),
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/mcp_server.py
git commit -m "prograph: M10 MCP tool — find_symbol_references (inbound/outbound)"
```

---

## Task 10: REST endpoint + `describe_project` enrichment

**Files:**
- Modify: `prograph/web_app.py`
- Modify: `prograph-core/src/store.rs` (`describe_project` adds two new fields)
- Modify: `prograph-core/src/models.rs`
- Modify: `prograph/models.py`

The browser UI needs inbound refs in `ProjectDescription` so the side panel can render them. Extend M5's `describe_project` aggregation with two new list fields: `inbound_refs` + `outbound_refs`.

Additionally expose a dedicated `/api/symbol_refs` endpoint for ad-hoc queries.

- [ ] **Step 1: Extend `ProjectDescription` pyclass**

In `prograph-core/src/models.rs`'s `ProjectDescription`, add two new fields:
```rust
pub struct ProjectDescription {
    // ... existing fields ...
    pub modules: Vec<ModuleRow>,
    pub public_symbols: Vec<PublicSymbolRow>,
    pub internal_imports: Vec<InternalImportRow>,
    /// M10: cross-project refs INTO this project.
    pub inbound_refs: Vec<SymbolRefRow>,
    /// M10: cross-project refs FROM this project.
    pub outbound_refs: Vec<SymbolRefRow>,
}
```

In `Store::describe_project` (in `store.rs`), append two queries before the final struct construction:
```rust
        let inbound_refs = self.refs_to_symbol(&name, None)?;
        let outbound_refs = self.refs_from_project(&name)?;
```

Include them in the `ProjectDescription { ... }` literal.

- [ ] **Step 2: Update `.pyi` + pydantic mirror**

`prograph/_core.pyi` — `ProjectDescription`:
```python
class ProjectDescription:
    # ... existing ...
    inbound_refs: list[SymbolRefRow]
    outbound_refs: list[SymbolRefRow]
```

`prograph/models.py` — extend `ProjectDescription.from_core`:
```python
            inbound_refs=[SymbolRefRow.from_core(r) for r in value.inbound_refs],
            outbound_refs=[SymbolRefRow.from_core(r) for r in value.outbound_refs],
```

And add the fields to the pydantic model.

- [ ] **Step 3: REST endpoint**

In `prograph/web_app.py`, append:
```python
    @app.get("/api/symbol_refs")
    async def symbol_refs(
        project: str,
        symbol: str | None = None,
        direction: str = "inbound",
    ) -> list[dict]:
        from prograph import _core
        from prograph.models import SymbolRefRow

        if direction == "inbound":
            rows = _core.refs_to_symbol(app.state.db_path, project, symbol)
        elif direction == "outbound":
            if symbol is not None:
                raise HTTPException(status_code=400, detail="symbol filter not supported for direction=outbound")
            rows = _core.refs_from_project(app.state.db_path, project)
        else:
            raise HTTPException(status_code=400, detail=f"invalid direction: {direction}")

        return [SymbolRefRow.from_core(r).model_dump(mode="json") for r in rows]
```

- [ ] **Step 4: Run + commit**

```sh
uv sync --reinstall-package prograph
cargo test --package prograph-core
uv run pytest -v
```

```sh
git add prograph/prograph-core/src/models.rs prograph/prograph-core/src/store.rs \
        prograph/prograph/_core.pyi prograph/prograph/models.py prograph/prograph/web_app.py
git commit -m "prograph: M10 ProjectDescription gains inbound_refs/outbound_refs + GET /api/symbol_refs"
```

---

## Task 11: MD render + browser UI side panel

**Files:**
- Modify: `prograph/export/render.py`
- Modify: `prograph/web_static/app.js`

The MD project card gets two new sections: "Inbound references" (other projects importing this one's symbols) and "Outbound references" (this project's imports of other in-monorepo projects). The browser side panel mirrors them.

- [ ] **Step 1: MD renderer**

In `prograph/export/render.py`, locate `render_project`. After the existing "## Modules" section, append:

```python
    lines.append("## Inbound references")
    lines.append("")
    if desc.inbound_refs:
        # Group by from_project_name → list.
        from collections import defaultdict
        grouped: dict[str, list] = defaultdict(list)
        for r in desc.inbound_refs:
            grouped[r.from_project_name].append(r)
        for project, refs in sorted(grouped.items()):
            lines.append(f"- from [[{project}]]:")
            for r in refs:
                sym = r.to_symbol_name or "(module)"
                target = f"{r.to_module_path}::{sym}" if r.to_module_path else sym
                lines.append(f"  - `{r.from_module_rel_path}:{r.line}` → `{target}`")
    else:
        lines.append("_None._")
    lines.append("")

    lines.append("## Outbound references")
    lines.append("")
    if desc.outbound_refs:
        from collections import defaultdict
        grouped_out: dict[str, list] = defaultdict(list)
        for r in desc.outbound_refs:
            grouped_out[r.to_project_name].append(r)
        for project, refs in sorted(grouped_out.items()):
            lines.append(f"- to [[{project}]]:")
            for r in refs:
                sym = r.to_symbol_name or "(module)"
                target = f"{r.to_module_path}::{sym}" if r.to_module_path else sym
                lines.append(f"  - `{r.from_module_rel_path}:{r.line}` → `{target}`")
    else:
        lines.append("_None._")
    lines.append("")
```

Regenerate goldens:
```sh
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py
```

Inspect the diffs.

- [ ] **Step 2: Browser UI**

In `prograph/web_static/app.js`, in `renderProject(p)`, after the existing "Modules" block, append:

```javascript
    if (p.inbound_refs && p.inbound_refs.length) {
        nodes.push(el('h3', {}, ['Inbound references']));
        const items = p.inbound_refs.slice(0, 50).map((r) => {
            const sym = r.to_symbol_name || '(module)';
            const target = r.to_module_path ? `${r.to_module_path}::${sym}` : sym;
            return el('li', {}, [
                'from ',
                el('strong', {}, [r.from_project_name]),
                ' — ',
                el('code', {}, [`${r.from_module_rel_path}:${r.line}`]),
                ' → ',
                el('code', {}, [target]),
            ]);
        });
        nodes.push(el('ul', {}, items));
        if (p.inbound_refs.length > 50) {
            nodes.push(el('p', {}, [el('em', {}, [`(${p.inbound_refs.length - 50} more)`])]));
        }
    }
    if (p.outbound_refs && p.outbound_refs.length) {
        nodes.push(el('h3', {}, ['Outbound references']));
        const items = p.outbound_refs.slice(0, 50).map((r) => {
            const sym = r.to_symbol_name || '(module)';
            const target = r.to_module_path ? `${r.to_module_path}::${sym}` : sym;
            return el('li', {}, [
                'to ',
                el('strong', {}, [r.to_project_name]),
                ' — ',
                el('code', {}, [`${r.from_module_rel_path}:${r.line}`]),
                ' → ',
                el('code', {}, [target]),
            ]);
        });
        nodes.push(el('ul', {}, items));
    }
```

Add a static-asset test in `tests/unit/test_web_static.py`:
```python
def test_app_js_renders_inbound_outbound_refs():
    js = (STATIC_DIR / "app.js").read_text()
    assert "Inbound references" in js
    assert "Outbound references" in js
    assert "inbound_refs" in js
    assert "outbound_refs" in js
```

- [ ] **Step 3: Run + commit**

```sh
uv run pytest -v
```

```sh
git add prograph/prograph/export/render.py prograph/prograph/web_static/app.js \
        prograph/tests/unit/test_web_static.py \
        prograph/tests/fixtures/monorepo_full/golden/ \
        prograph/tests/fixtures/monorepo_multilang/golden/ \
        prograph/tests/fixtures/monorepo_mcp/golden/
git commit -m "prograph: M10 MD + browser UI — Inbound/Outbound references sections"
```

---

## Task 12: `monorepo_symbol_refs` fixture + integration tests

**Files:**
- Create: `tests/fixtures/monorepo_symbol_refs/` (~10 files)
- Create: `tests/integration/test_symbol_refs_python.py`
- Create: `tests/integration/test_symbol_refs_rust.py`

A focused fixture with two Python projects + one cross-project import, plus two Rust crates with a similar cross.

- [ ] **Step 1: Python sub-fixture**

`tests/fixtures/monorepo_symbol_refs/py_sdk/pyproject.toml`:
```toml
[project]
name = "py_sdk"
version = "0.1.0"
```

`tests/fixtures/monorepo_symbol_refs/py_sdk/py_sdk/__init__.py`: (empty)

`tests/fixtures/monorepo_symbol_refs/py_sdk/py_sdk/client.py`:
```python
class Client:
    def fetch(self):
        return None

class AdminClient(Client):
    pass

def helper():
    return 1
```

`tests/fixtures/monorepo_symbol_refs/py_consumer/pyproject.toml`:
```toml
[project]
name = "py_consumer"
version = "0.1.0"
```

`tests/fixtures/monorepo_symbol_refs/py_consumer/py_consumer/__init__.py`: (empty)

`tests/fixtures/monorepo_symbol_refs/py_consumer/py_consumer/uses.py`:
```python
from py_sdk.client import Client, AdminClient
from py_sdk.client import helper as h

def main():
    c = Client()
    a = AdminClient()
    return h()
```

- [ ] **Step 2: Rust sub-fixture**

`tests/fixtures/monorepo_symbol_refs/rust_sdk/Cargo.toml`:
```toml
[package]
name = "rust_sdk"
version = "0.1.0"
edition = "2021"
```

`tests/fixtures/monorepo_symbol_refs/rust_sdk/src/lib.rs`:
```rust
pub mod client;
```

`tests/fixtures/monorepo_symbol_refs/rust_sdk/src/client.rs`:
```rust
pub struct Client;

impl Client {
    pub fn new() -> Self { Self }
}

pub fn build() -> Client { Client }
```

`tests/fixtures/monorepo_symbol_refs/rust_consumer/Cargo.toml`:
```toml
[package]
name = "rust_consumer"
version = "0.1.0"
edition = "2021"
```

`tests/fixtures/monorepo_symbol_refs/rust_consumer/src/lib.rs`:
```rust
use rust_sdk::client::Client;
use rust_sdk::client::build;

pub fn make() -> Client {
    build()
}
```

- [ ] **Step 3: Python integration test**

`tests/integration/test_symbol_refs_python.py`:
```python
"""M10: Python cross-project symbol refs persist + are queryable."""

import shutil
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph import _core
from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_symbol_refs"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "msr"
    shutil.copytree(FIXTURE, dst)
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def test_python_inbound_refs_for_py_sdk(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    refs = _core.refs_to_symbol(db, "py_sdk", None)
    assert refs, "expected ≥1 inbound ref to py_sdk"
    names = {r.to_symbol_name for r in refs}
    assert "Client" in names
    assert "AdminClient" in names
    assert "helper" in names


def test_python_inbound_refs_filter_by_symbol(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    client_refs = _core.refs_to_symbol(db, "py_sdk", "Client")
    assert len(client_refs) == 1
    assert client_refs[0].from_project_name == "py_consumer"
    assert client_refs[0].to_module_path == "client"


def test_python_outbound_refs_for_consumer(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    refs = _core.refs_from_project(db, "py_consumer")
    targets = {(r.to_project_name, r.to_symbol_name) for r in refs}
    assert ("py_sdk", "Client") in targets
    assert ("py_sdk", "AdminClient") in targets
    assert ("py_sdk", "helper") in targets
```

- [ ] **Step 4: Rust integration test**

`tests/integration/test_symbol_refs_rust.py`:
```python
"""M10: Rust cross-project symbol refs persist + are queryable."""

import shutil
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph import _core
from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_symbol_refs"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "msr"
    shutil.copytree(FIXTURE, dst)
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def test_rust_inbound_refs_for_sdk(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    refs = _core.refs_to_symbol(db, "rust_sdk", None)
    names = {r.to_symbol_name for r in refs}
    assert "Client" in names
    assert "build" in names


def test_rust_inbound_module_path_stripped(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    refs = _core.refs_to_symbol(db, "rust_sdk", "Client")
    assert len(refs) == 1
    # `use rust_sdk::client::Client` → to_module_path="client", to_symbol="Client"
    assert refs[0].to_module_path == "client"
```

- [ ] **Step 5: Run + commit**

```sh
uv sync --reinstall-package prograph
uv run pytest tests/integration/test_symbol_refs_python.py tests/integration/test_symbol_refs_rust.py -v
```

```sh
git add prograph/tests/fixtures/monorepo_symbol_refs/ \
        prograph/tests/integration/test_symbol_refs_python.py \
        prograph/tests/integration/test_symbol_refs_rust.py
git commit -m "prograph: M10 monorepo_symbol_refs fixture + Python/Rust integration tests"
```

---

## Task 13: MCP integration test for `find_symbol_references`

**Files:**
- Create: `tests/integration/test_mcp_find_symbol_references.py`

Async test exercising the new MCP tool against the `monorepo_symbol_refs` fixture.

- [ ] **Step 1: Write the test**

`tests/integration/test_mcp_find_symbol_references.py`:
```python
"""M10: find_symbol_references MCP tool via stdio."""

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
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_symbol_refs"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "msr"
    shutil.copytree(FIXTURE, dst)
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


async def _session(monorepo: Path):
    params = StdioServerParameters(
        command=sys.executable,
        args=["-m", "prograph.mcp_server", str(monorepo)],
    )
    return stdio_client(params)


async def test_find_symbol_references_inbound(indexed: Path):
    async with await _session(indexed) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool(
                "find_symbol_references",
                arguments={"project_name": "py_sdk", "symbol_name": "Client"},
            )
            payload = json.loads(result.content[0].text)
            assert len(payload) == 1
            assert payload[0]["from_project_name"] == "py_consumer"
            assert payload[0]["to_symbol_name"] == "Client"


async def test_find_symbol_references_outbound(indexed: Path):
    async with await _session(indexed) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool(
                "find_symbol_references",
                arguments={"project_name": "py_consumer", "direction": "outbound"},
            )
            payload = json.loads(result.content[0].text)
            target_pairs = {(r["to_project_name"], r["to_symbol_name"]) for r in payload}
            assert ("py_sdk", "Client") in target_pairs
            assert ("py_sdk", "AdminClient") in target_pairs


async def test_find_symbol_references_missing_project_arg(indexed: Path):
    async with await _session(indexed) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool("find_symbol_references", arguments={})
            payload = json.loads(result.content[0].text)
            assert "error" in payload


async def test_find_symbol_references_invalid_direction(indexed: Path):
    async with await _session(indexed) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool(
                "find_symbol_references",
                arguments={"project_name": "py_sdk", "direction": "sideways"},
            )
            payload = json.loads(result.content[0].text)
            assert "error" in payload
```

- [ ] **Step 2: Run + commit**

```sh
uv run pytest tests/integration/test_mcp_find_symbol_references.py -v
```

```sh
git add prograph/tests/integration/test_mcp_find_symbol_references.py
git commit -m "prograph: M10 MCP find_symbol_references integration tests"
```

---

## Task 14: Real-monorepo smoke verification

**Files:**
- Modify: `tests/integration/test_smoke_real.py`

After M10, the real `all_ai_orchestrators/` should produce at least one cross-project symbol ref — Maestro imports atp-platform's SDK somewhere.

- [ ] **Step 1: Extend the smoke**

Append to `tests/integration/test_smoke_real.py` (after the M9 modules assertion):
```python
    # M10: at least one cross-project symbol ref should exist.
    import sqlite3
    conn = sqlite3.connect(paths_db)
    n_refs = conn.execute(
        """
        SELECT COUNT(*) FROM cross_project_symbol_refs
        WHERE last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchone()[0]

    # Project pairs that have at least one ref between them.
    pairs = conn.execute(
        """
        SELECT DISTINCT p1.name, p2.name
        FROM cross_project_symbol_refs ref
        JOIN projects p1 ON p1.id = ref.from_project_id
        JOIN projects p2 ON p2.id = ref.to_project_id
        WHERE ref.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()

    # Soft assertion — log instead of fail if zero. The Maestro→atp-platform import
    # depends on whether atp_platform_sdk is the import path actually used in source.
    if n_refs == 0:
        import warnings as _w
        _w.warn(
            f"M10 smoke: real monorepo has 0 cross_project_symbol_refs. "
            f"Either no in-monorepo imports exist, or the resolver missed them. "
            f"Project pairs seen: {pairs}",
            stacklevel=2,
        )
    else:
        # If we did find refs, sanity-check that at least one pair is between
        # two distinct known projects.
        distinct_pairs = {(a, b) for (a, b) in pairs if a != b}
        assert distinct_pairs, f"expected refs between distinct projects, got: {pairs}"
```

- [ ] **Step 2: Run + commit**

```sh
uv run pytest -m realmonorepo -v
```

```sh
git add prograph/tests/integration/test_smoke_real.py
git commit -m "prograph: M10 real-monorepo smoke — log cross-project symbol ref counts"
```

---

## Task 15: README + CLAUDE.md + close

**Files:**
- Modify: `prograph/README.md`
- Modify: `prograph/CLAUDE.md`
- Modify: this plan file

- [ ] **Step 1: Update README**

```markdown
**Status:** M10 — Cross-project symbol references. Each external import in a project's source is resolved to a publisher project + sub-module path + (best-effort) symbol name. Persisted in `cross_project_symbol_refs`. Exposed via MCP tool `find_symbol_references` (direction=inbound|outbound), REST endpoint `/api/symbol_refs`, MD project cards ("Inbound references" + "Outbound references" sections), and the browser UI side panel. AI agents can now answer "if I change `MaestroAPI`, who calls it?" in one tool call.
```

Add a "Cross-project symbol references" subsection:
```markdown
### Cross-project symbol references (M10)

For each external import in a project's source file (e.g. `from atp_platform.sdk import Client` in Maestro), prograph resolves the target to a publisher project + module + symbol when both sides are in the same monorepo.

- **Inbound**: "who imports my X?" — `find_symbol_references project_name=X` returns from/to module + line.
- **Outbound**: "what do I import?" — `find_symbol_references project_name=X direction=outbound`.

Language coverage: Python (dotted paths, dash↔underscore norm, alias-aware) and Rust (`use crate_name::a::b::Symbol`). JS deferred (no driver in scope).

Resolution is conservative: stdlib + PyPI + crates.io imports are dropped silently. `pub use` re-exports in Rust land at the directly-imported crate, not the re-export source (best-effort; full chain following is a future enrichment).
```

- [ ] **Step 2: Update CLAUDE.md**

Add to the components list:
```markdown
  - `resolvers/{python,rust}` — cross-project import resolution (M10)
  - `migrations/v7.sql` — cross_project_symbol_refs table (M10)
  - New MCP tool: `find_symbol_references`
  - New REST endpoint: `GET /api/symbol_refs`
```

Update "What is NOT" section:
```markdown
## What is NOT in M10

- JS cross-project symbol resolution. package.json `exports` maze; no JS driver in scope.
- Method/attribute-level resolution (`obj.method`). M10 is module + top-level symbol granularity.
- `pub use` re-export chain following in Rust. The resolver lands at the directly-imported crate.
- Type signatures + docstrings. Still deferred from M9 backlog.
- HTTP/REST runtime edges. Still deferred.
- WebSocket live updates, offline asset bundle, Playwright E2E, auth/TLS, mobile/responsive. Still deferred from M8.
```

- [ ] **Step 3: Full local gate**

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

- [ ] **Step 4: Check DoD boxes + final commit**

```sh
git add prograph/README.md prograph/CLAUDE.md \
        prograph/docs/superpowers/plans/2026-05-26-prograph-m10-symbol-refs.md
git commit -m "prograph: M10 close — symbol refs shipped; docs updated; DoD checked"
```

---

## Definition of Done (M10)

- [x] `cargo test --all-targets` passes (≥160 tests — Tasks 1/2/5/6/7/8 add ~15).
- [x] `uv run pytest -v` passes (≥160 tests — Tasks 11/12/13 add ~13).
- [x] `uv run pytest -m realmonorepo -v` passes; soft warning emitted if real monorepo has zero refs.
- [x] Schema v7 (`cross_project_symbol_refs`) applies cleanly over v6.
- [x] `Module.external_imports` populated by both Python + Rust parsers; relative imports excluded.
- [x] `resolvers::python::resolve` handles dotted paths + dash↔underscore + alias lookup.
- [x] `resolvers::rust::resolve` handles `crate::a::b::Symbol` form + `{Symbol1, Symbol2}` list form.
- [x] Indexer writes one row per resolved cite (idempotent: COALESCE on first_seen).
- [x] `Store::refs_to_symbol(project, symbol?)` + `Store::refs_from_project(project)` both expose via PyO3 + pydantic + REST + MCP.
- [x] `ProjectDescription` carries `inbound_refs` + `outbound_refs`; MD + browser UI render both.
- [x] `monorepo_symbol_refs` fixture exercises Python + Rust resolution; 3 + 2 integration tests pass.
- [x] MCP `find_symbol_references` tool returns expected results for both directions; invalid args return `{"error": ...}` JSON.
- [x] CI workflow continues to pass.
- [x] All commits follow the `prograph: M10 ...` prefix convention.

## What is NOT done in M10 (deferred to M11+ or never)

- **JS cross-project symbol resolution** — needs package.json `exports` parsing; defer until JS driver appears.
- **Method-level resolution** — `obj.method()` granularity; current is module + top-level symbol.
- **`pub use` re-export chain following** in Rust.
- **Type signatures + docstrings** for symbols.
- **HTTP / REST runtime edges, WebSocket, offline bundle, Playwright, auth/TLS, mobile** — still deferred from M8.

M10 ships as v1.2. Subsequent work is genuinely usage-feedback-driven; the roadmap considered "complete" relative to the original brainstorm vision.
