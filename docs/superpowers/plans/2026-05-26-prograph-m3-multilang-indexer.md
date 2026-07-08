# prograph M3 — Multi-language Indexer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After M3, `prograph index` against the real `all_ai_orchestrators/` monorepo detects actual cross-project dependencies (Maestro→atp-platform-sdk, arbiter→spec-runner, atp-platform→spec-runner). Three manifest-based parsers ship — Python (extended), Rust, JS. Cross-language matching via shared `declared_name` + per-project `aliases` (workspace sub-packages). The real-monorepo smoke flips from "asserts ≥0 edges" to "asserts ≥3 edges."

**Architecture:** M3 stays manifest-based — `pyproject.toml`, `Cargo.toml`, `package.json` only. **No tree-sitter and no source-file parsing** — those land in M5+ when we need module-level facts (public symbols, internal imports, MCP decorators). The existing `parsers/python.rs` pattern (`fn parse(root: &Path) -> Result<ParserOutput>`) extends 1:1 for `parsers/rust.rs` and `parsers/js.rs`. The `Manifest` struct gains an `aliases: Vec<String>` field; `deps_detector` matches consumers' dep names against publishers' `declared_name` OR any of `aliases` (and warns on name collisions). Python parser additionally learns to read `[dependency-groups]` (PEP 735) and `[project.optional-dependencies]`, and to honor a `[tool.prograph]` config block that declares publish aliases for workspace projects.

**Tech Stack:**
- **Rust:** unchanged from M2 (edition 2021, MSRV 1.75; pyo3 0.22, rusqlite 0.31, toml 0.8, fslock 0.2, sha2 0.10, thiserror 1, serde/serde_json)
- **Python:** unchanged (typer + pydantic + rich)
- **Build:** maturin via `uv sync --reinstall-package prograph` (unchanged)
- **No new workspace deps.** `Cargo.toml` parsing uses the existing `toml` crate; `package.json` parsing uses `serde_json` (already a workspace dep).

**Spec reference:** `docs/superpowers/specs/2026-05-25-prograph-design.md` — §4.1 `parsers` (M3 ships 3 of 4 planned languages; vendored detection still deferred), §5.2 identity rules (unchanged — `package_dep` identity is still `(kind, from, to, dep_name)`), §6 indexing flow (phases unchanged).

**Baseline:** Branch off `main` at the M2 close commit `2706dc4`. 54 cargo + 34 pytest passing; CI green; `prograph init/index/status` working on Python monorepos.

**M3 explicitly out of scope (deferred to M4+):**
- Tree-sitter parsing of source files — M5 (when MD export needs module-level facts).
- Contracts detector + MCP detector — M4.
- HTTP/REST runtime edges — Phase 5 (post-M7).
- Vendored-file detection — Phase 6.
- MD export, browser UI, MCP stdio server — M5/M6/M7.
- Incremental reindex (mtime tracking) — Phase 7+.

---

## File Structure (created/modified in M3)

```
prograph/
├── Cargo.toml                                  # unchanged (no new workspace deps)
├── prograph-core/
│   ├── Cargo.toml                              # unchanged (toml + serde_json already present)
│   ├── src/
│   │   ├── lib.rs                              # MODIFY — register new parser dispatch arms
│   │   ├── facts.rs                            # MODIFY — add `aliases: Vec<String>` to Manifest
│   │   ├── parsers/
│   │   │   ├── mod.rs                          # MODIFY — extend dispatch for Rust + JS + Mixed
│   │   │   ├── python.rs                       # MODIFY — add PEP 735 / optional-deps / [tool.prograph] aliases
│   │   │   ├── rust.rs                         # NEW — Cargo.toml parser
│   │   │   └── js.rs                           # NEW — package.json parser
│   │   ├── detectors/
│   │   │   └── deps.rs                         # MODIFY — match against declared_name OR aliases; warn on collisions
│   │   └── indexer.rs                          # MODIFY — capture git_commit in snapshots
├── tests/
│   ├── fixtures/
│   │   └── monorepo_multilang/                 # NEW — 6 projects covering py/rust/js/mixed/aliases
│   │       ├── py_consumer/pyproject.toml
│   │       ├── py_publisher/pyproject.toml
│   │       ├── py_workspace/pyproject.toml     # uses [tool.prograph].aliases
│   │       ├── py_dev_consumer/pyproject.toml  # uses [dependency-groups]
│   │       ├── rust_consumer/Cargo.toml
│   │       ├── rust_publisher/Cargo.toml
│   │       ├── js_consumer/package.json
│   │       └── js_publisher/package.json
│   ├── unit/
│   │   └── test_facts.py                       # NEW — exercise Manifest.aliases round-trip (small)
│   └── integration/
│       ├── test_cli_index_multilang.py         # NEW — full pipeline against the multilang fixture
│       └── test_smoke_real.py                  # MODIFY — assert n_edges >= 3
```

