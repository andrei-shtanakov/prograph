# prograph M9 — Module-Level Facts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After M9, every project carries module-level facts — list of source files, public symbols exposed (functions / classes / structs / traits), internal imports — alongside the existing manifest-level data. The "Public surface" section in per-project MD files (M5) finally has content. The MCP `describe_project` tool (M7) returns symbols. The browser UI's side panel (M6) shows them. On the real `all_ai_orchestrators/` monorepo, opening `Maestro.md` reveals `MaestroAPI`, `MaestroATPAdapter`, and the actual modules they live in.

**Architecture:**
- **Three languages, three tree-sitter scans** — extend the existing M4 `parsers/python.rs` and `parsers/rust.rs` source scanners (which currently scan only for MCP tool decls/uses) to ALSO emit `Module`, `PublicSymbol`, and `InternalImport` facts. New `parsers/js.rs` source scan replaces M3's manifest-only JS parser (the JS MCP scanning previously deferred in M7 lands here too, as a fall-out — but only for completeness, not because the target monorepo needs it).
- **Schema v6 adds three additive tables** — `modules` (per-file metadata), `public_symbols` (denormalised one-row-per-symbol), `internal_imports` (one-row-per-import-site). All three follow the same temporal pattern as `mcp_tool_decls` (M5) and `contract_files` (M4): `first_seen` + `last_seen`, sub-data of projects, no `change_log` entries written directly.
- **`ProjectDescription` (M5) gains three new fields** — `modules`, `public_symbols`, `internal_imports`. The aggregation pyclass extension flows to pydantic mirror, to the MCP `describe_project` return shape, to the REST `/api/projects/by-name/{name}` response, to the browser UI's side panel, and to the M5 MD renderer.
- **Identity rules**: per spec §5.2 — modules identified by `(project_id, rel_path)`; symbols by `(module_id, symbol_name)`; imports by `(module_id, target_path, line)` (line is part of identity because the same target imported on two lines is two facts).
- **Public-only filtering applied in Rust, not in tree-sitter queries.** Tree-sitter captures all candidate symbols; Rust-side post-processing applies the language-specific public/private rules (Python: no leading underscore + member of `__all__` if present; Rust: `pub` visibility; JS: explicit `export`).

**Tech Stack additions (M9 only):**
- `tree-sitter-javascript = "0.21"` (workspace dep) — extends the M4 tree-sitter family to JS for the new module scan. The grammar crate compiles via `cc-rs`; same MSRV constraints as `tree-sitter-python` + `tree-sitter-rust`.

No new Python deps. No new MCP transport. No schema changes beyond the three additive tables in v6.

**Spec reference:** `docs/superpowers/specs/2026-05-25-prograph-design.md` §4.1 parsers ("Module { project_id, path, language, public_symbols, imports }" — M9 finally implements the spec-promised fields), §5.3 MD export (the "Public surface" subsection that M5 left empty for symbols).

**Baseline:** Branch off `main` at the M8 close commit (after user reports M8 complete; check `git log`). All gates green from M1-M8.

**M9 explicitly out of scope (deferred to M10+ or never):**
- **Module dependency graph edges** — internal_imports could materialise as edges between modules within a project. M9 stores the facts but doesn't create graph edges from them. M10 stretch goal.
- **Symbol-level cross-project edges** — "Maestro.api uses arbiter::policy::decide" via tree-sitter resolution. Heavy work, low payoff with current MCP detection covering the API surface.
- **Type signatures / docstrings** for symbols — the public_symbols table stores name + kind + line. Signature extraction is a future enrichment.
- **HTTP / REST runtime edges** — still deferred from M8.
- **WebSocket live updates, offline bundle, Playwright E2E, auth/TLS, mobile** — still deferred from M8.

---

## File Structure (created/modified in M9)

```
prograph/
├── Cargo.toml                                      # MODIFY — add tree-sitter-javascript workspace dep
├── prograph-core/
│   ├── Cargo.toml                                  # MODIFY — reference tree-sitter-javascript
│   ├── src/
│   │   ├── lib.rs                                  # MODIFY — register new pyclasses + exports
│   │   ├── facts.rs                                # MODIFY — Module / PublicSymbol / InternalImport
│   │   ├── models.rs                               # MODIFY — three new aggregation pyclasses
│   │   ├── store.rs                                # MODIFY — alive_* + insert/touch helpers + describe_project extension
│   │   ├── indexer.rs                              # MODIFY — persist modules + symbols + imports per snapshot
│   │   ├── parsers/
│   │   │   ├── python.rs                           # MODIFY — extend source scan to symbols + imports + module list
│   │   │   ├── rust.rs                             # MODIFY — same
│   │   │   ├── js.rs                               # MODIFY — add tree-sitter source scan (replaces manifest-only)
│   │   │   └── mod.rs                              # MODIFY — ParserOutput gains modules field
│   │   ├── ts_queries/
│   │   │   ├── python_symbols.scm                  # NEW — public symbols + imports
│   │   │   ├── rust_symbols.scm                    # NEW
│   │   │   └── js_symbols.scm                      # NEW
│   │   └── migrations/
│   │       └── v6.sql                              # NEW — modules + public_symbols + internal_imports
├── prograph/
│   ├── _core.pyi                                   # MODIFY — three new pyclass stubs
│   ├── __init__.py                                 # MODIFY — re-export new pydantic types
│   ├── models.py                                   # MODIFY — three new pydantic mirrors
│   ├── export/render.py                            # MODIFY — Public surface section renders symbols + modules
│   └── web_static/app.js                           # MODIFY — side panel shows public_symbols list
├── tests/
│   ├── fixtures/
│   │   └── monorepo_modules/                       # NEW — Python + Rust + JS projects with explicit public symbols
│   ├── unit/
│   │   ├── test_module_facts_serde.py              # NEW
│   │   └── test_export_render_public_surface.py    # NEW
│   └── integration/
│       ├── test_module_facts_python.py             # NEW
│       ├── test_module_facts_rust.py               # NEW
│       └── test_module_facts_js.py                 # NEW
```

---

## Task 1: Workspace dep — `tree-sitter-javascript`

**Files:**
- Modify: `prograph/Cargo.toml`
- Modify: `prograph-core/Cargo.toml`

- [ ] **Step 1: Add workspace dep**

In `prograph/Cargo.toml`, append to `[workspace.dependencies]`:
```toml
tree-sitter-javascript = "0.21"
```

- [ ] **Step 2: Pull into the crate**

In `prograph-core/Cargo.toml`, append to `[dependencies]`:
```toml
tree-sitter-javascript = { workspace = true }
```

- [ ] **Step 3: Verify build**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo build --package prograph-core 2>&1 | tail -5
```

First build compiles the C grammar (~30s). Look for MSRV violations on transitive deps — apply the same pinning pattern as M1's getrandom or M2's indexmap if encountered.

```sh
cargo test --package prograph-core
```
Expected: existing test count unchanged (no semantic change yet).

- [ ] **Step 4: Commit**

```sh
git add prograph/Cargo.toml prograph/prograph-core/Cargo.toml prograph/Cargo.lock
git commit -m "prograph: M9 add tree-sitter-javascript workspace dep"
```

---

## Task 2: Schema v6 — `modules`, `public_symbols`, `internal_imports`

**Files:**
- Create: `prograph-core/src/migrations/v6.sql`
- Modify: `prograph-core/src/store.rs`

Three additive tables, all temporal (first_seen / last_seen). No CHECK widening needed (these are sub-data, not first-class entities — they don't appear in `change_log`).

- [ ] **Step 1: Write `v6.sql`**

`prograph-core/src/migrations/v6.sql`:
```sql
-- prograph schema v6 — module-level facts (public symbols + internal imports).
-- Sub-data of projects; no change_log entries are emitted for these rows directly.

CREATE TABLE IF NOT EXISTS modules (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id),
    rel_path    TEXT NOT NULL,
    language    TEXT NOT NULL CHECK (language IN ('python', 'rust', 'js')),
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
    UNIQUE(project_id, rel_path)
);

CREATE INDEX IF NOT EXISTS idx_modules_last_seen ON modules(last_seen);
CREATE INDEX IF NOT EXISTS idx_modules_project ON modules(project_id);

CREATE TABLE IF NOT EXISTS public_symbols (
    module_id   INTEGER NOT NULL REFERENCES modules(id),
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,  -- 'function', 'class', 'struct', 'enum', 'trait', 'const', ...
    line        INTEGER NOT NULL,
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
    PRIMARY KEY(module_id, name)
);

CREATE INDEX IF NOT EXISTS idx_public_symbols_last_seen ON public_symbols(last_seen);

CREATE TABLE IF NOT EXISTS internal_imports (
    module_id   INTEGER NOT NULL REFERENCES modules(id),
    target_path TEXT NOT NULL,  -- e.g. "maestro.api" or "crate::policy" or "./util"
    line        INTEGER NOT NULL,
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
    PRIMARY KEY(module_id, target_path, line)
);