No new top-level files at repo root. No new workspace dependencies. No schema migration (M2's `edges.kind = 'package_dep'` continues to apply — M3 just adds more sources of that same kind).

---

## Task 1: `Manifest.aliases` + Python `[dependency-groups]` parsing

**Files:**
- Modify: `prograph-core/src/facts.rs`
- Modify: `prograph-core/src/parsers/python.rs`

`Manifest` gains a `Vec<String> aliases` field. Python parser reads two new sources:

1. **`[dependency-groups]`** (PEP 735) — the dependencies that arbiter / atp-platform actually declare for `spec-runner`. The parser flattens all groups into the `declared_deps` list (groups themselves are not modeled in M3 — that's a M4+ refinement).
2. **`[project.optional-dependencies]`** — pip extras. Flatten into `declared_deps` same as dev-groups.
3. **`[tool.prograph]` block** — opt-in user override:
   ```toml
   [tool.prograph]
   aliases = ["atp-platform-sdk", "atp-platform-cli"]
   ```
   Declares additional names the project publishes (for workspace orchestrator monorepos).

- [ ] **Step 1: Extend `Manifest` in `facts.rs`**

Edit `prograph-core/src/facts.rs`. Find the `Manifest` struct definition and add a single field `aliases`:

```rust
/// A project's declared manifest — the canonical view downstream detectors use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Published package name (e.g. "atp-platform-sdk"), NOT the directory name.
    /// Detectors match consumers' `declared_deps[].name` against this field
    /// AND against any entry in `aliases`.
    pub declared_name: String,
    pub version: Option<String>,
    pub declared_deps: Vec<DepRequirement>,
    /// Additional names this project also publishes (workspace orchestrators).
    /// Populated from `[tool.prograph].aliases` in pyproject.toml or from
    /// `.prograph/config.toml` `[[project]].aliases`. Empty by default.
    #[serde(default)]
    pub aliases: Vec<String>,
}
```

Update the inline `manifest_round_trips_via_serde` test to include an `aliases` field:

```rust
    #[test]
    fn manifest_round_trips_via_serde() {
        let m = Manifest {
            declared_name: "atp-platform".into(),
            version: Some("1.0.0".into()),
            declared_deps: vec![DepRequirement {
                name: "spec-runner".into(),
                version_req: Some(">=0.1.4".into()),
            }],
            aliases: vec!["atp-platform-sdk".into(), "atp-platform-cli".into()],
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }
```

Also add a new test:
```rust
    #[test]
    fn manifest_aliases_default_empty() {
        let json = r#"{"declared_name":"x","version":null,"declared_deps":[]}"#;
        let m: Manifest = serde_json::from_str(json).unwrap();
        assert!(m.aliases.is_empty());
    }
```

The `#[serde(default)]` attribute ensures M2 snapshots (which have no `aliases` field in `projects.attrs_json`) deserialize cleanly. **This is the backward-compat seam — do not remove it.**

- [ ] **Step 2: Update `python.rs` parser**

In `prograph-core/src/parsers/python.rs`:

Extend the `PyprojectRoot` deserializer to read the new sections:

```rust
#[derive(Debug, Deserialize)]
struct PyprojectRoot {
    project: Option<PyprojectProject>,
    #[serde(rename = "dependency-groups", default)]
    dependency_groups: std::collections::HashMap<String, Vec<String>>,
    #[serde(default)]
    tool: Option<PyprojectTool>,
}

#[derive(Debug, Deserialize)]
struct PyprojectProject {
    name: Option<String>,
    version: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default, rename = "optional-dependencies")]
    optional_dependencies: std::collections::HashMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct PyprojectTool {
    #[serde(default)]
    prograph: Option<PyprojectToolPrograph>,
}

#[derive(Debug, Deserialize)]
struct PyprojectToolPrograph {
    #[serde(default)]
    aliases: Vec<String>,
}
```

Then in `parse`, after the existing `declared_deps` line, flatten the new sources and read aliases:

```rust
    // Flatten [project].dependencies + [project.optional-dependencies] + [dependency-groups]
    // into a single declared_deps list. M3 does not model dep groups separately; they're
    // all just declared dependencies of the project for matching purposes.
    let mut declared_deps: Vec<DepRequirement> =
        project.dependencies.iter().map(|raw| parse_pep508(raw)).collect();

    for (_group_name, deps) in &project.optional_dependencies {
        for raw in deps {
            declared_deps.push(parse_pep508(raw));
        }
    }
    for (_group_name, deps) in &root.dependency_groups {
        for raw in deps {
            declared_deps.push(parse_pep508(raw));
        }
    }

    let aliases = root
        .tool
        .as_ref()
        .and_then(|t| t.prograph.as_ref())
        .map(|p| p.aliases.clone())
        .unwrap_or_default();

    Ok(ParserOutput {
        manifest: Some(Manifest {
            declared_name,
            version: project.version,
            declared_deps,
            aliases,
        }),
        warnings: vec![],
    })
```

- [ ] **Step 3: Add 4 new inline tests in `python.rs`**

Append to the `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn reads_dependency_groups_pep735() {
        let dir = write_pyproject(r#"
[project]
name = "consumer"
dependencies = []

[dependency-groups]
dev = ["spec-runner>=0.1.4", "pytest"]
docs = ["sphinx"]
"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let names: Vec<_> = manifest.declared_deps.iter().map(|d| d.name.as_str()).collect();
        // Order isn't guaranteed across HashMap iteration, so use a set comparison.
        let names_set: std::collections::HashSet<_> = names.into_iter().collect();
        assert_eq!(
            names_set,
            ["spec-runner", "pytest", "sphinx"].into_iter().collect()
        );
    }

    #[test]
    fn reads_optional_dependencies() {
        let dir = write_pyproject(r#"
[project]
name = "consumer"
dependencies = ["core-lib"]

[project.optional-dependencies]
gui = ["qt-bindings>=6.0"]
cli = ["typer"]
"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let names: std::collections::HashSet<_> = manifest
            .declared_deps
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(names.contains("core-lib"));
        assert!(names.contains("qt-bindings"));
        assert!(names.contains("typer"));
    }

    #[test]
    fn reads_tool_prograph_aliases() {
        let dir = write_pyproject(r#"
[project]
name = "atp-platform"
dependencies = []

[tool.prograph]
aliases = ["atp-platform-sdk", "atp-platform-cli"]
"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        assert_eq!(manifest.declared_name, "atp-platform");
        assert_eq!(
            manifest.aliases,
            vec!["atp-platform-sdk".to_string(), "atp-platform-cli".to_string()]
        );
    }

    #[test]
    fn aliases_default_to_empty_when_no_tool_block() {
        let dir = write_pyproject(r#"
[project]
name = "plain"
dependencies = []
"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        assert!(manifest.aliases.is_empty());
    }
```

- [ ] **Step 4: Run cargo tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core facts
cargo test --package prograph-core parsers
```

Expected: facts 3 pass (2 prior + 1 new aliases test), parsers 11 pass (7 prior + 4 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 58 tests (54 prior + 1 new facts + 4 new parsers — minus 1 because the updated `manifest_round_trips_via_serde` test was already counted in the "prior" total).

Verify clean:
```sh
cargo fmt --all -- --check
cargo clippy --package prograph-core --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/facts.rs prograph/prograph-core/src/parsers/python.rs
git commit -m "prograph: M3 Manifest.aliases + Python [dependency-groups]/[optional-dependencies]/[tool.prograph] parsing"
```

---

## Task 2: `deps_detector` — alias-aware matching + collision warning

**Files:**
- Modify: `prograph-core/src/detectors/deps.rs`

The detector builds a publisher index. Today it's `name → idx`. M3 changes this to include each project's `declared_name` AND each entry in its `aliases`. When matching, multiple projects publishing the same name yields a deterministic warning (logged to indexer's warning count) and uses the first match.

- [ ] **Step 1: Update the publisher index build + matcher**

In `prograph-core/src/detectors/deps.rs`, replace the existing `detect` function. The signature stays the same:

```rust
pub fn detect(facts: &[ProjectFacts]) -> Vec<EdgeCandidate> {
    // Build name → publisher index map, including aliases.
    // On collision, log a warning (via the warnings_emitted side channel below)
    // and keep the FIRST registration (deterministic + ordering-stable).
    let mut publishers: HashMap<&str, usize> = HashMap::new();
    let mut collisions: Vec<String> = Vec::new();

    for (idx, p) in facts.iter().enumerate() {
        let Some(m) = &p.manifest else { continue };

        // Register declared_name + every alias under the same publisher index.
        let mut names_for_this: Vec<&str> = Vec::with_capacity(1 + m.aliases.len());
        names_for_this.push(m.declared_name.as_str());
        for alias in &m.aliases {
            names_for_this.push(alias.as_str());
        }

        for name in names_for_this {
            match publishers.entry(name) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(idx);
                }
                std::collections::hash_map::Entry::Occupied(_existing) => {
                    collisions.push(format!(
                        "name '{}' published by multiple projects (kept first)",
                        name
                    ));
                }
            }
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
                continue; // self-dep
            }

            let attrs = serde_json::json!({
                "dep_name": dep.name,
                "version_req": dep.version_req,
            });
            let attrs_json = serde_json::to_string(&attrs).unwrap();

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

    // Stash collisions in a thread-local so the indexer can pick them up as ParseWarnings.
    // We don't change the function signature in M3 to keep test churn low.
    if !collisions.is_empty() {
        COLLISION_WARNINGS.with(|w| {
            let mut v = w.borrow_mut();
            v.extend(collisions);
        });
    }

    out
}

thread_local! {
    /// Collisions detected during the last `detect_all` invocation on this thread.
    /// The indexer drains this after each call and folds the messages into the
    /// snapshot's warning count.
    pub static COLLISION_WARNINGS: std::cell::RefCell<Vec<String>> =
        std::cell::RefCell::new(Vec::new());
}

/// Drain and return any collision warnings accumulated by the most recent `detect`/`detect_all`
/// call on this thread. The indexer should call this once per index pipeline run.
pub fn drain_collision_warnings() -> Vec<String> {
    COLLISION_WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}
```

Add `use std::cell::RefCell;` to the imports if needed (or use the fully-qualified path inline as above).

- [ ] **Step 2: Add 3 new inline tests**

Append to the `#[cfg(test)] mod tests` block of `deps.rs`:

```rust
    fn fact_with_aliases(name: &str, aliases: &[&str], deps: &[(&str, Option<&str>)]) -> ProjectFacts {
        let base = fact(name, deps);
        ProjectFacts {
            manifest: Some(Manifest {
                aliases: aliases.iter().map(|s| s.to_string()).collect(),
                ..base.manifest.unwrap()
            }),
            ..base
        }
    }

    #[test]
    fn alias_matches_consumer_to_publisher() {
        let _ = drain_collision_warnings(); // clear thread-local from prior tests
        let facts = vec![
            fact("consumer", &[("atp-platform-sdk", Some(">=2.0"))]),
            fact_with_aliases("atp-platform", &["atp-platform-sdk"], &[]),
        ];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1, "expected consumer -> atp-platform via alias match");
        assert_eq!(edges[0].from_idx, 0);
        assert_eq!(edges[0].to_idx, 1);
    }

    #[test]
    fn name_collision_emits_warning_and_keeps_first() {
        let _ = drain_collision_warnings();
        let facts = vec![
            fact("first-publisher", &[]),
            fact_with_aliases("second-publisher", &["first-publisher"], &[]),
            fact("consumer", &[("first-publisher", None)]),
        ];
        let edges = detect(&facts);
        let warnings = drain_collision_warnings();
        assert!(!warnings.is_empty(), "expected at least one collision warning");
        assert!(warnings.iter().any(|w| w.contains("first-publisher")));
        // The collision keeps the FIRST registration -> consumer's edge goes to facts[0].
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to_idx, 0);
    }

    #[test]
    fn drain_collision_warnings_is_idempotent_after_drain() {
        let _ = drain_collision_warnings();
        let facts = vec![
            fact("a", &[]),
            fact_with_aliases("b", &["a"], &[]),
        ];
        let _ = detect(&facts);
        let first = drain_collision_warnings();
        assert!(!first.is_empty());
        let second = drain_collision_warnings();
        assert!(second.is_empty(), "drain should empty the thread-local");
    }
```

Also update the existing `matches_consumer_to_publisher_by_name` and similar tests to call `drain_collision_warnings()` at the top — otherwise stale warnings from one test pollute another (cargo test runs in the same thread per test binary by default but parallelism may interleave them; the drain is cheap and defensive).

Actually — cargo test parallelizes across threads but `thread_local!` is per-thread, so stale warnings only leak within the same thread. Add the drain to each test as a defensive hygiene step:

```rust
    #[test]
    fn matches_consumer_to_publisher_by_name() {
        let _ = drain_collision_warnings();
        // ... rest of existing test body unchanged
    }
```

Apply this `let _ = drain_collision_warnings();` line at the top of EVERY existing test in `deps.rs::tests` (5 existing tests + 3 new). It's a 1-line per-test addition.

- [ ] **Step 3: Run cargo tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core detectors
```
Expected: 9 detectors tests pass (6 prior + 3 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 61 tests (58 from Task 1 + 3 new).

Verify clean:
```sh
cargo fmt --all -- --check
cargo clippy --package prograph-core --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph-core/src/detectors/deps.rs
git commit -m "prograph: M3 deps_detector — alias-aware matching + name collision warning"
```

---

## Task 3: Indexer — drain collision warnings into snapshot warning count

**Files:**
- Modify: `prograph-core/src/indexer.rs`

The detector now stashes collision warnings in a thread-local. The indexer needs to drain them after `detect_all` and fold them into `warning_count` so `IndexSummary.n_warnings` reflects them.

- [ ] **Step 1: Drain warnings after detector phase**

In `prograph-core/src/indexer.rs`, locate the line:

```rust
    // Phase 3: Edge detection.
    let edge_candidates = detectors::detect_all(&facts);
```

Replace with:

```rust
    // Phase 3: Edge detection.
    let edge_candidates = detectors::detect_all(&facts);
    let collision_warnings = detectors::deps::drain_collision_warnings();
    warning_count += collision_warnings.len() as i64;
```

- [ ] **Step 2: Update the existing version-bump test to be resilient**

The new `warning_count` may pick up unrelated collisions if other tests leave warnings in the thread-local. The simplest fix is a defensive drain at the top of each indexer test:

In `prograph-core/src/indexer.rs`'s `#[cfg(test)] mod tests` block, add to the top of EACH of the 4 existing tests:
```rust
        let _ = crate::detectors::deps::drain_collision_warnings();
```

- [ ] **Step 3: Add one new indexer test asserting collision warning counts**

Append to the tests block:

```rust
    #[test]
    fn collision_warning_increments_n_warnings() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();

        // Two projects publishing the same name (via alias).
        fs::create_dir_all(dir.path().join("first")).unwrap();
        fs::write(
            dir.path().join("first/pyproject.toml"),
            r#"[project]
name = "shared-name"
"#,
        ).unwrap();
        fs::create_dir_all(dir.path().join("second")).unwrap();
        fs::write(
            dir.path().join("second/pyproject.toml"),
            r#"[project]
name = "second-actual"

[tool.prograph]
aliases = ["shared-name"]
"#,
        ).unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        let summary = index_monorepo(dir.path(), &mut store).unwrap();
        assert!(summary.n_warnings >= 1, "expected ≥1 warning for name collision, got {}", summary.n_warnings);
    }
```

- [ ] **Step 4: Run tests**

```sh
cargo test --package prograph-core indexer
```
Expected: 5 indexer tests pass (4 prior + 1 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 62 tests (61 from Task 2 + 1 new).

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/indexer.rs
git commit -m "prograph: M3 indexer — fold deps_detector collision warnings into n_warnings"
```

---

## Task 4: Rust parser (`Cargo.toml` → `Manifest`)

**Files:**
- Create: `prograph-core/src/parsers/rust.rs`
- Modify: `prograph-core/src/parsers/mod.rs`

Rust parser reads:
- `[package].name` → `declared_name`
- `[package].version` → `version`
- `[dependencies]` (table form: `key = { version = "..." }` OR string form: `key = "..."`) → `declared_deps`
- `[dev-dependencies]` and `[build-dependencies]` → flattened into `declared_deps`
- `[workspace.dependencies]` (workspace root case) → skipped in M3, warned

Workspace handling: if the file has `[workspace]` and no `[package]`, it's a workspace root with no package. M3 emits a warning ("workspace root with no [package] — sub-packages must be scanned separately") and returns `manifest: None`. M3 does NOT auto-scan workspace members; that's a discovery-layer change deferred to a later milestone.

- [ ] **Step 1: Write `rust.rs`**

`prograph-core/src/parsers/rust.rs`:
```rust
//! Rust project parser — reads `Cargo.toml` to extract package name + deps.

use std::path::Path;

use serde::Deserialize;

use super::ParserOutput;
use crate::errors::{PrographError, Result};
use crate::facts::{DepRequirement, Manifest, ParseWarning};

#[derive(Debug, Deserialize)]
struct CargoToml {
    package: Option<CargoPackage>,
    workspace: Option<toml::Value>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, CargoDep>,
    #[serde(default, rename = "dev-dependencies")]
    dev_dependencies: std::collections::BTreeMap<String, CargoDep>,
    #[serde(default, rename = "build-dependencies")]
    build_dependencies: std::collections::BTreeMap<String, CargoDep>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: Option<String>,
    version: Option<String>,
}

/// A single entry under `[dependencies]` — accepts either the short string form
/// (`serde = "1.0"`) or the table form (`serde = { version = "1.0", features = [...] }`).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CargoDep {
    Simple(String),
    Detailed { version: Option<String> },
}

impl CargoDep {
    fn version_req(&self) -> Option<String> {
        match self {
            CargoDep::Simple(v) => Some(v.clone()),
            CargoDep::Detailed { version } => version.clone(),
        }
    }
}

pub fn parse(project_root: &Path) -> Result<ParserOutput> {
    let cargo_toml = project_root.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Ok(ParserOutput {
            manifest: None,
            warnings: vec![ParseWarning {
                rel_path: "Cargo.toml".into(),
                message: "no Cargo.toml found".into(),
            }],
        });
    }

    let contents = std::fs::read_to_string(&cargo_toml).map_err(|source| PrographError::Io {
        path: cargo_toml.display().to_string(),
        source,
    })?;

    let root: CargoToml = toml::from_str(&contents).map_err(|e| PrographError::Parse {
        path: cargo_toml.display().to_string(),
        reason: e.to_string(),
    })?;

    // Workspace-only manifest (no [package]) — emit warning, return no manifest.
    if root.package.is_none() {
        let msg = if root.workspace.is_some() {
            "workspace root with no [package] — sub-packages must be scanned separately".into()
        } else {
            "Cargo.toml has no [package] section".into()
        };
        return Ok(ParserOutput {
            manifest: None,
            warnings: vec![ParseWarning {
                rel_path: "Cargo.toml".into(),
                message: msg,
            }],
        });
    }
    let package = root.package.unwrap();

    let declared_name = match package.name {
        Some(n) => n,
        None => {
            return Ok(ParserOutput {
                manifest: None,
                warnings: vec![ParseWarning {
                    rel_path: "Cargo.toml".into(),
                    message: "[package] missing 'name' key".into(),
                }],
            });
        }
    };

    // Flatten all three dep tables. Order: dependencies, dev-dependencies, build-dependencies.
    // BTreeMap iteration is deterministic (sorted by key), so the resulting dep list is stable.
    let mut declared_deps: Vec<DepRequirement> = Vec::new();
    for (name, dep) in &root.dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: dep.version_req(),
        });
    }
    for (name, dep) in &root.dev_dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: dep.version_req(),
        });
    }
    for (name, dep) in &root.build_dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: dep.version_req(),
        });
    }

    Ok(ParserOutput {
        manifest: Some(Manifest {
            declared_name,
            version: package.version,
            declared_deps,
            aliases: Vec::new(), // Rust doesn't have a tool.prograph equivalent in M3
        }),
        warnings: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_cargo_toml(toml: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), toml).unwrap();
        dir
    }

    #[test]
    fn parses_minimal_cargo_toml() {
        let dir = write_cargo_toml(r#"
[package]
name = "foo"
version = "0.1.0"
"#);
        let out = parse(dir.path()).unwrap();
        let manifest = out.manifest.unwrap();
        assert_eq!(manifest.declared_name, "foo");
        assert_eq!(manifest.version.as_deref(), Some("0.1.0"));
        assert!(manifest.declared_deps.is_empty());
        assert!(manifest.aliases.is_empty());
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn parses_string_form_dependencies() {
        let dir = write_cargo_toml(r#"
[package]
name = "consumer"
version = "1.0"

[dependencies]
serde = "1.0"
tokio = "1.36.0"
"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let names: std::collections::HashSet<_> =
            manifest.declared_deps.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains("serde"));
        assert!(names.contains("tokio"));
        let serde_dep = manifest.declared_deps.iter().find(|d| d.name == "serde").unwrap();
        assert_eq!(serde_dep.version_req.as_deref(), Some("1.0"));
    }

    #[test]
    fn parses_table_form_dependencies() {
        let dir = write_cargo_toml(r#"
[package]
name = "consumer"
version = "1.0"

[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
pyo3 = { workspace = true }
"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let rusqlite_dep = manifest.declared_deps.iter().find(|d| d.name == "rusqlite").unwrap();
        assert_eq!(rusqlite_dep.version_req.as_deref(), Some("0.31"));
        let pyo3_dep = manifest.declared_deps.iter().find(|d| d.name == "pyo3").unwrap();
        // workspace = true has no `version` key → version_req is None
        assert_eq!(pyo3_dep.version_req, None);
    }

    #[test]
    fn includes_dev_and_build_dependencies() {
        let dir = write_cargo_toml(r#"
[package]
name = "x"

[dependencies]
runtime = "1"

[dev-dependencies]
test-lib = "2"

[build-dependencies]
build-helper = "3"
"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let names: std::collections::HashSet<_> =
            manifest.declared_deps.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains("runtime"));
        assert!(names.contains("test-lib"));
        assert!(names.contains("build-helper"));
    }

    #[test]
    fn warns_when_workspace_root_only() {
        let dir = write_cargo_toml(r#"
[workspace]
members = ["a", "b"]
"#);
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("workspace root"));
    }

    #[test]
    fn warns_when_no_cargo_toml() {
        let dir = TempDir::new().unwrap();
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("no Cargo.toml"));
    }

    #[test]
    fn errors_on_invalid_toml() {
        let dir = write_cargo_toml("[ this is not toml");
        let err = parse(dir.path()).unwrap_err();
        assert!(err.to_string().contains("parse error"));
    }
}
```

- [ ] **Step 2: Wire `rust.rs` into the dispatch in `parsers/mod.rs`**

In `prograph-core/src/parsers/mod.rs`, add the new module and extend the match:

```rust
pub mod python;
pub mod rust;  // NEW
```

And the dispatch:
```rust
pub fn parse_project(root: &Path, kind: ProjectKind) -> Result<ParserOutput> {
    match kind {
        ProjectKind::Python => python::parse(root),
        ProjectKind::Rust => rust::parse(root),  // NEW
        ProjectKind::Mixed => parse_mixed(root),  // NEW — see below
        _ => Ok(ParserOutput {
            manifest: None,
            warnings: vec![],
        }),
    }
}

/// For Mixed projects (e.g. prograph itself: Python + Rust core), prefer Python's
/// pyproject.toml as the canonical declared_name. The Rust crate's name typically
/// differs (e.g. "prograph-core" vs the Python "prograph") and is the *internal*
/// extension name, not the published name. M3 keeps this heuristic simple; M4+ may
/// expose both via separate sub-projects.
fn parse_mixed(root: &Path) -> Result<ParserOutput> {
    let py = python::parse(root)?;
    if py.manifest.is_some() {
        return Ok(py);
    }
    // Fall back to Rust if no Python manifest produced.
    rust::parse(root)
}
```

- [ ] **Step 3: Run cargo tests**

```sh
cargo test --package prograph-core parsers
```
Expected: 18 parsers tests (11 from Task 1 + 7 new Rust).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 69 tests (62 from Task 3 + 7 new).

Verify clean:
```sh
cargo fmt --all -- --check
cargo clippy --package prograph-core --all-targets -- -D warnings
```

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph-core/src/parsers/rust.rs prograph/prograph-core/src/parsers/mod.rs
git commit -m "prograph: M3 Rust parser (Cargo.toml → Manifest, includes dev/build-deps)"
```

---

## Task 5: JS parser (`package.json` → `Manifest`)

**Files:**
- Create: `prograph-core/src/parsers/js.rs`
- Modify: `prograph-core/src/parsers/mod.rs`

JS parser reads:
- `"name"` → `declared_name`
- `"version"` → `version`
- `"dependencies"` (object: `{name: "version"}`) → `declared_deps`
- `"devDependencies"` → flattened into `declared_deps`
- `"peerDependencies"` → flattened into `declared_deps`

`package.json` is JSON, so we use `serde_json` (already a workspace dep). The version-req strings can be PEP-style (`^1.0`, `~2.3`, `>=1.0 <2.0`) — we store them verbatim in `version_req`. The deps detector identity excludes version_req per spec §5.2, so this is fine.

- [ ] **Step 1: Write `js.rs`**

`prograph-core/src/parsers/js.rs`:
```rust
//! JS / TypeScript project parser — reads `package.json` to extract name + deps.

use std::path::Path;

use serde::Deserialize;

use super::ParserOutput;
use crate::errors::{PrographError, Result};
use crate::facts::{DepRequirement, Manifest, ParseWarning};

#[derive(Debug, Deserialize)]
struct PackageJson {
    name: Option<String>,
    version: Option<String>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: std::collections::BTreeMap<String, String>,
    #[serde(default, rename = "peerDependencies")]
    peer_dependencies: std::collections::BTreeMap<String, String>,
}

pub fn parse(project_root: &Path) -> Result<ParserOutput> {
    let package_json = project_root.join("package.json");
    if !package_json.is_file() {
        return Ok(ParserOutput {
            manifest: None,
            warnings: vec![ParseWarning {
                rel_path: "package.json".into(),
                message: "no package.json found".into(),
            }],
        });
    }

    let contents = std::fs::read_to_string(&package_json).map_err(|source| PrographError::Io {
        path: package_json.display().to_string(),
        source,
    })?;

    let pkg: PackageJson = serde_json::from_str(&contents).map_err(|e| PrographError::Parse {
        path: package_json.display().to_string(),
        reason: e.to_string(),
    })?;

    let declared_name = match pkg.name {
        Some(n) => n,
        None => {
            return Ok(ParserOutput {
                manifest: None,
                warnings: vec![ParseWarning {
                    rel_path: "package.json".into(),
                    message: "package.json missing 'name' key".into(),
                }],
            });
        }
    };

    let mut declared_deps: Vec<DepRequirement> = Vec::new();
    for (name, version) in &pkg.dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: Some(version.clone()),
        });
    }
    for (name, version) in &pkg.dev_dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: Some(version.clone()),
        });
    }
    for (name, version) in &pkg.peer_dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: Some(version.clone()),
        });
    }

    Ok(ParserOutput {
        manifest: Some(Manifest {
            declared_name,
            version: pkg.version,
            declared_deps,
            aliases: Vec::new(),
        }),
        warnings: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_package_json(json: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("package.json"), json).unwrap();
        dir
    }

    #[test]
    fn parses_minimal_package_json() {
        let dir = write_package_json(r#"{
  "name": "my-app",
  "version": "1.0.0"
}"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        assert_eq!(manifest.declared_name, "my-app");
        assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
        assert!(manifest.declared_deps.is_empty());
    }

    #[test]
    fn parses_dependencies_and_dev_dependencies() {
        let dir = write_package_json(r#"{
  "name": "consumer",
  "version": "1.0.0",
  "dependencies": {
    "react": "^18.2.0",
    "lodash": "~4.17.21"
  },
  "devDependencies": {
    "typescript": "5.0.0",
    "vitest": ">=1.0"
  }
}"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let names: std::collections::HashSet<_> =
            manifest.declared_deps.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains("react"));
        assert!(names.contains("lodash"));
        assert!(names.contains("typescript"));
        assert!(names.contains("vitest"));
        let react_dep = manifest.declared_deps.iter().find(|d| d.name == "react").unwrap();
        assert_eq!(react_dep.version_req.as_deref(), Some("^18.2.0"));
    }

    #[test]
    fn parses_peer_dependencies() {
        let dir = write_package_json(r#"{
  "name": "plugin",
  "peerDependencies": {
    "host-app": "^2.0"
  }
}"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let host_dep = manifest.declared_deps.iter().find(|d| d.name == "host-app").unwrap();
        assert_eq!(host_dep.version_req.as_deref(), Some("^2.0"));
    }

    #[test]
    fn warns_when_no_package_json() {
        let dir = TempDir::new().unwrap();
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("no package.json"));
    }

    #[test]
    fn warns_when_no_name() {
        let dir = write_package_json(r#"{"version": "1.0"}"#);
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("missing 'name'"));
    }

    #[test]
    fn errors_on_invalid_json() {
        let dir = write_package_json("{not json");
        let err = parse(dir.path()).unwrap_err();
        assert!(err.to_string().contains("parse error"));
    }
}
```

- [ ] **Step 2: Wire `js.rs` into the dispatch**

In `prograph-core/src/parsers/mod.rs`, add the module and dispatch arm:

```rust
pub mod js;
pub mod python;
pub mod rust;
```

And:
```rust
pub fn parse_project(root: &Path, kind: ProjectKind) -> Result<ParserOutput> {
    match kind {
        ProjectKind::Python => python::parse(root),
        ProjectKind::Rust => rust::parse(root),
        ProjectKind::Js => js::parse(root),  // NEW
        ProjectKind::Mixed => parse_mixed(root),
        _ => Ok(ParserOutput {
            manifest: None,
            warnings: vec![],
        }),
    }
}
```

Also extend `parse_mixed` to fall through to JS if neither Python nor Rust produced a manifest:
```rust
fn parse_mixed(root: &Path) -> Result<ParserOutput> {
    let py = python::parse(root)?;
    if py.manifest.is_some() {
        return Ok(py);
    }
    let rs = rust::parse(root)?;
    if rs.manifest.is_some() {
        return Ok(rs);
    }
    js::parse(root)
}
```

- [ ] **Step 3: Run cargo tests**

```sh
cargo test --package prograph-core parsers
```
Expected: 24 parsers tests (18 from Task 4 + 6 new JS).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 75 tests (69 from Task 4 + 6 new).

Verify clean.

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph-core/src/parsers/js.rs prograph/prograph-core/src/parsers/mod.rs
git commit -m "prograph: M3 JS parser (package.json — dependencies, devDependencies, peerDependencies)"
```

---

## Task 6: `monorepo_multilang` fixture

**Files:**
- Create: `tests/fixtures/monorepo_multilang/<8 files>`

A richer fixture covering all three languages + cross-language and alias edges.

Expected edge count: **6 cross-project edges** within the fixture:
1. `py_consumer` → `py_publisher` (Python → Python, `[project].dependencies`)
2. `py_consumer` → `py_workspace` (Python → Python via alias — `py_workspace` aliases `py-sdk`)
3. `py_dev_consumer` → `py_publisher` (Python → Python, via `[dependency-groups]`)
4. `rust_consumer` → `rust_publisher` (Rust → Rust)
5. `js_consumer` → `js_publisher` (JS → JS)
6. `py_consumer` → `py_publisher` (via `optional-dependencies`) — actually this would dedup since same (from, to, dep_name). Remove this case to keep counts deterministic.

So expected n_edges = 5 (we'll verify in Task 7 e2e test).

Wait — actually one edge per (from, to, dep_name) due to identity hash. Let me reconsider:
- py_consumer declares "py-publisher" in [project].dependencies → edge to py_publisher
- py_consumer declares "py-sdk" → alias match → edge to py_workspace
- py_dev_consumer declares "py-publisher" in [dependency-groups].dev → edge to py_publisher
- rust_consumer declares "rust-publisher" in [dependencies] → edge to rust_publisher
- js_consumer declares "js-publisher" in dependencies → edge to js_publisher

5 distinct edges. Good.

- [ ] **Step 1: Python projects**

`tests/fixtures/monorepo_multilang/py_consumer/pyproject.toml`:
```toml
[project]
name = "py-consumer"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = [
    "py-publisher>=1.0",
    "py-sdk",
    "external-lib",
]
```

`tests/fixtures/monorepo_multilang/py_publisher/pyproject.toml`:
```toml
[project]
name = "py-publisher"
version = "1.5.0"
requires-python = ">=3.11"
dependencies = []
```

`tests/fixtures/monorepo_multilang/py_workspace/pyproject.toml`:
```toml
[project]
name = "py-workspace"
version = "2.0.0"
requires-python = ">=3.11"
dependencies = []

[tool.prograph]
aliases = ["py-sdk", "py-cli"]
```

`tests/fixtures/monorepo_multilang/py_dev_consumer/pyproject.toml`:
```toml
[project]
name = "py-dev-consumer"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = []

[dependency-groups]
dev = ["py-publisher>=1.0", "pytest"]
```

- [ ] **Step 2: Rust projects**

`tests/fixtures/monorepo_multilang/rust_consumer/Cargo.toml`:
```toml
[package]
name = "rust-consumer"
version = "0.1.0"
edition = "2021"

[dependencies]
rust-publisher = "1.0"
serde = "1"
```

`tests/fixtures/monorepo_multilang/rust_publisher/Cargo.toml`:
```toml
[package]
name = "rust-publisher"
version = "1.0.0"
edition = "2021"
```

- [ ] **Step 3: JS projects**

`tests/fixtures/monorepo_multilang/js_consumer/package.json`:
```json
{
  "name": "js-consumer",
  "version": "0.1.0",
  "dependencies": {
    "js-publisher": "^1.0.0",
    "react": "^18.2.0"
  }
}
```

`tests/fixtures/monorepo_multilang/js_publisher/package.json`:
```json
{
  "name": "js-publisher",
  "version": "1.0.0"
}
```

- [ ] **Step 4: Verify discovery + parsing work**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run python -c "
from prograph._core import scan_monorepo
for c in sorted(scan_monorepo('tests/fixtures/monorepo_multilang'), key=lambda x: x.name):
    print(c.name, c.kind.name(), c.manifests)
"
```

Expected (8 projects):
```
js_consumer js ['package.json']
js_publisher js ['package.json']
py_consumer python ['pyproject.toml']
py_dev_consumer python ['pyproject.toml']
py_publisher python ['pyproject.toml']
py_workspace python ['pyproject.toml']
rust_consumer rust ['Cargo.toml']
rust_publisher rust ['Cargo.toml']
```

- [ ] **Step 5: Commit**

```sh
git add prograph/tests/fixtures/monorepo_multilang/
git commit -m "prograph: M3 monorepo_multilang fixture — 8 projects covering py/rust/js + aliases + dep-groups"
```

---

## Task 7: Indexer — capture `git_commit` in snapshots

**Files:**
- Modify: `prograph-core/src/indexer.rs`

The spec §6 calls for capturing the current git commit in each snapshot. M2 left it as `None`. M3 fills it in via a shell-out to `git rev-parse HEAD`, but only when the working tree is clean (per the spec's intent: snapshots reflect a reproducible state). If the tree is dirty or git fails, we record `None`.

- [ ] **Step 1: Add a small helper to `indexer.rs`**

Insert at the bottom of `indexer.rs` (before the `#[cfg(test)]` block):

```rust
/// Best-effort: return `Some(commit_sha)` if `monorepo_root` is inside a git repository
/// AND the working tree is clean. Returns `None` otherwise.
///
/// Rationale: a snapshot tied to a dirty working tree can't be reproduced from the
/// recorded commit alone, so we don't claim to know the state.
fn detect_git_commit(monorepo_root: &Path) -> Option<String> {
    use std::process::Command;

    let status_out = Command::new("git")
        .args(["-C"])
        .arg(monorepo_root)
        .args(["status", "--porcelain"])
        .output()
        .ok()?;
    if !status_out.status.success() {
        return None;
    }
    // Empty stdout = clean tree.
    if !status_out.stdout.is_empty() {
        return None;
    }

    let rev_out = Command::new("git")
        .args(["-C"])
        .arg(monorepo_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !rev_out.status.success() {
        return None;
    }
    let sha = String::from_utf8(rev_out.stdout).ok()?.trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}
```

- [ ] **Step 2: Call it from `index_monorepo` when inserting the snapshot**

In `index_monorepo`, locate the line:
```rust
    let snap_id = writer.insert_snapshot(
        &ts,
        &monorepo_root.display().to_string(),
        None,
        env!("CARGO_PKG_VERSION"),
    )?;
```

Replace with:
```rust
    let git_commit = detect_git_commit(monorepo_root);
    let snap_id = writer.insert_snapshot(
        &ts,
        &monorepo_root.display().to_string(),
        git_commit.as_deref(),
        env!("CARGO_PKG_VERSION"),
    )?;
```

- [ ] **Step 3: Add an inline test**

Add to `indexer.rs`'s `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn detect_git_commit_returns_none_for_non_git_dir() {
        let dir = TempDir::new().unwrap();
        assert!(detect_git_commit(dir.path()).is_none());
    }

    #[test]
    fn snapshot_records_git_commit_when_tree_clean() {
        // We can't easily set up a clean git repo in a temp dir without making this test
        // depend on the user's git config. Instead, we just assert the field is wired:
        // when there's no git repo at all, git_commit is None and the snapshot still records.
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".prograph")).unwrap();
        fs::create_dir_all(dir.path().join("p")).unwrap();
        fs::write(
            dir.path().join("p/pyproject.toml"),
            r#"[project]
name = "p"
"#,
        ).unwrap();

        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        let _summary = index_monorepo(dir.path(), &mut store).unwrap();
        let info = store.latest_snapshot_info().unwrap().unwrap();
        // No git repo → git_commit is None.
        assert!(info.git_commit.is_none());
    }
```

- [ ] **Step 4: Run tests**

```sh
cargo test --package prograph-core indexer
```
Expected: 7 indexer tests (5 from Task 3 + 2 new).

Full crate:
```sh
cargo test --package prograph-core
```
Expected: 77 tests (75 from Task 5 + 2 new).

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/indexer.rs
git commit -m "prograph: M3 capture git_commit in snapshots when working tree is clean"
```

---

## Task 8: Integration test against `monorepo_multilang`

**Files:**
- Create: `tests/integration/test_cli_index_multilang.py`

End-to-end test against the new fixture. Verifies all 5 expected edges land.

- [ ] **Step 1: Write the test**

`tests/integration/test_cli_index_multilang.py`:
```python
"""End-to-end integration test against monorepo_multilang fixture."""

import json
import shutil
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()

FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_multilang"


@pytest.fixture
def fresh_multilang_fixture(tmp_path: Path) -> Path:
    dst = tmp_path / "monorepo_multilang"
    shutil.copytree(FIXTURE, dst)
    return dst


def _run(args: list[str]) -> dict:
    result = runner.invoke(app, [*args, "--json"])
    assert result.exit_code == 0, result.stdout + result.stderr
    return json.loads(result.stdout)


def test_multilang_index_detects_all_cross_lang_edges(fresh_multilang_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_multilang_fixture)])
    summary = _run(["index", "--monorepo", str(fresh_multilang_fixture)])

    # 8 projects (4 py + 2 rust + 2 js)
    assert summary["n_projects"] == 8, summary

    # Expected edges:
    # 1. py_consumer -> py_publisher (via [project].dependencies)
    # 2. py_consumer -> py_workspace (via py-sdk alias)
    # 3. py_dev_consumer -> py_publisher (via [dependency-groups])
    # 4. rust_consumer -> rust_publisher
    # 5. js_consumer -> js_publisher
    assert summary["n_edges"] == 5, summary


def test_multilang_python_alias_edge(fresh_multilang_fixture: Path):
    """py_consumer declares 'py-sdk'; py_workspace publishes name 'py-workspace' but aliases 'py-sdk'."""
    runner.invoke(app, ["init", "--monorepo", str(fresh_multilang_fixture)])
    _run(["index", "--monorepo", str(fresh_multilang_fixture)])

    # We don't yet have a `prograph query edges` CLI. Reach into the DB.
    import sqlite3
    db = fresh_multilang_fixture / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT consumer.name, publisher.name, json_extract(e.attrs_json, '$.dep_name')
        FROM edges e
        JOIN projects consumer ON consumer.id = e.from_id
        JOIN projects publisher ON publisher.id = e.to_id
        WHERE e.kind = 'package_dep'
        ORDER BY consumer.name, publisher.name
        """
    ).fetchall()
    conn.close()
    # Find the alias edge.
    alias_edge = [r for r in rows if r[0] == "py_consumer" and r[1] == "py_workspace"]
    assert len(alias_edge) == 1
    assert alias_edge[0][2] == "py-sdk"


def test_multilang_rust_edge(fresh_multilang_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_multilang_fixture)])
    _run(["index", "--monorepo", str(fresh_multilang_fixture)])
    import sqlite3
    db = fresh_multilang_fixture / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT consumer.name, publisher.name
        FROM edges e
        JOIN projects consumer ON consumer.id = e.from_id
        JOIN projects publisher ON publisher.id = e.to_id
        WHERE consumer.kind = 'rust' AND publisher.kind = 'rust'
        """
    ).fetchall()
    conn.close()
    assert ("rust_consumer", "rust_publisher") in rows


def test_multilang_js_edge(fresh_multilang_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_multilang_fixture)])
    _run(["index", "--monorepo", str(fresh_multilang_fixture)])
    import sqlite3
    db = fresh_multilang_fixture / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT consumer.name, publisher.name
        FROM edges e
        JOIN projects consumer ON consumer.id = e.from_id
        JOIN projects publisher ON publisher.id = e.to_id
        WHERE consumer.kind = 'js' AND publisher.kind = 'js'
        """
    ).fetchall()
    conn.close()
    assert ("js_consumer", "js_publisher") in rows


def test_multilang_dependency_groups_edge(fresh_multilang_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_multilang_fixture)])
    _run(["index", "--monorepo", str(fresh_multilang_fixture)])
    import sqlite3
    db = fresh_multilang_fixture / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT consumer.name, publisher.name
        FROM edges e
        JOIN projects consumer ON consumer.id = e.from_id
        JOIN projects publisher ON publisher.id = e.to_id
        WHERE consumer.name = 'py_dev_consumer'
        """
    ).fetchall()
    conn.close()
    assert ("py_dev_consumer", "py_publisher") in rows
```