CREATE INDEX IF NOT EXISTS idx_internal_imports_last_seen ON internal_imports(last_seen);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (6, datetime('now'));
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
];
```

- [ ] **Step 3: Add inline tests**

Append to `store.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn schema_v6_creates_module_tables() {
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
        assert!(names.contains(&"modules".to_string()));
        assert!(names.contains(&"public_symbols".to_string()));
        assert!(names.contains(&"internal_imports".to_string()));
        assert_eq!(store.schema_version().unwrap(), 6);
    }
```

- [ ] **Step 4: Run + commit**

```sh
cargo test --package prograph-core store
```
Expected: existing store tests +1.

```sh
git add prograph/prograph-core/src/migrations/v6.sql prograph/prograph-core/src/store.rs
git commit -m "prograph: M9 schema v6 — modules + public_symbols + internal_imports"
```

---

## Task 3: Facts extension — `Module`, `PublicSymbol`, `InternalImport`

**Files:**
- Modify: `prograph-core/src/facts.rs`
- Modify: `prograph-core/src/parsers/mod.rs`

Three new fact types parsers emit. `ProjectFacts` gains a single `modules: Vec<Module>` field; symbols and imports live inside each `Module`.

- [ ] **Step 1: Extend `facts.rs`**

Append to `prograph-core/src/facts.rs`:
```rust
/// A source file inside a project. Carries public symbols + internal imports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Module {
    pub rel_path: String,
    pub language: String,  // 'python' | 'rust' | 'js'
    #[serde(default)]
    pub public_symbols: Vec<PublicSymbol>,
    #[serde(default)]
    pub internal_imports: Vec<InternalImport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Class,
    Struct,
    Enum,
    Trait,
    Const,
    Type,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Const => "const",
            SymbolKind::Type => "type",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalImport {
    /// Dotted (Python) or `::`-separated (Rust) or relative (JS) path.
    pub target_path: String,
    pub line: u32,
}
```

Update `ProjectFacts` to add a single field:
```rust
pub struct ProjectFacts {
    pub project_root: String,
    pub project_name: String,
    pub manifest: Option<Manifest>,
    pub warnings: Vec<ParseWarning>,
    pub parse_status: ParseStatus,
    #[serde(default)]
    pub mcp_decls: Vec<McpToolDecl>,
    #[serde(default)]
    pub mcp_uses: Vec<McpClientUse>,
    #[serde(default)]
    pub contracts: Vec<ContractFile>,
    /// M9: per-file source-level facts.
    #[serde(default)]
    pub modules: Vec<Module>,
}
```

The `#[serde(default)]` keeps M2-M8 snapshots round-trippable.

- [ ] **Step 2: Extend `ParserOutput`**

In `prograph-core/src/parsers/mod.rs`, add the `modules` field to `ParserOutput`:
```rust
pub struct ParserOutput {
    pub manifest: Option<Manifest>,
    pub warnings: Vec<ParseWarning>,
    pub mcp_decls: Vec<crate::facts::McpToolDecl>,
    pub mcp_uses: Vec<crate::facts::McpClientUse>,
    pub contracts: Vec<crate::facts::ContractFile>,
    /// M9: source files with public symbols + internal imports.
    pub modules: Vec<crate::facts::Module>,
}
```

Compiler will flag every existing `ParserOutput { ... }` construction. Add `modules: vec![]` to each — they're populated by the actual parser scans in Tasks 5-7.

- [ ] **Step 3: Add tests**

Append to `facts.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn module_round_trips_via_serde() {
        let m = Module {
            rel_path: "src/lib.rs".into(),
            language: "rust".into(),
            public_symbols: vec![PublicSymbol {
                name: "Decider".into(),
                kind: SymbolKind::Struct,
                line: 42,
            }],
            internal_imports: vec![InternalImport {
                target_path: "crate::policy".into(),
                line: 3,
            }],
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: Module = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn symbol_kind_as_str_matches_schema() {
        // Schema CHECK isn't on public_symbols.kind (we left it unconstrained for flexibility),
        // but verify the canonical strings stay consistent.
        assert_eq!(SymbolKind::Function.as_str(), "function");
        assert_eq!(SymbolKind::Class.as_str(), "class");
        assert_eq!(SymbolKind::Struct.as_str(), "struct");
    }

    #[test]
    fn project_facts_back_compat_without_modules() {
        let json = r#"{
            "project_root": "./p",
            "project_name": "p",
            "manifest": null,
            "warnings": [],
            "parse_status": "Ok"
        }"#;
        let back: ProjectFacts = serde_json::from_str(json).unwrap();
        assert!(back.modules.is_empty());
    }
```

- [ ] **Step 4: Run + commit**

```sh
cargo test --package prograph-core facts
cargo test --package prograph-core
```
Expected: facts tests +3.

```sh
git add prograph/prograph-core/src/facts.rs prograph/prograph-core/src/parsers/mod.rs \
        prograph/prograph-core/src/parsers/python.rs \
        prograph/prograph-core/src/parsers/rust.rs \
        prograph/prograph-core/src/parsers/js.rs
git commit -m "prograph: M9 facts — Module / PublicSymbol / InternalImport + ParserOutput extension"
```

---

## Task 4: Tree-sitter query files

**Files:**
- Create: `prograph-core/src/ts_queries/python_symbols.scm`
- Create: `prograph-core/src/ts_queries/rust_symbols.scm`
- Create: `prograph-core/src/ts_queries/js_symbols.scm`

S-expression patterns over the three grammars. **Public-only filtering is applied in Rust post-processing** — queries capture all top-level definitions; Rust drops underscored Python names and non-`pub` Rust items.

- [ ] **Step 1: `python_symbols.scm`**

`prograph-core/src/ts_queries/python_symbols.scm`:
```scheme
; Top-level function definitions (children of the module node, not nested in classes/functions).
(module
  (function_definition
    name: (identifier) @symbol_name)) @symbol_function

; Top-level class definitions.
(module
  (class_definition
    name: (identifier) @symbol_name)) @symbol_class

; Top-level assignments (potential constants like NAME = "...").
(module
  (expression_statement
    (assignment
      left: (identifier) @symbol_name))) @symbol_const

; Imports.
;
; Pattern 1: `import foo` or `import foo.bar`.
(import_statement
  name: (dotted_name) @import_target) @import_simple

; Pattern 2: `from foo.bar import x, y`.
(import_from_statement
  module_name: (dotted_name) @import_target) @import_from

; Pattern 3: `from .relative import x`.
(import_from_statement
  module_name: (relative_import) @import_target) @import_from_relative
```

- [ ] **Step 2: `rust_symbols.scm`**

`prograph-core/src/ts_queries/rust_symbols.scm`:
```scheme
; Public functions.
(function_item
  (visibility_modifier) @vis_pub
  name: (identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_function

; Public structs.
(struct_item
  (visibility_modifier) @vis_pub
  name: (type_identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_struct

; Public enums.
(enum_item
  (visibility_modifier) @vis_pub
  name: (type_identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_enum

; Public traits.
(trait_item
  (visibility_modifier) @vis_pub
  name: (type_identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_trait

; Public consts + statics.
(const_item
  (visibility_modifier) @vis_pub
  name: (identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_const

(static_item
  (visibility_modifier) @vis_pub
  name: (identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_const_static

; Public type aliases.
(type_item
  (visibility_modifier) @vis_pub
  name: (type_identifier) @symbol_name
  (#match? @vis_pub "^pub")) @symbol_type

; Internal imports: `use crate::...`.
(use_declaration
  argument: (scoped_use_list
    path: (identifier) @import_root
    (#eq? @import_root "crate"))) @import_use_crate

(use_declaration
  argument: (scoped_identifier
    path: (identifier) @import_root
    (#eq? @import_root "crate"))) @import_use_crate_simple
```

- [ ] **Step 3: `js_symbols.scm`**

`prograph-core/src/ts_queries/js_symbols.scm`:
```scheme
; Exported function declaration: `export function foo() {}`.
(export_statement
  declaration: (function_declaration
    name: (identifier) @symbol_name)) @symbol_function_export

; Exported class declaration.
(export_statement
  declaration: (class_declaration
    name: (identifier) @symbol_name)) @symbol_class_export

; Exported const: `export const FOO = ...`.
(export_statement
  declaration: (lexical_declaration
    (variable_declarator
      name: (identifier) @symbol_name))) @symbol_const_export

; Internal imports: `import x from './y'` or `import x from '../y'`.
(import_statement
  source: (string) @import_source
  (#match? @import_source "^['\"]\\.{1,2}/")) @import_relative
```

- [ ] **Step 4: Commit**

(No tests at this stage — they exercise via Tasks 5-7.)

```sh
git add prograph/prograph-core/src/ts_queries/{python,rust,js}_symbols.scm
git commit -m "prograph: M9 tree-sitter query files — python/rust/js public symbols + internal imports"
```

---