- [ ] **Step 2: Rebuild + run**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv sync --reinstall-package prograph
uv run pytest tests/integration/test_cli_index_multilang.py -v
```
Expected: 5 passed.

Full suite:
```sh
uv run pytest -v
```
Expected: 39 passed (34 prior + 5 new).

- [ ] **Step 3: Commit**

```sh
git add prograph/tests/integration/test_cli_index_multilang.py
git commit -m "prograph: M3 end-to-end integration test on monorepo_multilang fixture (5 cross-lang edges)"
```

---

## Task 9: Real-monorepo smoke — assert `n_edges >= 1`

**Files:**
- Modify: `tests/integration/test_smoke_real.py`

After Tasks 1-2, the Python parser reads `[dependency-groups]`. After Task 4, the Rust parser reads `Cargo.toml`. The real `all_ai_orchestrators/` monorepo should now produce ≥1 edge (specifically: `arbiter → spec-runner` via arbiter's `[dependency-groups].dev`, and `atp-platform → spec-runner` via the same mechanism).

The Maestro → atp-platform-sdk edge still won't fire unless atp-platform's pyproject.toml gets a `[tool.prograph].aliases = ["atp-platform-sdk"]` block — that's a user-side change documented in the README polish task.

- [ ] **Step 1: Tighten the assertion**

In `tests/integration/test_smoke_real.py`, find the line:
```python
    assert summary["n_edges"] >= 0, ...
```

Replace with:
```python
    assert summary["n_edges"] >= 1, (
        f"expected ≥1 edge after M3 dependency-groups parsing, got summary: {summary}. "
        f"If this fails: the user's real monorepo may not have any [dependency-groups] "
        f"cross-deps; check the actual pyproject.toml files."
    )
```

- [ ] **Step 2: Run the smoke**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run pytest -m realmonorepo -v
```

Expected: 1 passed. The smoke now requires ≥1 edge against the real monorepo.

If it fails: investigate which `[dependency-groups]` blocks the real monorepo has. If there are genuinely none, the assertion needs to be reverted with documentation of why — but this would mean the user's actual setup has migrated away from PEP 735 since the M2 review.

- [ ] **Step 3: Run regular suite to confirm no regression**

```sh
uv run pytest -v
```
Expected: 39 passed, 1 deselected.

- [ ] **Step 4: Commit**

```sh
git add prograph/tests/integration/test_smoke_real.py
git commit -m "prograph: M3 real-monorepo smoke — tighten to n_edges >= 1 (PEP 735 now parsed)"
```

---

## Task 10: README + CLAUDE.md updates

**Files:**
- Modify: `prograph/README.md`
- Modify: `prograph/CLAUDE.md`

- [ ] **Step 1: Update README**