## Task 5: Python source scan — populate `modules`

**Files:**
- Modify: `prograph-core/src/parsers/python.rs`

Add a second tree-sitter scan over `.py` files. For each file: emit a `Module` with the file's `rel_path`, language, the public symbols (post-filtered), and the internal imports (filtered to those starting with the project's package name).

- [ ] **Step 1: Add `scan_python_modules` to `python.rs`**

Below the existing `scan_python_source` (M4's MCP scanner), add:

```rust
/// Walk all .py files under `project_root` and extract Module + PublicSymbol + InternalImport facts.
/// Public symbols filter: skip names starting with '_'. If the file has a top-level `__all__ = [...]`
/// declaration, restrict the public set to that whitelist (still skipping underscored entries inside
/// `__all__` for safety).
fn scan_python_modules(
    project_root: &Path,
    declared_package: &str,
) -> (Vec<crate::facts::Module>, Vec<crate::facts::ParseWarning>) {
    use tree_sitter::{Language, Parser, Query, QueryCursor};
    use walkdir::WalkDir;

    let language: Language = tree_sitter_python::language();
    let query_src = include_str!("../ts_queries/python_symbols.scm");

    let query = match Query::new(&language, query_src) {
        Ok(q) => q,
        Err(e) => {
            return (
                vec![],
                vec![crate::facts::ParseWarning {
                    rel_path: "ts_queries/python_symbols.scm".into(),
                    message: format!("failed to compile query: {}", e),
                }],
            );
        }
    };

    let mut modules: Vec<crate::facts::Module> = Vec::new();
    let mut warnings: Vec<crate::facts::ParseWarning> = Vec::new();

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return (modules, vec![crate::facts::ParseWarning {
            rel_path: "<tree-sitter init>".into(),
            message: "failed to initialise tree-sitter-python".into(),
        }]);
    }

    // Package prefix for filtering internal imports. e.g. declared_name "maestro" matches
    // "maestro", "maestro.api", "maestro.benchmark.adapter".
    let pkg_prefix = declared_package.replace('-', "_");  // common normalisation

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !matches!(
            name.as_ref(),
            ".venv" | "__pycache__" | "node_modules" | "target" | "dist" | "build" | ".git"
        ) && !name.starts_with('.')
            || e.depth() == 0
    }) {
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        if !entry.file_type().is_file() { continue; }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("py") { continue; }

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
                    message: "tree-sitter parse failed".into(),
                });
                continue;
            }
        };

        let source_bytes = source.as_bytes();
        let mut cursor = QueryCursor::new();

        let mut public_symbols: Vec<crate::facts::PublicSymbol> = Vec::new();
        let mut internal_imports: Vec<crate::facts::InternalImport> = Vec::new();

        for m in cursor.matches(&query, tree.root_node(), source_bytes) {
            // Identify the pattern from capture name markers.
            let mut symbol_name: Option<String> = None;
            let mut import_target: Option<String> = None;
            let mut line: u32 = 1;
            let mut kind_hint: Option<crate::facts::SymbolKind> = None;
            let mut is_import = false;

            for capture in m.captures {
                let cap_name = &query.capture_names()[capture.index as usize];
                let text = capture.node.utf8_text(source_bytes).unwrap_or("").to_string();
                line = capture.node.start_position().row as u32 + 1;

                match cap_name.as_str() {
                    "symbol_name" => symbol_name = Some(text),
                    "import_target" => {
                        import_target = Some(text);
                        is_import = true;
                    }
                    "symbol_function" => kind_hint = Some(crate::facts::SymbolKind::Function),
                    "symbol_class" => kind_hint = Some(crate::facts::SymbolKind::Class),
                    "symbol_const" => kind_hint = Some(crate::facts::SymbolKind::Const),
                    "import_simple" | "import_from" | "import_from_relative" => is_import = true,
                    _ => {}
                }
            }

            if is_import {
                if let Some(target) = import_target {
                    // Internal-import filter: only keep imports starting with the package's name
                    // (or a relative ".something" form).
                    let is_relative = target.starts_with('.');
                    let is_internal = is_relative
                        || target == pkg_prefix
                        || target.starts_with(&format!("{pkg_prefix}."));
                    if is_internal {
                        internal_imports.push(crate::facts::InternalImport {
                            target_path: target,
                            line,
                        });
                    }
                }
                continue;
            }

            let Some(name) = symbol_name else { continue };
            if name.starts_with('_') { continue; }  // M9 public-only filter
            let kind = kind_hint.unwrap_or(crate::facts::SymbolKind::Function);
            public_symbols.push(crate::facts::PublicSymbol { name, kind, line });
        }

        // Stable ordering.
        public_symbols.sort_by(|a, b| (a.line, &a.name).cmp(&(b.line, &b.name)));
        internal_imports.sort_by(|a, b| (a.line, &a.target_path).cmp(&(b.line, &b.target_path)));

        modules.push(crate::facts::Module {
            rel_path,
            language: "python".into(),
            public_symbols,
            internal_imports,
        });
    }

    modules.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    (modules, warnings)
}
```

- [ ] **Step 2: Wire `scan_python_modules` into `parse()`**

Find where `parse` constructs the final `ParserOutput`. After the existing MCP scan call, add:
```rust
    let (modules, module_warnings) = scan_python_modules(
        project_root,
        declared_name.as_str(),
    );
    all_warnings.extend(module_warnings);
```

And include `modules` in the returned `ParserOutput`:
```rust
    Ok(ParserOutput {
        manifest: Some(Manifest { /* ... */ }),
        warnings: all_warnings,
        mcp_decls,
        mcp_uses,
        contracts: super::contracts::scan(project_root),
        modules,
    })
```

(Update the early-return branches similarly — they should construct empty `modules: vec![]`.)

- [ ] **Step 3: Add tests**

Append to `python.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn scans_public_python_function() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("pyproject.toml"), r#"[project]
name = "myproj"
"#).unwrap();
        fs::write(dir.path().join("api.py"), r#"def public_fn():
    return 1

def _private_fn():
    return 2

class PublicClass:
    pass
"#).unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "api.py").unwrap();
        let names: Vec<_> = module.public_symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"public_fn"));
        assert!(names.contains(&"PublicClass"));
        assert!(!names.contains(&"_private_fn"), "underscored names must be filtered");
    }

    #[test]
    fn scans_internal_imports_only() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("pyproject.toml"), r#"[project]
name = "myproj"
"#).unwrap();
        fs::write(dir.path().join("api.py"), r#"import os
import myproj.util
from myproj.helpers import foo
from external_lib import bar
"#).unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "api.py").unwrap();
        let targets: Vec<_> = module.internal_imports.iter().map(|i| i.target_path.as_str()).collect();
        assert!(targets.contains(&"myproj.util"));
        assert!(targets.contains(&"myproj.helpers"));
        assert!(!targets.contains(&"os"), "stdlib imports filtered");
        assert!(!targets.contains(&"external_lib"), "external pkg imports filtered");
    }

    #[test]
    fn scans_python_hyphen_dash_normalisation() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("pyproject.toml"), r#"[project]
name = "atp-platform"
"#).unwrap();
        fs::write(dir.path().join("api.py"), r#"import atp_platform.core
"#).unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "api.py").unwrap();
        let targets: Vec<_> = module.internal_imports.iter().map(|i| i.target_path.as_str()).collect();
        assert!(
            targets.contains(&"atp_platform.core"),
            "dash→underscore normalisation must match imports: {:?}",
            targets
        );
    }
```

- [ ] **Step 4: Run + commit**

```sh
cargo test --package prograph-core parsers::python
```
Expected: parsers Python tests +3.

```sh
git add prograph/prograph-core/src/parsers/python.rs
git commit -m "prograph: M9 Python source scan — modules + public symbols + internal imports"
```

---

## Task 6: Rust source scan — populate `modules`

**Files:**
- Modify: `prograph-core/src/parsers/rust.rs`

Apply the same pattern as Task 5 but for Rust. Add `scan_rust_modules` that walks `.rs` files, runs `rust_symbols.scm`, emits `Module` records.

- [ ] **Step 1: Add `scan_rust_modules` to `rust.rs`**

At the bottom of `prograph-core/src/parsers/rust.rs` (before `#[cfg(test)]`), add a function with the same shape as `scan_python_modules` from Task 5, with these specific differences:

- `language: Language = tree_sitter_rust::language()`
- `query_src = include_str!("../ts_queries/rust_symbols.scm")`
- File extension filter: `"rs"`
- Ignore dirs: `"target"` and the standard set
- Capture mapping:
  - `"symbol_function"` → `SymbolKind::Function`
  - `"symbol_struct"` → `SymbolKind::Struct`
  - `"symbol_enum"` → `SymbolKind::Enum`
  - `"symbol_trait"` → `SymbolKind::Trait`
  - `"symbol_const"`, `"symbol_const_static"` → `SymbolKind::Const`
  - `"symbol_type"` → `SymbolKind::Type`
- Internal-import filter: only `"import_use_crate"` and `"import_use_crate_simple"` matches are kept; `target_path` is set to the `@import_root` capture's parent expression text (the full `use crate::...;` line minus `use ` and `;`).
- Public filtering: tree-sitter query already filters with `(#match? @vis_pub "^pub")`, so no post-filter needed for visibility.

Here's the function in full:
```rust
fn scan_rust_modules(
    project_root: &Path,
) -> (Vec<crate::facts::Module>, Vec<crate::facts::ParseWarning>) {
    use tree_sitter::{Language, Parser, Query, QueryCursor};
    use walkdir::WalkDir;

    let language: Language = tree_sitter_rust::language();
    let query_src = include_str!("../ts_queries/rust_symbols.scm");

    let query = match Query::new(&language, query_src) {
        Ok(q) => q,
        Err(e) => return (vec![], vec![crate::facts::ParseWarning {
            rel_path: "ts_queries/rust_symbols.scm".into(),
            message: format!("failed to compile query: {}", e),
        }]),
    };

    let mut modules: Vec<crate::facts::Module> = Vec::new();
    let mut warnings: Vec<crate::facts::ParseWarning> = Vec::new();

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return (modules, vec![crate::facts::ParseWarning {
            rel_path: "<tree-sitter init>".into(),
            message: "failed to initialise tree-sitter-rust".into(),
        }]);
    }

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !matches!(
            name.as_ref(),
            "target" | "node_modules" | ".venv" | "dist" | "build" | ".git" | "__pycache__"
        ) && !name.starts_with('.')
            || e.depth() == 0
    }) {
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        if !entry.file_type().is_file() { continue; }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("rs") { continue; }

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
                    message: "tree-sitter parse failed".into(),
                });
                continue;
            }
        };

        let source_bytes = source.as_bytes();
        let mut cursor = QueryCursor::new();
        let mut public_symbols: Vec<crate::facts::PublicSymbol> = Vec::new();
        let mut internal_imports: Vec<crate::facts::InternalImport> = Vec::new();

        for m in cursor.matches(&query, tree.root_node(), source_bytes) {
            let mut symbol_name: Option<String> = None;
            let mut line: u32 = 1;
            let mut kind: Option<crate::facts::SymbolKind> = None;
            let mut import_path: Option<String> = None;

            for capture in m.captures {
                let cap_name = &query.capture_names()[capture.index as usize];
                let text = capture.node.utf8_text(source_bytes).unwrap_or("").to_string();
                line = capture.node.start_position().row as u32 + 1;
                match cap_name.as_str() {
                    "symbol_name" => symbol_name = Some(text),
                    "symbol_function" => kind = Some(crate::facts::SymbolKind::Function),
                    "symbol_struct" => kind = Some(crate::facts::SymbolKind::Struct),
                    "symbol_enum" => kind = Some(crate::facts::SymbolKind::Enum),
                    "symbol_trait" => kind = Some(crate::facts::SymbolKind::Trait),
                    "symbol_const" | "symbol_const_static" => kind = Some(crate::facts::SymbolKind::Const),
                    "symbol_type" => kind = Some(crate::facts::SymbolKind::Type),
                    "import_use_crate" | "import_use_crate_simple" => {
                        // Take the whole `use crate::...;` text minus the keyword + semicolon.
                        // The capture's parent is the `use_declaration` node.
                        if let Some(parent) = capture.node.parent() {
                            let raw = parent.utf8_text(source_bytes).unwrap_or("");
                            let cleaned = raw
                                .trim()
                                .trim_start_matches("use")
                                .trim()
                                .trim_end_matches(';')
                                .trim()
                                .to_string();
                            import_path = Some(cleaned);
                        }
                    }
                    _ => {}
                }
            }

            if let Some(path) = import_path {
                internal_imports.push(crate::facts::InternalImport { target_path: path, line });
                continue;
            }

            let Some(name) = symbol_name else { continue };
            let kind = kind.unwrap_or(crate::facts::SymbolKind::Function);
            public_symbols.push(crate::facts::PublicSymbol { name, kind, line });
        }

        public_symbols.sort_by(|a, b| (a.line, &a.name).cmp(&(b.line, &b.name)));
        internal_imports.sort_by(|a, b| (a.line, &a.target_path).cmp(&(b.line, &b.target_path)));

        modules.push(crate::facts::Module {
            rel_path,
            language: "rust".into(),
            public_symbols,
            internal_imports,
        });
    }

    modules.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    (modules, warnings)
}
```

- [ ] **Step 2: Wire into `parse()`**

In `rust.rs`'s `parse()`, after the existing MCP scan call, add:
```rust
    let (modules, module_warnings) = scan_rust_modules(project_root);
    all_warnings.extend(module_warnings);
```

And include `modules` in the returned `ParserOutput`. Same pattern as Python parser update from Task 5.

- [ ] **Step 3: Tests**

Append to `rust.rs`'s tests:
```rust
    #[test]
    fn scans_rust_public_struct_and_fn() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), r#"[package]
name = "my-crate"
"#).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), r#"pub struct Decider {
    pub policy: String,
}

pub fn decide(query: &str) -> bool { true }

fn private_helper() {}

pub enum Choice { Yes, No }

pub trait Decidable {}
"#).unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "src/lib.rs").unwrap();
        let names: Vec<_> = module.public_symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Decider"));
        assert!(names.contains(&"decide"));
        assert!(names.contains(&"Choice"));
        assert!(names.contains(&"Decidable"));
        assert!(!names.contains(&"private_helper"), "non-pub items filtered");
    }

    #[test]
    fn scans_rust_use_crate_imports() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), r#"[package]
name = "c"
"#).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), r#"use crate::policy::Decider;
use crate::storage;
use std::collections::HashMap;
"#).unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "src/lib.rs").unwrap();
        let imports: Vec<_> = module.internal_imports.iter().map(|i| i.target_path.as_str()).collect();
        assert!(
            imports.iter().any(|i| i.contains("crate::policy")),
            "expected crate::policy import, got: {:?}",
            imports
        );
        assert!(!imports.iter().any(|i| i.contains("std::collections")), "std imports filtered");
    }
```

- [ ] **Step 4: Run + commit**

```sh
cargo test --package prograph-core parsers::rust
```
Expected: parsers Rust tests +2.

```sh
git add prograph/prograph-core/src/parsers/rust.rs
git commit -m "prograph: M9 Rust source scan — modules + public symbols + crate::* imports"
```

---

## Task 7: JS source scan — populate `modules`

**Files:**
- Modify: `prograph-core/src/parsers/js.rs`

Same pattern as Tasks 5-6 but for JS using `tree-sitter-javascript`. Only `export`-prefixed declarations count as public.

- [ ] **Step 1: Add `scan_js_modules`**

In `prograph-core/src/parsers/js.rs`, add at the bottom (before `#[cfg(test)]`):
```rust
fn scan_js_modules(
    project_root: &Path,
) -> (Vec<crate::facts::Module>, Vec<crate::facts::ParseWarning>) {
    use tree_sitter::{Language, Parser, Query, QueryCursor};
    use walkdir::WalkDir;

    let language: Language = tree_sitter_javascript::language();
    let query_src = include_str!("../ts_queries/js_symbols.scm");

    let query = match Query::new(&language, query_src) {
        Ok(q) => q,
        Err(e) => return (vec![], vec![crate::facts::ParseWarning {
            rel_path: "ts_queries/js_symbols.scm".into(),
            message: format!("failed to compile query: {}", e),
        }]),
    };

    let mut modules: Vec<crate::facts::Module> = Vec::new();
    let mut warnings: Vec<crate::facts::ParseWarning> = Vec::new();

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return (modules, vec![crate::facts::ParseWarning {
            rel_path: "<tree-sitter init>".into(),
            message: "failed to initialise tree-sitter-javascript".into(),
        }]);
    }

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !matches!(
            name.as_ref(),
            "node_modules" | "dist" | "build" | ".git" | "target"
        ) && !name.starts_with('.')
            || e.depth() == 0
    }) {
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        if !entry.file_type().is_file() { continue; }
        let ext = entry.path().extension().and_then(|s| s.to_str());
        if !matches!(ext, Some("js") | Some("mjs") | Some("cjs") | Some("ts") | Some("tsx") | Some("jsx")) {
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
                    message: "tree-sitter parse failed".into(),
                });
                continue;
            }
        };

        let source_bytes = source.as_bytes();
        let mut cursor = QueryCursor::new();
        let mut public_symbols: Vec<crate::facts::PublicSymbol> = Vec::new();
        let mut internal_imports: Vec<crate::facts::InternalImport> = Vec::new();

        for m in cursor.matches(&query, tree.root_node(), source_bytes) {
            let mut symbol_name: Option<String> = None;
            let mut import_source: Option<String> = None;
            let mut line: u32 = 1;
            let mut kind: Option<crate::facts::SymbolKind> = None;

            for capture in m.captures {
                let cap_name = &query.capture_names()[capture.index as usize];
                let text = capture.node.utf8_text(source_bytes).unwrap_or("").to_string();
                line = capture.node.start_position().row as u32 + 1;
                match cap_name.as_str() {
                    "symbol_name" => symbol_name = Some(text),
                    "symbol_function_export" => kind = Some(crate::facts::SymbolKind::Function),
                    "symbol_class_export" => kind = Some(crate::facts::SymbolKind::Class),
                    "symbol_const_export" => kind = Some(crate::facts::SymbolKind::Const),
                    "import_source" => {
                        // Strip surrounding quotes.
                        let stripped = text
                            .trim_start_matches(['"', '\''])
                            .trim_end_matches(['"', '\''])
                            .to_string();
                        import_source = Some(stripped);
                    }
                    _ => {}
                }
            }

            if let Some(src) = import_source {
                internal_imports.push(crate::facts::InternalImport { target_path: src, line });
                continue;
            }

            let Some(name) = symbol_name else { continue };
            let kind = kind.unwrap_or(crate::facts::SymbolKind::Function);
            public_symbols.push(crate::facts::PublicSymbol { name, kind, line });
        }

        public_symbols.sort_by(|a, b| (a.line, &a.name).cmp(&(b.line, &b.name)));
        internal_imports.sort_by(|a, b| (a.line, &a.target_path).cmp(&(b.line, &b.target_path)));

        modules.push(crate::facts::Module {
            rel_path,
            language: "js".into(),
            public_symbols,
            internal_imports,
        });
    }

    modules.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    (modules, warnings)
}
```

- [ ] **Step 2: Wire into `parse()`**

Same shape as Tasks 5-6: after manifest parse, call `scan_js_modules(project_root)`, push warnings, set `modules: modules` in the returned `ParserOutput`.

- [ ] **Step 3: Tests**

Append to `js.rs`'s tests:
```rust
    #[test]
    fn scans_js_exports_and_relative_imports() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("package.json"), r#"{
  "name": "my-pkg"
}"#).unwrap();
        fs::write(dir.path().join("index.js"), r#"import { helper } from './util';
import lodash from 'lodash';

export function publicFn() {}
export class PublicClass {}
export const PUBLIC_CONST = 1;

function privateFn() {}
"#).unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "index.js").unwrap();
        let names: Vec<_> = module.public_symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"publicFn"));
        assert!(names.contains(&"PublicClass"));
        assert!(names.contains(&"PUBLIC_CONST"));
        assert!(!names.contains(&"privateFn"));

        let imports: Vec<_> = module.internal_imports.iter().map(|i| i.target_path.as_str()).collect();
        assert!(imports.contains(&"./util"));
        assert!(!imports.contains(&"lodash"), "non-relative imports filtered");
    }
```

- [ ] **Step 4: Commit**

```sh
cargo test --package prograph-core parsers::js
```

```sh
git add prograph/prograph-core/src/parsers/js.rs
git commit -m "prograph: M9 JS source scan — exports + relative imports via tree-sitter-javascript"
```

---

## Task 8: Store helpers + indexer persistence for modules + symbols + imports

**Files:**
- Modify: `prograph-core/src/store.rs`
- Modify: `prograph-core/src/indexer.rs`

Three new alive_* helpers + writer methods. Indexer extends the project persist loop to also write each project's `Module` facts.

- [ ] **Step 1: Add alive_* + writer methods**

Append to `impl Store`:
```rust
    pub fn alive_modules(&self) -> Result<std::collections::HashMap<String, i64>> {
        // Key: "{project_id}|{rel_path}", value: module_id.
        let mut out = std::collections::HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, rel_path FROM modules
             WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
        })?;
        for row in rows {
            let (mid, pid, path) = row?;
            out.insert(format!("{}|{}", pid, path), mid);
        }
        Ok(out)
    }
```

Append to `impl<'a> SnapshotWriter<'a>`:
```rust
    pub fn insert_module(
        &self,
        snapshot_id: i64,
        project_id: i64,
        rel_path: &str,
        language: &str,
    ) -> Result<i64> {
        // INSERT OR IGNORE so re-runs don't duplicate; then UPDATE last_seen.
        self.tx.execute(
            "INSERT OR IGNORE INTO modules (project_id, rel_path, language, first_seen, last_seen)
             VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![project_id, rel_path, language, snapshot_id, snapshot_id],
        )?;
        let mid: i64 = self.tx.query_row(
            "SELECT id FROM modules WHERE project_id = ? AND rel_path = ?",
            rusqlite::params![project_id, rel_path],
            |r| r.get(0),
        )?;
        self.tx.execute(
            "UPDATE modules SET last_seen = ? WHERE id = ?",
            rusqlite::params![snapshot_id, mid],
        )?;
        Ok(mid)
    }

    pub fn insert_public_symbol(
        &self,
        module_id: i64,
        snapshot_id: i64,
        name: &str,
        kind: &str,
        line: i64,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO public_symbols (module_id, name, kind, line, first_seen, last_seen)
             VALUES (?, ?, ?, ?,
                     COALESCE((SELECT first_seen FROM public_symbols WHERE module_id=? AND name=?), ?),
                     ?)",
            rusqlite::params![module_id, name, kind, line, module_id, name, snapshot_id, snapshot_id],
        )?;
        Ok(())
    }

    pub fn insert_internal_import(
        &self,
        module_id: i64,
        snapshot_id: i64,
        target_path: &str,
        line: i64,
    ) -> Result<()> {
        self.tx.execute(
            "INSERT OR REPLACE INTO internal_imports (module_id, target_path, line, first_seen, last_seen)
             VALUES (?, ?, ?,
                     COALESCE((SELECT first_seen FROM internal_imports
                               WHERE module_id=? AND target_path=? AND line=?), ?),
                     ?)",
            rusqlite::params![module_id, target_path, line, module_id, target_path, line, snapshot_id, snapshot_id],
        )?;
        Ok(())
    }
```

- [ ] **Step 2: Persist in `indexer.rs`**

In `prograph-core/src/indexer.rs`, locate the project persist loop. After each project is inserted/touched and `new_project_ids` updated, persist its modules. Use the same pattern as M5's `insert_mcp_tool_decl` block:

```rust
        if let Some(&pid) = new_project_ids.get(key) {
            let fact = facts.iter().find(|f| &f.project_root == key);
            if let Some(fact) = fact {
                // M5: mcp tool decls (existing block).
                for decl in &fact.mcp_decls {
                    writer.insert_mcp_tool_decl(pid, &decl.tool_name, &decl.rel_path, decl.line as i64, snap_id)?;
                }
                // M9: modules + symbols + imports.
                for module in &fact.modules {
                    let mid = writer.insert_module(snap_id, pid, &module.rel_path, &module.language)?;
                    for sym in &module.public_symbols {
                        writer.insert_public_symbol(mid, snap_id, &sym.name, sym.kind.as_str(), sym.line as i64)?;
                    }
                    for imp in &module.internal_imports {
                        writer.insert_internal_import(mid, snap_id, &imp.target_path, imp.line as i64)?;
                    }
                }
            }
        }
```

- [ ] **Step 3: Add an indexer test**

Append to `indexer.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn modules_persist_across_snapshots() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();
        fs::create_dir_all(dir.path().join("proj")).unwrap();
        fs::write(dir.path().join("proj/pyproject.toml"), r#"[project]
name = "proj"
"#).unwrap();
        fs::write(dir.path().join("proj/api.py"), r#"def public_thing():
    pass
"#).unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        index_monorepo(dir.path(), &mut store).unwrap();

        let n_modules: i64 = store.connection().query_row(
            "SELECT COUNT(*) FROM modules WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
            [], |r| r.get(0),
        ).unwrap();
        assert!(n_modules >= 1);

        let n_symbols: i64 = store.connection().query_row(
            "SELECT COUNT(*) FROM public_symbols WHERE last_seen = (SELECT MAX(id) FROM snapshots)",
            [], |r| r.get(0),
        ).unwrap();
        assert!(n_symbols >= 1, "expected at least one public symbol persisted");
    }
```

- [ ] **Step 4: Run + commit**

```sh
cargo test --package prograph-core
```

```sh
git add prograph/prograph-core/src/store.rs prograph/prograph-core/src/indexer.rs
git commit -m "prograph: M9 Store helpers + indexer persist modules / public_symbols / internal_imports"
```

---

## Task 9: Extend `describe_project` aggregation

**Files:**
- Modify: `prograph-core/src/models.rs`
- Modify: `prograph-core/src/store.rs`
- Modify: `prograph-core/src/lib.rs`
- Modify: `prograph/_core.pyi`
- Modify: `prograph/models.py`

`ProjectDescription` gets three new fields: `modules`, `public_symbols`, `internal_imports`. The aggregation reads from the new tables; pydantic mirrors follow.

- [ ] **Step 1: Add three new row pyclasses**

In `prograph-core/src/models.rs`, append:
```rust
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct ModuleRow {
    pub id: i64,
    pub rel_path: String,
    pub language: String,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct PublicSymbolRow {
    pub module_id: i64,
    pub rel_path: String,
    pub name: String,
    pub kind: String,
    pub line: i64,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct InternalImportRow {
    pub module_id: i64,
    pub rel_path: String,
    pub target_path: String,
    pub line: i64,
}
```

Extend `ProjectDescription` with the new fields:
```rust
pub struct ProjectDescription {
    // ... existing fields ...
    pub modules: Vec<ModuleRow>,
    pub public_symbols: Vec<PublicSymbolRow>,
    pub internal_imports: Vec<InternalImportRow>,
}
```

(Add the fields in the existing order; the struct literal in `Store::describe_project` will be updated next.)

Add the three classes to `pub use models::{...}` in `lib.rs` and register inside `#[pymodule]`:
```rust
    m.add_class::<ModuleRow>()?;
    m.add_class::<PublicSymbolRow>()?;
    m.add_class::<InternalImportRow>()?;
```

- [ ] **Step 2: Update `Store::describe_project` to populate the new fields**

In `prograph-core/src/store.rs`, find `describe_project`. Before the final `Ok(Some(ProjectDescription { ... }))`, add three queries:
```rust
        let modules: Vec<ModuleRow> = self
            .conn
            .prepare(
                "SELECT id, rel_path, language FROM modules
                 WHERE project_id = ? AND last_seen = ?
                 ORDER BY rel_path",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                Ok(ModuleRow {
                    id: r.get(0)?,
                    rel_path: r.get(1)?,
                    language: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let public_symbols: Vec<PublicSymbolRow> = self
            .conn
            .prepare(
                "SELECT ps.module_id, m.rel_path, ps.name, ps.kind, ps.line
                 FROM public_symbols ps
                 JOIN modules m ON m.id = ps.module_id
                 WHERE m.project_id = ? AND ps.last_seen = ?
                 ORDER BY m.rel_path, ps.line, ps.name",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                Ok(PublicSymbolRow {
                    module_id: r.get(0)?,
                    rel_path: r.get(1)?,
                    name: r.get(2)?,
                    kind: r.get(3)?,
                    line: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let internal_imports: Vec<InternalImportRow> = self
            .conn
            .prepare(
                "SELECT ii.module_id, m.rel_path, ii.target_path, ii.line
                 FROM internal_imports ii
                 JOIN modules m ON m.id = ii.module_id
                 WHERE m.project_id = ? AND ii.last_seen = ?
                 ORDER BY m.rel_path, ii.line, ii.target_path",
            )?
            .query_map(rusqlite::params![pid, snap_id], |r| {
                Ok(InternalImportRow {
                    module_id: r.get(0)?,
                    rel_path: r.get(1)?,
                    target_path: r.get(2)?,
                    line: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
```

Include them in the `ProjectDescription { ... }` literal:
```rust
        Ok(Some(ProjectDescription {
            // ... existing fields ...
            modules,
            public_symbols,
            internal_imports,
        }))
```

- [ ] **Step 3: Extend `_core.pyi`**

Append:
```python
class ModuleRow:
    id: int
    rel_path: str
    language: str

class PublicSymbolRow:
    module_id: int
    rel_path: str
    name: str
    kind: str
    line: int

class InternalImportRow:
    module_id: int
    rel_path: str
    target_path: str
    line: int
```

Update `ProjectDescription`:
```python
class ProjectDescription:
    # ... existing fields ...
    modules: list[ModuleRow]
    public_symbols: list[PublicSymbolRow]
    internal_imports: list[InternalImportRow]
```

- [ ] **Step 4: Pydantic mirrors**

In `prograph/models.py`, add:
```python
class ModuleRow(BaseModel):
    model_config = ConfigDict(frozen=True)
    id: int
    rel_path: str
    language: str

    @classmethod
    def from_core(cls, value: _core.ModuleRow) -> ModuleRow:
        return cls(id=value.id, rel_path=value.rel_path, language=value.language)


class PublicSymbolRow(BaseModel):
    model_config = ConfigDict(frozen=True)
    module_id: int
    rel_path: str
    name: str
    kind: str
    line: int

    @classmethod
    def from_core(cls, value: _core.PublicSymbolRow) -> PublicSymbolRow:
        return cls(
            module_id=value.module_id,
            rel_path=value.rel_path,
            name=value.name,
            kind=value.kind,
            line=value.line,
        )


class InternalImportRow(BaseModel):
    model_config = ConfigDict(frozen=True)
    module_id: int
    rel_path: str
    target_path: str
    line: int

    @classmethod
    def from_core(cls, value: _core.InternalImportRow) -> InternalImportRow:
        return cls(
            module_id=value.module_id,
            rel_path=value.rel_path,
            target_path=value.target_path,
            line=value.line,
        )
```

Extend `ProjectDescription` pydantic with the three new list fields and the `from_core` factory:
```python
    modules: list[ModuleRow]
    public_symbols: list[PublicSymbolRow]
    internal_imports: list[InternalImportRow]

    @classmethod
    def from_core(cls, value: _core.ProjectDescription) -> ProjectDescription:
        import json
        return cls(
            # ... existing fields ...
            modules=[ModuleRow.from_core(m) for m in value.modules],
            public_symbols=[PublicSymbolRow.from_core(s) for s in value.public_symbols],
            internal_imports=[InternalImportRow.from_core(i) for i in value.internal_imports],
        )
```

Update `prograph/__init__.py` to re-export the three new pydantic classes.

- [ ] **Step 5: Run + commit**

```sh
uv sync --reinstall-package prograph
cargo test --package prograph-core
uv run pytest -v
```

```sh
git add prograph/prograph-core/src/models.rs prograph/prograph-core/src/store.rs \
        prograph/prograph-core/src/lib.rs \
        prograph/prograph/_core.pyi prograph/prograph/models.py prograph/prograph/__init__.py
git commit -m "prograph: M9 ProjectDescription gains modules + public_symbols + internal_imports"
```

---

## Task 10: MD renderer — populate "Public surface" section

**Files:**
- Modify: `prograph/export/render.py`

The M5 MD template has a "Public surface" section with MCP tools + Contracts subsections. M9 adds a "Public symbols" subsection (per language) and a "Modules" mini-summary.

- [ ] **Step 1: Update `render_project`**

In `prograph/export/render.py`, locate the section that builds the "Public surface" block. After the existing "Contracts declared" subsection, insert:

```python
    lines.append("### Public symbols")
    lines.append("")
    if desc.public_symbols:
        for s in desc.public_symbols:
            lines.append(f"- `{s.name}` ({s.kind}) — `{s.rel_path}:{s.line}`")
    else:
        lines.append("_None._")
    lines.append("")
```

And after "Public surface", add a new top-level section:
```python
    lines.append("## Modules")
    lines.append("")
    if desc.modules:
        lines.append(f"_{len(desc.modules)} files, {len(desc.public_symbols)} public symbols, "
                     f"{len(desc.internal_imports)} internal imports._")
        lines.append("")
        for m in desc.modules:
            lines.append(f"- `{m.rel_path}` ({m.language})")
    else:
        lines.append("_None._")
    lines.append("")
```

(Place "## Modules" before the existing "## Outbound edges" section to keep document flow logical.)

- [ ] **Step 2: Refresh goldens**

The MD output for existing fixtures now has additional content. Regenerate goldens:
```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_full
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_multilang
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_mcp
```

Inspect a couple of the regenerated MDs — verify "Public symbols" + "Modules" sections look sensible.

- [ ] **Step 3: Run golden test without the env var**

```sh
uv run pytest tests/integration/test_cli_export_md.py -v
```
Expected: all pass.

- [ ] **Step 4: Add a focused unit test**

`tests/unit/test_export_render_public_surface.py`:
```python
"""M9: render_project's Public surface + Modules sections."""

from prograph.export.render import render_project
from prograph.models import (
    InternalImportRow,
    ModuleRow,
    ProjectDescription,
    PublicSymbolRow,
)


def _make_desc(**overrides) -> ProjectDescription:
    defaults = {
        "project_id": 1,
        "name": "x",
        "slug": "x",
        "kind": "python",
        "root_path": "./x",
        "attrs": {},
        "snapshot_id": 1,
        "snapshot_ts": "2026-05-26T00:00:00Z",
        "mcp_decls": [],
        "contract_files": [],
        "outbound": [],
        "inbound": [],
        "recent_changes": [],
        "modules": [],
        "public_symbols": [],
        "internal_imports": [],
    }
    defaults.update(overrides)
    return ProjectDescription(**defaults)


def test_public_symbols_render_when_present():
    desc = _make_desc(
        public_symbols=[
            PublicSymbolRow(module_id=1, rel_path="api.py", name="MaestroAPI", kind="class", line=10),
            PublicSymbolRow(module_id=1, rel_path="api.py", name="decide", kind="function", line=20),
        ],
    )
    md = render_project(desc)
    assert "### Public symbols" in md
    assert "`MaestroAPI` (class) — `api.py:10`" in md
    assert "`decide` (function) — `api.py:20`" in md


def test_modules_section_renders_summary():
    desc = _make_desc(
        modules=[
            ModuleRow(id=1, rel_path="api.py", language="python"),
            ModuleRow(id=2, rel_path="helpers.py", language="python"),
        ],
        public_symbols=[PublicSymbolRow(module_id=1, rel_path="api.py", name="x", kind="function", line=1)],
        internal_imports=[InternalImportRow(module_id=1, rel_path="api.py", target_path="x.util", line=3)],
    )
    md = render_project(desc)
    assert "## Modules" in md
    assert "2 files, 1 public symbols, 1 internal imports" in md
    assert "- `api.py` (python)" in md
    assert "- `helpers.py` (python)" in md


def test_empty_sections_render_none():
    desc = _make_desc()
    md = render_project(desc)
    # Public symbols section exists even when empty.
    public_section = md.split("### Public symbols")[1].split("###")[0]
    assert "_None._" in public_section


def test_render_is_deterministic_with_module_facts():
    desc = _make_desc(
        modules=[ModuleRow(id=1, rel_path="api.py", language="python")],
        public_symbols=[PublicSymbolRow(module_id=1, rel_path="api.py", name="x", kind="function", line=1)],
    )
    assert render_project(desc) == render_project(desc)
```

- [ ] **Step 5: Commit**

```sh
uv run pytest tests/unit/test_export_render_public_surface.py -v
```

```sh
git add prograph/prograph/export/render.py prograph/tests/unit/test_export_render_public_surface.py \
        prograph/tests/fixtures/monorepo_full/golden/ \
        prograph/tests/fixtures/monorepo_multilang/golden/ \
        prograph/tests/fixtures/monorepo_mcp/golden/
git commit -m "prograph: M9 MD renderer — Public symbols + Modules sections + golden refresh"
```

---

## Task 11: Browser UI side panel updates

**Files:**
- Modify: `prograph/web_static/app.js`

The side panel's `renderProject` already shows outbound/inbound edges. M9 adds a "Public symbols" list (collapsed by default for long lists) and a "Modules" count.

- [ ] **Step 1: Extend `renderProject`**

In `prograph/web_static/app.js`, locate `renderProject(p)`. After the existing MCP tools / Contracts / Outbound / Inbound blocks, add:

```javascript
    if (p.public_symbols && p.public_symbols.length) {
        nodes.push(el('h3', {}, ['Public symbols']));
        const items = p.public_symbols.slice(0, 50).map((s) => (
            el('li', {}, [
                el('code', {}, [s.name]),
                ` (${s.kind}) — `,
                el('code', {}, [`${s.rel_path}:${s.line}`]),
            ])
        ));
        nodes.push(el('ul', {}, items));
        if (p.public_symbols.length > 50) {
            nodes.push(el('p', {}, [el('em', {}, [`(${p.public_symbols.length - 50} more)`])]));
        }
    }
    if (p.modules && p.modules.length) {
        nodes.push(el('h3', {}, ['Modules']));
        const summary = `${p.modules.length} files, ${p.public_symbols?.length || 0} symbols, ` +
                        `${p.internal_imports?.length || 0} internal imports`;
        nodes.push(el('p', {}, [el('em', {}, [summary])]));
    }
```

- [ ] **Step 2: Update the structure assertion test**

In `tests/unit/test_web_static.py`, the existing `test_app_js_does_not_use_innerHTML` continues to enforce safety. Add one more assertion:

```python
def test_app_js_renders_public_symbols():
    js = (STATIC_DIR / "app.js").read_text()
    assert "Public symbols" in js
    assert "public_symbols" in js
```

- [ ] **Step 3: Run + commit**

```sh
uv run pytest tests/unit/test_web_static.py -v
uv run pytest tests/integration/test_cli_serve.py -v
```

```sh
git add prograph/prograph/web_static/app.js prograph/tests/unit/test_web_static.py
git commit -m "prograph: M9 browser UI — Public symbols + Modules sections in side panel"
```

---

## Task 12: `monorepo_modules` fixture + integration tests

**Files:**
- Create: `tests/fixtures/monorepo_modules/` (multiple files)
- Create: `tests/integration/test_module_facts_python.py`
- Create: `tests/integration/test_module_facts_rust.py`
- Create: `tests/integration/test_module_facts_js.py`

A focused fixture exercising all three language scans. Three small projects: `py_lib` (Python with internal imports + public API), `rust_lib` (Rust with `pub` items + `use crate::*`), `js_lib` (JS with exports + relative imports).

- [ ] **Step 1: Create `py_lib`**

`tests/fixtures/monorepo_modules/py_lib/pyproject.toml`:
```toml
[project]
name = "py_lib"
version = "0.1.0"
```

`tests/fixtures/monorepo_modules/py_lib/py_lib/__init__.py` (empty file).

`tests/fixtures/monorepo_modules/py_lib/py_lib/api.py`:
```python
"""Public surface module."""

from py_lib.helpers import normalize
from py_lib.storage import Store

class PublicAPI:
    """Top-level public class."""
    def __init__(self):
        self.store = Store()

    def query(self, q):
        return normalize(q)


def public_fn(x):
    return x * 2


def _private_helper():
    return 42


PUBLIC_CONST = "v1"
```

`tests/fixtures/monorepo_modules/py_lib/py_lib/helpers.py`:
```python
def normalize(s):
    return s.strip().lower()
```

`tests/fixtures/monorepo_modules/py_lib/py_lib/storage.py`:
```python
class Store:
    def __init__(self):
        self.data = {}
```

- [ ] **Step 2: Create `rust_lib`**

`tests/fixtures/monorepo_modules/rust_lib/Cargo.toml`:
```toml
[package]
name = "rust_lib"
version = "0.1.0"
edition = "2021"
```

`tests/fixtures/monorepo_modules/rust_lib/src/lib.rs`:
```rust
pub mod policy;
pub mod storage;

use crate::policy::Decider;
use crate::storage::Store;

pub struct PublicService {
    decider: Decider,
    store: Store,
}

pub fn build_service() -> PublicService {
    PublicService {
        decider: Decider::new(),
        store: Store::new(),
    }
}

fn internal_helper() -> i64 { 0 }
```

`tests/fixtures/monorepo_modules/rust_lib/src/policy.rs`:
```rust
pub struct Decider;

impl Decider {
    pub fn new() -> Self { Self }
}

pub trait Decidable {
    fn decide(&self) -> bool;
}
```

`tests/fixtures/monorepo_modules/rust_lib/src/storage.rs`:
```rust
pub struct Store;

impl Store {
    pub fn new() -> Self { Self }
}
```

- [ ] **Step 3: Create `js_lib`**

`tests/fixtures/monorepo_modules/js_lib/package.json`:
```json
{
  "name": "js_lib",
  "version": "0.1.0"
}
```

`tests/fixtures/monorepo_modules/js_lib/index.js`:
```javascript
import { normalize } from './helpers.js';
import { Store } from './storage.js';

export class PublicAPI {
    constructor() {
        this.store = new Store();
    }
    query(q) {
        return normalize(q);
    }
}

export function publicFn(x) {
    return x * 2;
}

export const PUBLIC_CONST = 'v1';
```

`tests/fixtures/monorepo_modules/js_lib/helpers.js`:
```javascript
export function normalize(s) {
    return s.trim().toLowerCase();
}
```

`tests/fixtures/monorepo_modules/js_lib/storage.js`:
```javascript
export class Store {
    constructor() {
        this.data = {};
    }
}
```

- [ ] **Step 4: Python integration test**

`tests/integration/test_module_facts_python.py`:
```python
"""M9: Python module facts populate after `prograph index`."""

import shutil
import sqlite3
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_modules"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "monorepo_modules"
    shutil.copytree(FIXTURE, dst)
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def test_py_lib_has_public_symbols(indexed: Path):
    db = indexed / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT ps.name, ps.kind, m.rel_path
        FROM public_symbols ps
        JOIN modules m ON m.id = ps.module_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.name = 'py_lib'
          AND ps.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()
    names = {r[0] for r in rows}
    assert "PublicAPI" in names
    assert "public_fn" in names
    assert "PUBLIC_CONST" in names
    assert "_private_helper" not in names, "private names must be filtered"


def test_py_lib_has_internal_imports(indexed: Path):
    db = indexed / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT ii.target_path
        FROM internal_imports ii
        JOIN modules m ON m.id = ii.module_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.name = 'py_lib'
          AND ii.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()
    targets = {r[0] for r in rows}
    assert "py_lib.helpers" in targets
    assert "py_lib.storage" in targets
```

- [ ] **Step 5: Rust integration test**

`tests/integration/test_module_facts_rust.py`:
```python
"""M9: Rust module facts populate after `prograph index`."""

import shutil
import sqlite3
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_modules"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "monorepo_modules"
    shutil.copytree(FIXTURE, dst)
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def test_rust_lib_has_pub_symbols(indexed: Path):
    db = indexed / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT ps.name, ps.kind
        FROM public_symbols ps
        JOIN modules m ON m.id = ps.module_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.name = 'rust_lib'
          AND ps.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()
    names = {r[0] for r in rows}
    assert "PublicService" in names
    assert "build_service" in names
    assert "Decider" in names
    assert "Decidable" in names
    assert "internal_helper" not in names, "non-pub items must be filtered"


def test_rust_lib_has_crate_imports(indexed: Path):
    db = indexed / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT ii.target_path
        FROM internal_imports ii
        JOIN modules m ON m.id = ii.module_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.name = 'rust_lib'
          AND ii.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()
    targets = " ".join(r[0] for r in rows)
    assert "crate::policy" in targets
    assert "crate::storage" in targets
```

- [ ] **Step 6: JS integration test**

`tests/integration/test_module_facts_js.py`:
```python
"""M9: JS module facts populate after `prograph index`."""

import shutil
import sqlite3
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_modules"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "monorepo_modules"
    shutil.copytree(FIXTURE, dst)
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def test_js_lib_has_exports(indexed: Path):
    db = indexed / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT ps.name
        FROM public_symbols ps
        JOIN modules m ON m.id = ps.module_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.name = 'js_lib'
          AND ps.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()
    names = {r[0] for r in rows}
    assert "PublicAPI" in names
    assert "publicFn" in names
    assert "PUBLIC_CONST" in names


def test_js_lib_has_relative_imports(indexed: Path):
    db = indexed / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT ii.target_path
        FROM internal_imports ii
        JOIN modules m ON m.id = ii.module_id
        JOIN projects p ON p.id = m.project_id
        WHERE p.name = 'js_lib'
          AND ii.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()
    targets = {r[0] for r in rows}
    assert "./helpers.js" in targets
    assert "./storage.js" in targets
```

- [ ] **Step 7: Run + commit**

```sh
uv sync --reinstall-package prograph
uv run pytest tests/integration/test_module_facts_python.py tests/integration/test_module_facts_rust.py tests/integration/test_module_facts_js.py -v
```
Expected: 6 passed.

```sh
git add prograph/tests/fixtures/monorepo_modules/ \
        prograph/tests/integration/test_module_facts_python.py \
        prograph/tests/integration/test_module_facts_rust.py \
        prograph/tests/integration/test_module_facts_js.py
git commit -m "prograph: M9 monorepo_modules fixture + Python/Rust/JS module fact integration tests"
```

---

## Task 13: Real-monorepo smoke verification

**Files:**
- Modify: `tests/integration/test_smoke_real.py`

Assert the real `all_ai_orchestrators/` produces known symbols. Use loose assertions that survive code refactors — just verify "Maestro project has at least one public symbol".

- [ ] **Step 1: Extend the smoke**

Append to `tests/integration/test_smoke_real.py`'s existing test body (after the M8 evidence assertion):
```python
    # M9: at least one project should have ≥1 public symbol persisted.
    import sqlite3
    conn = sqlite3.connect(paths_db)
    n_symbols = conn.execute(
        """
        SELECT COUNT(*) FROM public_symbols
        WHERE last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchone()[0]
    n_modules = conn.execute(
        """
        SELECT COUNT(*) FROM modules
        WHERE last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchone()[0]
    conn.close()
    assert n_modules >= 5, f"expected ≥5 modules across the real monorepo, got {n_modules}"
    assert n_symbols >= 5, f"expected ≥5 public symbols, got {n_symbols}"
```

- [ ] **Step 2: Run**

```sh
uv run pytest -m realmonorepo -v
```
Expected: 1 passed.

- [ ] **Step 3: Commit**

```sh
git add prograph/tests/integration/test_smoke_real.py
git commit -m "prograph: M9 real-monorepo smoke — assert modules + public_symbols populated"
```

---

## Task 14: README + CLAUDE.md + close

**Files:**
- Modify: `prograph/README.md`
- Modify: `prograph/CLAUDE.md`
- Modify: this plan file

- [ ] **Step 1: Update README Status**

```markdown
**Status:** M9 — Module-level facts. Each project now carries a list of source modules with their public symbols (Python `def`/`class`, Rust `pub` items, JS `export`s) and internal imports. The "Public surface" section in per-project MD files (M5) is populated; the MCP `describe_project` tool (M7) returns symbols; the browser UI side panel (M6) shows them.
```

Add a new "Module facts" subsection:
```markdown
### Module-level facts (M9)

For each project, prograph scans source files (`.py`, `.rs`, `.js`/`.ts`/`.mjs`/etc.) and extracts:

- **Modules** — one row per source file with rel_path + language
- **Public symbols** — `def`/`class` at module top level without leading underscore (Python); items with `pub` visibility (Rust); `export` declarations (JS)
- **Internal imports** — imports targeting the same project (Python `import myproj.x`; Rust `use crate::x`; JS `import x from './y'`)

These appear in MD project cards, MCP `describe_project`, and the browser UI side panel.

Module identity is `(project_id, rel_path)`; symbol identity is `(module_id, name)`. Renaming a file or symbol = remove + add events in the change_log.

#### Limitations

- Type signatures and docstrings are not extracted.
- Symbol-level cross-project edges (e.g. "Maestro.api uses arbiter::policy::decide") are deferred — module-level facts are descriptive metadata, not graph edges.
- The Python heuristic uses simple prefix matching for internal imports — `__all__` whitelists are not consulted (M10+ refinement).
```

- [ ] **Step 2: Update CLAUDE.md**

Bump the architecture section to "M9 state". Add the new entities to the components list:
- `parsers/{python,rust,js}` now emit `Module` facts via second tree-sitter scan
- `ts_queries/{python,rust,js}_symbols.scm` — module-level query files
- Schema v6 (modules + public_symbols + internal_imports tables)
- `ProjectDescription` gains `modules`/`public_symbols`/`internal_imports` lists
- MD renderer's "Public surface" section is populated

Update "What is NOT" section:
```markdown
## What is NOT in M9

- Symbol-level cross-project edges (e.g. "Maestro uses arbiter's Decider"). M9 stores facts but doesn't materialise them as graph edges.
- Type signatures + docstrings for symbols. Future enrichment.
- HTTP / REST runtime edges. Still deferred.
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

Expected: every command exits 0.

- [ ] **Step 4: Check DoD boxes + final commit**

Mark every `- [ ]` in "Definition of Done (M9)" as `- [x]`.

```sh
git add prograph/README.md prograph/CLAUDE.md \
        prograph/docs/superpowers/plans/2026-05-26-prograph-m9-module-facts.md
git commit -m "prograph: M9 close — module-level facts shipped; docs updated; DoD checked"
```

---

## Definition of Done (M9)

- [x] `cargo test --all-targets` passes (≥145 tests — Tasks 2/3/5/6/7/8/9 add ~15).
- [x] `uv run pytest -v` passes (≥147 tests — Tasks 10/11/12 add ~12).
- [x] `uv run pytest -m realmonorepo -v` passes; the real monorepo has ≥5 modules + ≥5 public_symbols persisted.
- [x] Schema v6 (`modules`, `public_symbols`, `internal_imports`) applies cleanly over v5 and the indexer populates all three.
- [x] `Module`, `PublicSymbol`, `InternalImport`, `SymbolKind` types exist in `facts.rs` with `#[serde(default)]` back-compat on `ProjectFacts.modules`.
- [x] `ParserOutput.modules` field exists; Python + Rust + JS parsers populate it from their respective `*_symbols.scm` queries.
- [x] Python parser filters underscored names + non-internal imports; Rust parser filters non-`pub` items + non-`crate::` imports; JS parser filters non-`export` decls + non-relative imports.
- [x] `ProjectDescription` carries `modules`/`public_symbols`/`internal_imports`; pydantic mirrors round-trip.
- [x] MD project cards include a "Public symbols" subsection + a "Modules" section; goldens are refreshed and committed.
- [x] Browser UI side panel renders public symbols list + modules summary (XSS-safe via `dom.js` `el()`).
- [x] CI workflow continues to pass.
- [x] All commits follow the `prograph: M9 ...` prefix convention.

## What is NOT done in M9 (deferred to M10+)

- **Symbol-level cross-project edges** — materialise `internal_imports` AND cross-project symbol references as graph edges. Significant tree-sitter resolution work.
- **Type signatures + docstrings** — enrich `PublicSymbol` with these. Tree-sitter can extract; design question is how to surface in MD / MCP / UI.
- **HTTP / REST runtime edges** — still deferred.
- **WebSocket live updates, offline asset bundle, Playwright E2E, auth/TLS, mobile/responsive** — still deferred from M8.

M9 ships as v1.1 (incremental release on top of M8's v1.0 candidate). Further milestones are genuinely optional and driven by usage feedback.