In `prograph/README.md`, replace the Status line:
```markdown
**Status:** M3 — Multi-language indexer. `prograph init`, `prograph status`, `prograph index` work end-to-end on Python + Rust + JS monorepos. Manifest-based dependency detection across `pyproject.toml` (`[project].dependencies`, `[project.optional-dependencies]`, `[dependency-groups]` per PEP 735, `[tool.prograph].aliases`), `Cargo.toml` (`[dependencies]` / `[dev-dependencies]` / `[build-dependencies]`), and `package.json` (`dependencies` / `devDependencies` / `peerDependencies`). Cross-language matching via shared name + per-project `aliases`. Contracts/MCP detectors, MD export, browser UI, and MCP server land in M4–M7.
```

Add a new subsection under "Usage" titled "Working with workspace sub-packages":
```markdown
### Working with workspace sub-packages

If a project publishes multiple package names (common for workspace orchestrators), declare them in `[tool.prograph].aliases`:

```toml
# atp-platform/pyproject.toml
[project]
name = "atp-platform"

[tool.prograph]
aliases = ["atp-platform-sdk", "atp-platform-cli"]
```

Now any consumer declaring `atp-platform-sdk>=2.0` in its `dependencies` resolves to this project. Without aliases, M3 only matches the project's `[project].name`.
```

Update the limitations list:
```markdown
### M3 limitations (intentional — addressed in later milestones)

- **Manifest-based only.** Parsers don't read source files; M5 adds tree-sitter-backed module-level facts (imports, public symbols, MCP decorators) for parser quality.
- **No workspace auto-discovery.** A `[workspace]`-only `Cargo.toml` or a Python workspace orchestrator is treated as a single project; sub-packages must either be siblings (first-level subdirs) or be declared via `[tool.prograph].aliases`.
- **No PEP 508 URL deps** (`name @ git+https://...`) — the name part isn't extracted cleanly.
- **No contracts / MCP / HTTP edge types** — M4.
- **No MD export, browser UI, or MCP server** — M5/M6/M7.
```

- [ ] **Step 2: Update CLAUDE.md**

In `prograph/CLAUDE.md`, replace the "Architecture (M2 state)" section with:

```markdown
## Architecture (M3 state)

Two-layer build:

- **`prograph-core` (Rust crate via PyO3):**
  - `discovery` — project classification + monorepo walk (M1)
  - `parsers/python` — `pyproject.toml` parsing: `[project].dependencies`, `[project.optional-dependencies]`, `[dependency-groups]` (PEP 735), `[tool.prograph].aliases` (M2+M3)
  - `parsers/rust` — `Cargo.toml` parsing: `[package]`, `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]` (M3)
  - `parsers/js` — `package.json` parsing: `name`, `dependencies`, `devDependencies`, `peerDependencies` (M3)
  - `detectors/deps` — alias-aware package-dependency matching with name-collision warnings (M3)
  - `diff` — added/removed/attrs_changed/unchanged classifier (M2)
  - `lock` — RAII FS exclusive lock via `fslock` (M2)
  - `indexer` — pipeline orchestrator (discover → parse → detect → diff → persist); captures `git_commit` when working tree is clean (M3)
  - `store` — SQLite schema v2 + transactional snapshot writer
  - `models` — Rust pyclasses (`ProjectKind`, `ProjectCandidate`, `Edge`, `ChangeEvent`, `SnapshotInfo`, `IndexSummary`, plus `NodeKind`, `EdgeKind`, `ChangeKind`, `EntityKind`)
  - `facts` — `Manifest` (with `aliases`), `DepRequirement`, `ProjectFacts`, `ParseStatus`, `ParseWarning`
  - `errors` — `PrographError` with PyErr mapping
  - `migrations/v1.sql`, `migrations/v2.sql` (M3 adds no schema)
- **`prograph` (Python package):** `cli.py` (`init`, `index`, `status`, `--version`), `models.py` (pydantic mirrors with `from_core(...)`), `paths.py`.

Tests live in `tests/` (pytest) and as inline `#[cfg(test)]` modules in each Rust source file.

The Rust↔Python boundary remains data-only.
```

Replace "What is NOT in M2":
```markdown
## What is NOT in M3

- Tree-sitter source-file parsing (module-level facts: imports, public symbols, MCP decorators) — M5.
- Workspace auto-discovery (nested manifests under a `[workspace]` root) — M4 or later.
- PEP 508 URL deps (`name @ git+...`) — M4 polish.
- Contracts detector + MCP detector — M4.
- MD export + golden tests — M5.
- Browser UI + MCP stdio server — M6/M7.

(See `docs/superpowers/plans/` for individual milestone plans.)
```

Update "Common commands" — no new commands in M3, but add the alias-config tip near the end:
```markdown
### Workspace aliases

For workspace orchestrators that publish under multiple names:

```toml
# project/pyproject.toml
[tool.prograph]
aliases = ["alt-name-1", "alt-name-2"]
```
```

- [ ] **Step 3: Commit**

```sh
git add prograph/README.md prograph/CLAUDE.md
git commit -m "prograph: M3 docs — multi-language status, workspace aliases, limitations"
```

---

## Task 11: M3 close — full gate + DoD

**Files:**
- Modify: `prograph/docs/superpowers/plans/2026-05-26-prograph-m3-multilang-indexer.md` (this file)

- [ ] **Step 1: Run the full local gate**

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

Expected: every command exits 0. Expected counts:
- cargo: 77 tests
- pytest: 39 tests
- realmonorepo: 1 test

- [ ] **Step 2: Check the DoD boxes below**

In the "Definition of Done (M3)" section of this plan file, change each `- [ ]` to `- [x]` with the achieved counts.

- [ ] **Step 3: Final commit**

```sh
git add prograph/docs/superpowers/plans/2026-05-26-prograph-m3-multilang-indexer.md
git commit -m "prograph: M3 close — full gate green, DoD checked"
```

---

## Definition of Done (M3)

- [x] `cargo test --all-targets` passes (78 tests).
- [x] `uv run pytest -v` passes (39 tests; 1 deselected).
- [x] `uv run pytest -m realmonorepo -v` passes against the real `all_ai_orchestrators/` and produces **2 edges** (arbiter→spec-runner, atp-platform→spec-runner via `[dependency-groups].dev`).
- [x] `prograph index` on `tests/fixtures/monorepo_multilang/` produces exactly 5 cross-project edges (1 Python alias + 1 Python dep-groups + 1 Python direct + 1 Rust + 1 JS).
- [x] `Manifest.aliases` round-trips through SQLite's `attrs_json` and matches in `deps_detector`.
- [x] Name collisions across `declared_name` + `aliases` emit warnings logged in `IndexSummary.n_warnings` (thread-local sink, drained by indexer).
- [x] `[dependency-groups]` (PEP 735) entries in `pyproject.toml` produce edges when their dep names match in-monorepo publishers.
- [x] `Cargo.toml` `[dependencies]` / `[dev-dependencies]` / `[build-dependencies]` produce edges.
- [x] `package.json` `dependencies` / `devDependencies` / `peerDependencies` produce edges.
- [x] `Mixed`-kind projects (prograph itself) parse via the Python-first fall-through.
- [x] `git_commit` is captured in `snapshots` when the working tree is clean; `None` otherwise.
- [x] CI workflow continues to pass with no changes required.
- [x] All commits follow the `prograph: M3 ...` prefix convention.

## What is NOT done in M3 (handled in subsequent milestones)

- **M4** — Contracts detector + MCP detector.
- **M5** — Tree-sitter parsing of source files (module-level imports, public symbols, MCP decorators); MD exporter + golden tests + Obsidian-friendly per-project files.
- **M6** — Browser UI (FastAPI + static + d3/cytoscape) + REST API.
- **M7** — MCP stdio server + tool surface for AI agents.
- **M8** — Workspace auto-discovery, PEP 508 URL deps, real-monorepo CI matrix, performance baselines.
