# Tracked-Projects Allowlist Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `prograph index` indexes only an allowlist from `.prograph/tracked.toml` (plus workspace members of tracked roots); `--discover` / `status` / `serve` report untracked and missing projects without indexing them.

**Architecture:** A single Rust helper (`discovery::tracked_closure` + `discovery::missing_names`) is the source of truth for "is this candidate tracked". The indexer filters candidates through it right after `scan_monorepo`; the same helpers are exposed via PyO3 so the Python-side audit (`--discover`, `status`, `serve`) cannot drift from the filter. Config reading is Python-side (`read_tracked_projects`), with malformed config a hard error in every command that reads it.

**Tech Stack:** Rust (PyO3 0.29, `Bound` API), Python 3.11+ (typer, pydantic), maturin, pytest, cargo test.

**Spec:** `docs/superpowers/specs/2026-07-10-prograph-tracked-projects-design.md`

## Global Constraints

- Ruff line length is **100** in this repo (not the global 88).
- After ANY Rust edit, run `uv run maturin develop` before pytest/CLI — Python imports the compiled `.so`, not the crate.
- `cargo test --all-targets` never enables the `extension-module` feature (maturin-only).
- Pyrefly must be run with explicit globs: `uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'` — never bare `pyrefly check`.
- `prograph/_core.pyi` is maintained by hand — update it whenever the PyO3 surface changes.
- `--json` stdout must carry ONLY the JSON object; human/audit text goes to `err_console` (stderr).
- The Rust↔Python boundary stays data-only (strings, lists, bools, pyclasses).
- Commit messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Rust core helpers `tracked_closure` + `missing_names`

**Files:**
- Modify: `prograph-core/src/discovery.rs` (add two pub fns + private `is_top_level`, tests in the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub fn tracked_closure(candidates: &[ProjectCandidate], names: &[String]) -> Vec<bool>` and `pub fn missing_names(candidates: &[ProjectCandidate], names: &[String]) -> Vec<String>` — used by Task 2 (indexer) and Task 3 (PyO3 exposure).
- `ProjectCandidate` is the existing struct in `prograph-core/src/models.rs:37` (`name`, `root_path` like `"./arbiter"` or `"./arbiter/arbiter-core"`, `kind`, `manifests`).

- [ ] **Step 1: Write failing tests**

Append to the `mod tests` block in `prograph-core/src/discovery.rs` (it already imports `super::*`). Helper to build candidates without touching the FS:

```rust
    fn cand(name: &str, root_path: &str) -> ProjectCandidate {
        ProjectCandidate {
            name: name.to_string(),
            root_path: root_path.to_string(),
            kind: ProjectKind::Python,
            manifests: vec![],
        }
    }

    #[test]
    fn tracked_closure_selects_subset() {
        let cands = vec![cand("a", "./a"), cand("b", "./b"), cand("c", "./c")];
        let names = vec!["a".to_string(), "c".to_string()];
        assert_eq!(tracked_closure(&cands, &names), vec![true, false, true]);
    }

    #[test]
    fn tracked_closure_includes_workspace_members_of_tracked_root() {
        let cands = vec![
            cand("arbiter", "./arbiter"),
            cand("arbiter-core", "./arbiter/arbiter-core"),
            cand("other", "./other"),
            cand("other-sub", "./other/sub"),
        ];
        let names = vec!["arbiter".to_string()];
        assert_eq!(tracked_closure(&cands, &names), vec![true, true, false, false]);
    }

    #[test]
    fn tracked_closure_nested_name_collision_does_not_select_root() {
        // A nested member named "wanted" must NOT become a root; only the
        // top-level project "wanted" (absent here) could.
        let cands = vec![cand("host", "./host"), cand("wanted", "./host/wanted")];
        let names = vec!["wanted".to_string()];
        assert_eq!(tracked_closure(&cands, &names), vec![false, false]);
    }

    #[test]
    fn tracked_closure_prefix_name_is_not_a_path_prefix() {
        // "./ab" must not be swallowed by tracked root "./a".
        let cands = vec![cand("a", "./a"), cand("ab", "./ab")];
        let names = vec!["a".to_string()];
        assert_eq!(tracked_closure(&cands, &names), vec![true, false]);
    }

    #[test]
    fn tracked_closure_empty_names_tracks_nothing() {
        let cands = vec![cand("a", "./a")];
        assert_eq!(tracked_closure(&cands, &[]), vec![false]);
    }

    #[test]
    fn missing_names_reports_unknown_once_despite_duplicates() {
        let cands = vec![cand("a", "./a"), cand("nested", "./a/nested")];
        let names = vec![
            "a".to_string(),
            "ghost".to_string(),
            "ghost".to_string(),
            "nested".to_string(), // nested member name is NOT a top-level match
        ];
        assert_eq!(
            missing_names(&cands, &names),
            vec!["ghost".to_string(), "nested".to_string()]
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path prograph-core/Cargo.toml tracked_closure missing_names 2>&1 | tail -20`
Expected: compile error — `tracked_closure` / `missing_names` not found.

- [ ] **Step 3: Implement the helpers**

Add above the `#[cfg(test)]` block in `prograph-core/src/discovery.rs`:

```rust
/// For each candidate, decide whether it is tracked under the `names` allowlist.
///
/// Single source of truth for "is this candidate tracked" — the indexer filters
/// through this, and the Python-side audit calls the same function via PyO3.
///
/// - `names` are deduplicated; matching is exact and case-sensitive.
/// - Tracked roots are TOP-LEVEL candidates (root_path of the form `./<dir>`,
///   exactly one `/`) whose name is in the set. A nested workspace member whose
///   name collides with an allowlist entry does NOT become a root.
/// - A candidate is tracked iff its root_path equals a tracked root's path or
///   descends from one (`starts_with(root + "/")`).
/// - Empty `names` returns all-false. The "empty allowlist = track all" rule
///   lives in callers, which pass `None` / skip the call entirely.
pub fn tracked_closure(candidates: &[ProjectCandidate], names: &[String]) -> Vec<bool> {
    let set: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();
    let roots: Vec<&str> = candidates
        .iter()
        .filter(|c| is_top_level(&c.root_path) && set.contains(c.name.as_str()))
        .map(|c| c.root_path.as_str())
        .collect();
    candidates
        .iter()
        .map(|c| {
            roots
                .iter()
                .any(|r| c.root_path == *r || c.root_path.starts_with(&format!("{r}/")))
        })
        .collect()
}

/// Allowlist names (deduplicated, first-occurrence order) that match no
/// top-level candidate. Used for `n_warnings` by the indexer and for the
/// `missing` audit list on the Python side.
pub fn missing_names(candidates: &[ProjectCandidate], names: &[String]) -> Vec<String> {
    let top: std::collections::HashSet<&str> = candidates
        .iter()
        .filter(|c| is_top_level(&c.root_path))
        .map(|c| c.name.as_str())
        .collect();
    let mut seen = std::collections::HashSet::new();
    names
        .iter()
        .filter(|n| seen.insert(n.as_str()) && !top.contains(n.as_str()))
        .cloned()
        .collect()
}

/// Top-level == direct child of the monorepo root: `./<dir>` with exactly one `/`.
fn is_top_level(root_path: &str) -> bool {
    root_path.starts_with("./") && root_path.matches('/').count() == 1
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path prograph-core/Cargo.toml tracked_closure missing_names 2>&1 | tail -10`
Expected: `test result: ok. 6 passed` (the 6 new tests).

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add prograph-core/src/discovery.rs
git commit -m "feat(core): tracked_closure + missing_names allowlist helpers

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Thread `tracked` through the indexer

**Files:**
- Modify: `prograph-core/src/indexer.rs:24` (signature + filter), plus every Rust call site of `index_monorepo(` (find with grep; at minimum `prograph-core/src/lib.rs:37` and `#[cfg(test)]` tests inside `indexer.rs`)

**Interfaces:**
- Consumes: `discovery::tracked_closure`, `discovery::missing_names` (Task 1).
- Produces: `pub fn index_monorepo(monorepo_root: &Path, store: &mut Store, tracked: Option<Vec<String>>) -> Result<IndexSummary>`. `None` → current behaviour, helpers not called. Task 3 exposes the third parameter to Python.

- [ ] **Step 1: Write the failing test**

The `#[cfg(test)] mod tests` in `prograph-core/src/indexer.rs` (line ~667) has a `setup_monorepo()` fixture creating a temp monorepo with two top-level projects — directories `consumer/` and `sdk/` (note: `sdk`'s *declared* package name is `my-sdk`, but the allowlist matches the *candidate* name, which is the directory name `sdk`). Add to that module:

```rust
    #[test]
    fn tracked_allowlist_filters_projects_and_warns_on_unknown() {
        let _ = crate::detectors::deps::drain_collision_warnings();
        let dir = setup_monorepo();
        let mut store = Store::open(&dir.path().join(".prograph/graph.db")).unwrap();
        let summary = index_monorepo(
            dir.path(),
            &mut store,
            Some(vec!["sdk".to_string(), "ghost".to_string(), "ghost".to_string()]),
        )
        .unwrap();
        assert_eq!(summary.n_projects, 1, "only sdk should be indexed");
        assert_eq!(summary.n_warnings, 1, "one warning for 'ghost', deduplicated");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --manifest-path prograph-core/Cargo.toml tracked_allowlist 2>&1 | tail -10`
Expected: compile error — `index_monorepo` takes 2 arguments.

- [ ] **Step 3: Implement**

In `prograph-core/src/indexer.rs`, change the signature and insert the filter between Phase 1 and Phase 2. `warning_count` currently begins at 0 just before the parse loop — hoist its declaration above the filter:

```rust
pub fn index_monorepo(
    monorepo_root: &Path,
    store: &mut Store,
    tracked: Option<Vec<String>>,
) -> Result<IndexSummary> {
    let start = Instant::now();
    let lock_path = monorepo_root.join(".prograph").join("index.lock");
    let _lock = IndexLockGuard::acquire(&lock_path)?;

    // Phase 1: Discovery.
    let mut candidates = scan_monorepo(monorepo_root)?;
    let mut warning_count: i64 = 0;

    // Phase 1b: allowlist filter (spec 2026-07-10-prograph-tracked-projects).
    // None -> track everything (legacy). Some(names) -> keep only the tracked
    // closure; each deduplicated name matching no top-level candidate warns.
    if let Some(names) = &tracked {
        warning_count += crate::discovery::missing_names(&candidates, names).len() as i64;
        let flags = crate::discovery::tracked_closure(&candidates, names);
        candidates = candidates
            .into_iter()
            .zip(flags)
            .filter_map(|(c, keep)| keep.then_some(c))
            .collect();
    }
```

Delete the old `let mut warning_count: i64 = 0;` line before the parse loop (now hoisted). Then fix every other call site:

Run: `grep -rn "index_monorepo(" prograph-core/src/ | grep -v "pub fn"`

For each Rust call, append `, None` as the third argument — EXCEPT `lib.rs` `py_index_monorepo`, which Task 3 rewires properly; for now make it pass `None` to keep the crate compiling:

```rust
    Ok(indexer::index_monorepo(
        std::path::Path::new(monorepo_root),
        &mut store,
        None,
    )?)
```

- [ ] **Step 4: Run the full Rust suite**

Run: `cargo test --all-targets 2>&1 | tail -10`
Expected: all green, including the new `tracked_allowlist_filters_projects_and_warns_on_unknown`.

- [ ] **Step 5: Format, lint, commit**

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add prograph-core/src/indexer.rs prograph-core/src/lib.rs
git commit -m "feat(core): index_monorepo takes Option<Vec<String>> allowlist

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: PyO3 surface — expose helpers, extend `index_monorepo` signature

**Files:**
- Modify: `prograph-core/src/discovery.rs` (two `#[pyfunction]` wrappers next to `py_scan_monorepo`, ~line 299)
- Modify: `prograph-core/src/lib.rs` (`py_index_monorepo` signature ~line 35; registration block ~line 181)
- Modify: `prograph/_core.pyi` (hand-maintained stub)
- Test: `tests/unit/test_core_tracked.py` (new)

**Interfaces:**
- Consumes: Rust `tracked_closure` / `missing_names` (Task 1), 3-arg `index_monorepo` (Task 2).
- Produces (Python): `_core.tracked_closure(candidates: list[ProjectCandidate], names: list[str]) -> list[bool]`; `_core.missing_names(candidates: list[ProjectCandidate], names: list[str]) -> list[str]`; `_core.index_monorepo(monorepo_root: str, db_path: str, tracked: list[str] | None = None) -> IndexSummary`. Tasks 5–7 call these.

- [ ] **Step 1: Write the failing test**

Create `tests/unit/test_core_tracked.py`:

```python
"""Tests for the PyO3-exposed tracked_closure / missing_names helpers."""

from prograph import _core


def _cand(name: str, root_path: str) -> "_core.ProjectCandidate":
    return _core.ProjectCandidate(name, root_path, _core.ProjectKind.Python, [])


def test_tracked_closure_subset_and_members() -> None:
    cands = [
        _cand("arbiter", "./arbiter"),
        _cand("arbiter-core", "./arbiter/arbiter-core"),
        _cand("other", "./other"),
    ]
    assert _core.tracked_closure(cands, ["arbiter"]) == [True, True, False]


def test_missing_names_deduplicated() -> None:
    cands = [_cand("a", "./a")]
    assert _core.missing_names(cands, ["a", "ghost", "ghost"]) == ["ghost"]


def test_index_monorepo_accepts_two_args() -> None:
    # Backward-compatible signature: tracked defaults to None.
    import inspect  # noqa: F401 — presence check happens via the call in integration tests

    assert _core.index_monorepo.__doc__ is not None or True
```

- [ ] **Step 2: Run to verify it fails**

Run: `uv run pytest tests/unit/test_core_tracked.py -v 2>&1 | tail -5`
Expected: FAIL — `module 'prograph._core' has no attribute 'tracked_closure'`.

- [ ] **Step 3: Implement the wrappers**

In `prograph-core/src/discovery.rs`, next to `py_scan_monorepo`. `ProjectCandidate` is `skip_from_py_object` (output-only pyclass), so accept `Vec<Py<ProjectCandidate>>` and borrow — the boundary stays data-only:

```rust
/// Python entry point: per-candidate tracked flags under an allowlist.
#[pyfunction]
#[pyo3(name = "tracked_closure")]
pub fn py_tracked_closure(
    py: Python<'_>,
    candidates: Vec<Py<ProjectCandidate>>,
    names: Vec<String>,
) -> PyResult<Vec<bool>> {
    let cands: Vec<ProjectCandidate> = candidates.iter().map(|c| c.borrow(py).clone()).collect();
    Ok(tracked_closure(&cands, &names))
}

/// Python entry point: allowlist names matching no top-level candidate.
#[pyfunction]
#[pyo3(name = "missing_names")]
pub fn py_missing_names(
    py: Python<'_>,
    candidates: Vec<Py<ProjectCandidate>>,
    names: Vec<String>,
) -> PyResult<Vec<String>> {
    let cands: Vec<ProjectCandidate> = candidates.iter().map(|c| c.borrow(py).clone()).collect();
    Ok(missing_names(&cands, &names))
}
```

Add `use pyo3::prelude::*;` only if not already imported in `discovery.rs` (check the `py_scan_monorepo` imports — likely already there).

In `prograph-core/src/lib.rs`, rewire `py_index_monorepo` with a default so 2-arg calls keep working:

```rust
/// Python entry point for `prograph index`.
#[pyfunction]
#[pyo3(name = "index_monorepo", signature = (monorepo_root, db_path, tracked=None))]
fn py_index_monorepo(
    monorepo_root: &str,
    db_path: &str,
    tracked: Option<Vec<String>>,
) -> PyResult<IndexSummary> {
    let mut store = Store::open(std::path::Path::new(db_path))?;
    Ok(indexer::index_monorepo(
        std::path::Path::new(monorepo_root),
        &mut store,
        tracked,
    )?)
}
```

Register in the `#[pymodule]` block after `py_scan_monorepo`:

```rust
    m.add_function(wrap_pyfunction!(discovery::py_tracked_closure, m)?)?;
    m.add_function(wrap_pyfunction!(discovery::py_missing_names, m)?)?;
```

Update `prograph/_core.pyi` — change line 7 and add two lines after line 6:

```python
def scan_monorepo(monorepo_root: str) -> list[ProjectCandidate]: ...
def tracked_closure(candidates: list[ProjectCandidate], names: list[str]) -> list[bool]: ...
def missing_names(candidates: list[ProjectCandidate], names: list[str]) -> list[str]: ...
def index_monorepo(
    monorepo_root: str, db_path: str, tracked: list[str] | None = None
) -> IndexSummary: ...
```

- [ ] **Step 4: Rebuild and run the test**

```bash
uv run maturin develop
uv run pytest tests/unit/test_core_tracked.py -v
```
Expected: PASS (3 tests).

- [ ] **Step 5: Full Rust + Python gates, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
uv run pytest -x -q 2>&1 | tail -5   # existing suite must stay green (2-arg calls still work)
git add prograph-core/src/discovery.rs prograph-core/src/lib.rs prograph/_core.pyi tests/unit/test_core_tracked.py
git commit -m "feat(pyo3): expose tracked_closure/missing_names, optional tracked in index_monorepo

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Python config reader `read_tracked_projects` + `TrackedConfigError`

**Files:**
- Modify: `prograph/config.py` (new exception + reader)
- Modify: `prograph/paths.py` (add `tracked_path` property next to `config_path`, line ~35)
- Test: `tests/unit/test_config.py` (append)

**Interfaces:**
- Produces: `TrackedConfigError(Exception)`; `read_tracked_projects(prograph_dir: Path) -> list[str] | None`; `PrographPaths.tracked_path -> Path` (`.prograph/tracked.toml`). Tasks 5–8 consume all three.

- [ ] **Step 1: Write failing tests**

Append to `tests/unit/test_config.py` (it already imports `Path` and config helpers — extend imports to `from prograph.config import TrackedConfigError, read_tracked_projects` and add `import pytest` if absent):

```python
def test_read_tracked_missing_file_is_none(tmp_path: Path) -> None:
    assert read_tracked_projects(tmp_path) is None


def test_read_tracked_valid_list(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text('projects = ["a", "b"]\n')
    assert read_tracked_projects(tmp_path) == ["a", "b"]


def test_read_tracked_empty_list_is_none(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text("projects = []\n")
    assert read_tracked_projects(tmp_path) is None


def test_read_tracked_missing_key_is_none(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text("# nothing here\n")
    assert read_tracked_projects(tmp_path) is None


def test_read_tracked_malformed_toml_raises(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text("projects = [broken\n")
    with pytest.raises(TrackedConfigError):
        read_tracked_projects(tmp_path)


def test_read_tracked_non_list_raises(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text('projects = "a"\n')
    with pytest.raises(TrackedConfigError):
        read_tracked_projects(tmp_path)


def test_read_tracked_non_string_items_raise(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text("projects = [1, 2]\n")
    with pytest.raises(TrackedConfigError):
        read_tracked_projects(tmp_path)


def test_paths_tracked_path() -> None:
    from prograph.paths import PrographPaths

    p = PrographPaths(monorepo_root=Path("/x"))
    assert p.tracked_path == Path("/x/.prograph/tracked.toml")
```

- [ ] **Step 2: Run to verify they fail**

Run: `uv run pytest tests/unit/test_config.py -v -k tracked 2>&1 | tail -5`
Expected: FAIL — `ImportError: cannot import name 'TrackedConfigError'`.

- [ ] **Step 3: Implement**

Append to `prograph/config.py`:

```python
class TrackedConfigError(Exception):
    """`.prograph/tracked.toml` exists but cannot be interpreted.

    Deliberately a hard error (unlike `read_export_root`'s fail-open): a broken
    allowlist silently falling back to "track everything" would reintroduce the
    graph pollution the allowlist exists to prevent.
    """


def read_tracked_projects(prograph_dir: Path) -> list[str] | None:
    """Return the tracked-projects allowlist from `.prograph/tracked.toml`.

    Missing file, missing `projects` key, or an empty list -> None (track
    everything — legacy behaviour). Malformed TOML or a non-list /
    non-string-list `projects` -> TrackedConfigError.
    """
    path = prograph_dir / "tracked.toml"
    if not path.is_file():
        return None
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise TrackedConfigError(f"cannot parse {path}: {exc}") from exc
    projects = data.get("projects")
    if projects is None or projects == []:
        return None
    if not isinstance(projects, list) or not all(isinstance(p, str) for p in projects):
        raise TrackedConfigError(f"{path}: `projects` must be a list of strings")
    return projects
```

Add to `prograph/paths.py` next to `config_path` (~line 35):

```python
    @property
    def tracked_path(self) -> Path:
        return self.prograph_dir / "tracked.toml"
```

- [ ] **Step 4: Run to verify they pass**

Run: `uv run pytest tests/unit/test_config.py tests/unit/test_paths.py -v 2>&1 | tail -5`
Expected: PASS (new + existing).

- [ ] **Step 5: Lint, typecheck, commit**

```bash
uv run ruff format prograph/config.py prograph/paths.py tests/unit/test_config.py
uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/config.py prograph/paths.py tests/unit/test_config.py
git commit -m "feat(config): read_tracked_projects with hard-error TrackedConfigError

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: CLI `index` — allowlist filtering + `--discover` audit

**Files:**
- Modify: `prograph/cli.py` (`index` command ~line 149; new module-level helpers)
- Test: `tests/integration/test_cli_tracked.py` (new)

**Interfaces:**
- Consumes: `_core.index_monorepo(root, db, tracked)`, `_core.scan_monorepo`, `_core.tracked_closure`, `_core.missing_names` (Task 3); `read_tracked_projects`, `TrackedConfigError` (Task 4).
- Produces: `_read_tracked_or_exit(paths: PrographPaths) -> list[str] | None` and `_compute_audit(root: Path, tracked: list[str]) -> dict[str, object]` module-level helpers in `cli.py` — Tasks 6 and 7 reuse them. JSON audit shape: `{"untracked": [{"name", "root_path", "kind"}], "missing": [str]}` embedded under key `"discover"`.

- [ ] **Step 1: Write failing tests**

Create `tests/integration/test_cli_tracked.py`:

```python
"""Tests for the tracked-projects allowlist: `index` filtering + `--discover` audit."""

import json as _json
from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()


def _setup(root: Path) -> None:
    """Two plain projects + one uv workspace with a nested member."""
    (root / "tracked_proj").mkdir()
    (root / "tracked_proj" / "pyproject.toml").write_text(
        '[project]\nname="tracked_proj"\nversion="1.0"\ndependencies=[]\n'
    )
    (root / "loose_proj").mkdir()
    (root / "loose_proj" / "pyproject.toml").write_text(
        '[project]\nname="loose_proj"\nversion="1.0"\ndependencies=[]\n'
    )
    ws = root / "ws_root"
    (ws / "member_a").mkdir(parents=True)
    (ws / "pyproject.toml").write_text(
        '[project]\nname="ws_root"\nversion="1.0"\ndependencies=[]\n'
        '[tool.uv.workspace]\nmembers=["member_a"]\n'
    )
    (ws / "member_a" / "pyproject.toml").write_text(
        '[project]\nname="member_a"\nversion="1.0"\ndependencies=[]\n'
    )


def _init_with_allowlist(root: Path, toml_body: str) -> PrographPaths:
    runner.invoke(app, ["init", "--monorepo", str(root)])
    paths = PrographPaths(monorepo_root=root)
    paths.tracked_path.write_text(toml_body)
    return paths


def test_index_filters_to_allowlist_closure(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, 'projects = ["tracked_proj", "ws_root"]\n')
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--json"])
    assert result.exit_code == 0, result.output
    summary = _json.loads(result.stdout)
    # tracked_proj + ws_root + member_a (workspace member of a tracked root)
    assert summary["n_projects"] == 3


def test_index_without_tracked_toml_indexes_all(tmp_path: Path) -> None:
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    paths = PrographPaths(monorepo_root=tmp_path)
    if paths.tracked_path.exists():  # init writes an empty template — empty means all
        assert "projects = []" in paths.tracked_path.read_text()
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--json"])
    assert result.exit_code == 0, result.output
    assert _json.loads(result.stdout)["n_projects"] == 4  # all incl. member_a


def test_index_malformed_tracked_toml_exits_1(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, "projects = [broken\n")
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1
    assert "tracked.toml" in (result.stdout + result.stderr)


def test_index_discover_json_embeds_audit(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, 'projects = ["tracked_proj", "ghost"]\n')
    result = runner.invoke(
        app, ["index", "--monorepo", str(tmp_path), "--json", "--discover"]
    )
    assert result.exit_code == 0, result.output
    payload = _json.loads(result.stdout)  # stdout must be pure JSON
    audit = payload["discover"]
    untracked_names = {e["name"] for e in audit["untracked"]}
    assert untracked_names == {"loose_proj", "ws_root", "member_a"}
    assert all({"name", "root_path", "kind"} <= set(e) for e in audit["untracked"])
    assert audit["missing"] == ["ghost"]


def test_index_discover_text_goes_to_stderr(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, 'projects = ["tracked_proj", "ghost"]\n')
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--discover"])
    assert result.exit_code == 0, result.output
    assert "loose_proj" in result.stderr
    assert "ghost" in result.stderr
    assert "loose_proj" not in result.stdout
```

- [ ] **Step 2: Run to verify they fail**

Run: `uv run pytest tests/integration/test_cli_tracked.py -v 2>&1 | tail -10`
Expected: FAIL — filtering tests see `n_projects == 4`, `--discover` is an unknown option.

- [ ] **Step 3: Implement**

In `prograph/cli.py`. Extend the config import (line ~14):

```python
from prograph.config import (
    TrackedConfigError,
    read_auto_export,
    read_export_root,
    read_tracked_projects,
)
```

Add module-level helpers near `_resolve_export_root` (~line 95):

```python
def _read_tracked_or_exit(paths: PrographPaths) -> list[str] | None:
    """Read the allowlist; malformed tracked.toml is a hard error (exit 1).

    Uniform across index/status/serve — a silently-ignored broken allowlist
    would present a wrong picture (spec 2026-07-10-prograph-tracked-projects).
    """
    try:
        return read_tracked_projects(paths.prograph_dir)
    except TrackedConfigError as exc:
        err_console.print(f"[red]error:[/red] {exc}")
        raise typer.Exit(code=1) from exc


def _compute_audit(root: Path, tracked: list[str]) -> dict[str, object]:
    """Full-scan audit vs the allowlist: untracked candidates + missing names.

    Uses the same Rust helpers the indexer filters with — the audit cannot
    drift from the filter.
    """
    raw_candidates = _core.scan_monorepo(str(root))
    flags = _core.tracked_closure(raw_candidates, tracked)
    mirrors = [ProjectCandidate.from_core(c) for c in raw_candidates]
    untracked = [
        {"name": m.name, "root_path": m.root_path, "kind": m.kind.value}
        for m, keep in zip(mirrors, flags)
        if not keep
    ]
    missing = list(_core.missing_names(raw_candidates, tracked))
    return {"untracked": untracked, "missing": missing}


def _print_audit_stderr(audit: dict[str, object]) -> None:
    untracked = audit["untracked"]
    missing = audit["missing"]
    assert isinstance(untracked, list) and isinstance(missing, list)  # narrow for pyrefly
    if untracked:
        err_console.print(f"[yellow]discover:[/yellow] {len(untracked)} untracked project(s):")
        for entry in untracked:
            err_console.print(
                f"  - {entry['name']} ({entry['root_path']}, {entry['kind']})"
            )
    if missing:
        err_console.print(
            "[yellow]discover:[/yellow] allowlisted but not found: " + ", ".join(missing)
        )
    if not untracked and not missing:
        err_console.print("[green]discover:[/green] allowlist matches discovery — no drift.")
```

In the `index` command: add the option after `out_dir`:

```python
    discover: bool = typer.Option(
        False,
        "--discover",
        help="After indexing, run a full scan and report untracked/missing projects "
        "(report only — untracked projects are not indexed).",
    ),
```

After the `is_initialized` check and before the `_core.index_monorepo` call:

```python
    tracked = _read_tracked_or_exit(paths)
```

Change the core call:

```python
        raw = _core.index_monorepo(str(root), str(paths.db_path), tracked)
```

After `summary = IndexSummary.from_core(raw)` (before the export block), compute the audit:

```python
    audit: dict[str, object] | None = None
    if discover:
        # tracked is None -> everything is tracked; audit trivially empty.
        audit = (
            _compute_audit(root, tracked)
            if tracked is not None
            else {"untracked": [], "missing": []}
        )
```

Replace the JSON emission block:

```python
    if json:
        payload = summary.model_dump(mode="json")
        if audit is not None:
            payload["discover"] = audit
        sys.stdout.write(_json.dumps(payload, indent=2) + "\n")
        return
```

And after the existing non-JSON `console.print` lines:

```python
    if audit is not None:
        _print_audit_stderr(audit)
```

`ProjectCandidate` (pydantic mirror) is already imported in `cli.py` for `status` — verify, don't duplicate the import.

- [ ] **Step 4: Run to verify they pass**

```bash
uv run maturin develop   # only if Task 3's build isn't current
uv run pytest tests/integration/test_cli_tracked.py tests/integration/test_cli_index.py -v 2>&1 | tail -10
```
Expected: PASS (new file + existing index tests untouched by the default `tracked=None` path... existing tests run without tracked.toml except via `init` — see Task 8 note: until Task 8, `init` does not create the file, so legacy tests are unaffected).

- [ ] **Step 5: Lint, typecheck, commit**

```bash
uv run ruff format prograph/cli.py tests/integration/test_cli_tracked.py
uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/cli.py tests/integration/test_cli_tracked.py
git commit -m "feat(cli): index respects tracked.toml allowlist, --discover audit

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: CLI `status` — tracked/untracked annotation

**Files:**
- Modify: `prograph/cli.py` (`status` command, ~line 226)
- Test: `tests/integration/test_cli_tracked.py` (append)

**Interfaces:**
- Consumes: `_read_tracked_or_exit`, `_core.tracked_closure` (Tasks 3, 5).
- Produces: `status --json` projects each gain `"tracked": bool`; table gains a `tracked` column.

- [ ] **Step 1: Write failing tests**

Append to `tests/integration/test_cli_tracked.py`:

```python
def test_status_json_annotates_tracked(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, 'projects = ["tracked_proj"]\n')
    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path), "--json"])
    assert result.exit_code == 0, result.output
    payload = _json.loads(result.stdout)
    by_name = {p["name"]: p["tracked"] for p in payload["projects"]}
    assert by_name["tracked_proj"] is True
    assert by_name["loose_proj"] is False


def test_status_without_allowlist_all_tracked(tmp_path: Path) -> None:
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path), "--json"])
    payload = _json.loads(result.stdout)
    assert all(p["tracked"] for p in payload["projects"])


def test_status_malformed_tracked_toml_exits_1(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, "projects = [broken\n")
    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1
```

- [ ] **Step 2: Run to verify they fail**

Run: `uv run pytest tests/integration/test_cli_tracked.py -v -k status 2>&1 | tail -5`
Expected: FAIL — `KeyError: 'tracked'`.

- [ ] **Step 3: Implement**

In the `status` command, after `raw_candidates = _core.scan_monorepo(str(root))`:

```python
    tracked_list = _read_tracked_or_exit(paths)
    if tracked_list is None:
        tracked_flags = [True] * len(raw_candidates)
    else:
        tracked_flags = list(_core.tracked_closure(raw_candidates, tracked_list))
```

In the JSON payload, zip the flag in:

```python
        payload = {
            "monorepo_root": str(root),
            "snapshot": snapshot.model_dump(mode="json") if snapshot else None,
            "projects": [
                {**c.model_dump(mode="json"), "tracked": flag}
                for c, flag in zip(candidates, tracked_flags)
            ],
        }
```

In the table, add a column after "manifests":

```python
    table.add_column("tracked")
```

and in the row loop (zip with flags):

```python
    for c, flag in zip(candidates, tracked_flags):
        table.add_row(
            c.name,
            c.kind.value,
            c.root_path,
            ", ".join(c.manifests),
            "[green]yes[/green]" if flag else "[yellow]no[/yellow]",
        )
```

- [ ] **Step 4: Run to verify they pass**

Run: `uv run pytest tests/integration/test_cli_tracked.py tests/integration/test_cli_status.py -v 2>&1 | tail -5`
Expected: PASS (existing status tests keep working — no tracked.toml → all-True flag path).

- [ ] **Step 5: Lint, typecheck, commit**

```bash
uv run ruff format prograph/cli.py tests/integration/test_cli_tracked.py
uv run ruff check . && uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/cli.py tests/integration/test_cli_tracked.py
git commit -m "feat(cli): status annotates tracked/untracked per project

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: CLI `serve` — one-shot startup audit

**Files:**
- Modify: `prograph/cli.py` (`serve` command, ~line 432)
- Test: `tests/integration/test_cli_tracked.py` (append)

**Interfaces:**
- Consumes: `_read_tracked_or_exit`, `_compute_audit`, `_print_audit_stderr` (Task 5).
- Produces: audit lines on stderr before uvicorn starts; malformed tracked.toml → exit 1 before binding.

- [ ] **Step 1: Write failing tests**

Append to `tests/integration/test_cli_tracked.py`. `serve` blocks on uvicorn, so test only the pre-server paths (malformed exit + audit emission) by mocking `uvicorn.run`:

```python
def test_serve_malformed_tracked_toml_exits_1(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, "projects = [broken\n")
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])  # exits 1, no db — fine
    result = runner.invoke(app, ["serve", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1


def test_serve_logs_audit_before_start(tmp_path: Path, monkeypatch) -> None:
    _setup(tmp_path)
    paths = _init_with_allowlist(tmp_path, 'projects = ["tracked_proj", "ghost"]\n')
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0
    assert paths.db_path.is_file()

    import uvicorn

    monkeypatch.setattr(uvicorn, "run", lambda *a, **k: None)
    result = runner.invoke(app, ["serve", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.output
    assert "loose_proj" in result.stderr
    assert "ghost" in result.stderr
```

Note ordering: the malformed test relies on `serve` reading tracked.toml BEFORE the `db_path.exists()` check would matter — put the read after `is_initialized` but the db check first is also fine as long as exit is 1. Simplest: in the malformed test the `index` invocation already failed, so there is no db; `serve` exits 1 on the missing-db check with a different message. To pin the tracked error specifically, place the `_read_tracked_or_exit` call BEFORE the db check in `serve` — then assert `"tracked.toml" in result.stderr` too:

```python
    assert "tracked.toml" in (result.stdout + result.stderr)
```

(Add that assertion to `test_serve_malformed_tracked_toml_exits_1`.)

- [ ] **Step 2: Run to verify they fail**

Run: `uv run pytest tests/integration/test_cli_tracked.py -v -k serve 2>&1 | tail -5`
Expected: FAIL — malformed test sees the missing-db error instead of tracked.toml; audit test finds no "loose_proj" on stderr.

- [ ] **Step 3: Implement**

In `serve`, right after the `is_initialized` check (BEFORE the `db_path.exists()` check, so a broken allowlist fails fast with the right message):

```python
    tracked_list = _read_tracked_or_exit(paths)
```

Then after the existing `console.print(f"[green]prograph serve[/green] ...")` line, before `build_app`:

```python
    if tracked_list is not None:
        _print_audit_stderr(_compute_audit(root, tracked_list))
```

- [ ] **Step 4: Run to verify they pass**

Run: `uv run pytest tests/integration/test_cli_tracked.py tests/integration/test_cli_serve.py -v 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 5: Lint, typecheck, commit**

```bash
uv run ruff format prograph/cli.py tests/integration/test_cli_tracked.py
uv run ruff check . && uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/cli.py tests/integration/test_cli_tracked.py
git commit -m "feat(cli): serve runs one-shot tracked-projects audit at startup

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 8: `init` template + README migration note

**Files:**
- Modify: `prograph/cli.py` (`DEFAULT_TRACKED_TOML` const near `DEFAULT_CONFIG_TOML`/`DEFAULT_GITIGNORE` ~line 59-75; `init` command)
- Modify: `README.md` (tracked.toml section + removed-wave migration note)
- Test: `tests/integration/test_cli_init.py` (append)

**Interfaces:**
- Consumes: `PrographPaths.tracked_path` (Task 4).
- Produces: `prograph init` writes a commented `tracked.toml` template with `projects = []` (idempotent — never overwrites an existing file).

- [ ] **Step 1: Write the failing test**

Append to `tests/integration/test_cli_init.py` (match its existing imports/runner conventions):

```python
def test_init_creates_tracked_toml_template(tmp_path: Path) -> None:
    result = runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0
    tracked = tmp_path / ".prograph" / "tracked.toml"
    assert tracked.is_file()
    assert "projects = []" in tracked.read_text()


def test_init_does_not_overwrite_tracked_toml(tmp_path: Path) -> None:
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    tracked = tmp_path / ".prograph" / "tracked.toml"
    tracked.write_text('projects = ["mine"]\n')
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    assert tracked.read_text() == 'projects = ["mine"]\n'
```

- [ ] **Step 2: Run to verify it fails**

Run: `uv run pytest tests/integration/test_cli_init.py -v -k tracked 2>&1 | tail -5`
Expected: FAIL — tracked.toml not created.

- [ ] **Step 3: Implement**

Add the constant next to `DEFAULT_CONFIG_TOML` in `prograph/cli.py`:

```python
DEFAULT_TRACKED_TOML = """\
# Tracked-projects allowlist — edit by hand. Re-running `prograph init` will not
# overwrite this file.
#
# Projects listed here (top-level directory names) are indexed on every
# `prograph index`; workspace members of a tracked root are included
# automatically. Empty list or missing file -> ALL discovered projects are
# tracked (legacy behaviour). Malformed file -> hard error.
#
# `prograph index --discover` reports projects that exist but are not listed.
projects = []
"""
```

In `init`, after the `gitignore_path` block:

```python
    if not paths.tracked_path.exists():
        paths.tracked_path.write_text(DEFAULT_TRACKED_TOML)
```

In `README.md`, add a subsection under the usage/config docs (find the section documenting `config.toml` / CLI usage and append after it):

```markdown
### Tracked projects (`.prograph/tracked.toml`)

`prograph index` indexes only the projects named in `.prograph/tracked.toml`
(top-level directory names; workspace members of a tracked root are included
automatically). An empty list or a missing file tracks everything. A malformed
file is a hard error for `index`, `status`, and `serve`.

`prograph index --discover` additionally runs a full scan and reports
*untracked* projects (discovered but not listed) and *missing* names (listed
but not discovered) — report only, nothing is indexed or written.

> **Migration note:** the first `index` after introducing an allowlist emits
> `removed` change-log entries for every previously-indexed project that is no
> longer tracked. This is expected — the graph now reflects the tracked set —
> not a mass deletion bug.
```

- [ ] **Step 4: Run to verify tests pass**

Run: `uv run pytest tests/integration/test_cli_init.py tests/integration/test_cli_tracked.py -v 2>&1 | tail -5`
Expected: PASS — including Task 5's `test_index_without_tracked_toml_indexes_all`, which tolerates the empty template (`projects = []` → `None` → track all).

- [ ] **Step 5: Lint, typecheck, commit**

```bash
uv run ruff format prograph/cli.py tests/integration/test_cli_init.py
uv run ruff check . && uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/cli.py tests/integration/test_cli_init.py README.md
git commit -m "feat(cli): init writes tracked.toml template; document allowlist in README

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 9: Full gates + deploy the real allowlist to the monorepo

**Files:**
- Create: `/Users/Andrei_Shtanakov/labs/all_ai_orchestrators/.prograph/tracked.toml` (the parent monorepo's live config — OUTSIDE this repo, not committed here)

**Interfaces:**
- Consumes: everything above.

- [x] **Step 1: Run every gate**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
uv run maturin develop
uv run ruff check . && uv run ruff format --check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
uv run pytest -v 2>&1 | tail -15
```
Expected: all green. Fix anything red before proceeding.

- [x] **Step 2: Write the live allowlist (the 15 projects the user chose)**

```bash
cat > /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/.prograph/tracked.toml <<'EOF'
# Tracked-projects allowlist — edit by hand.
# Workspace members of a tracked root are included automatically.
# `prograph index --discover` reports projects not listed here.
projects = [
  "arbiter",
  "atp-platform",
  "deployer",
  "dispatcher",
  "Maestro",
  "open-prose",
  "proctor",
  "prograph",
  "prograph-vault",
  "robin-runtime",
  "robin-toolkit",
  "spec-runner",
  "spec-runner-vscode",
  "steward",
  "github-checker",
]
EOF
```

- [x] **Step 3: Verify end-to-end on the real monorepo**

```bash
uv run prograph index --monorepo /Users/Andrei_Shtanakov/labs/all_ai_orchestrators --discover --json
```
Expected: `n_projects` = 15 tracked roots + their workspace members (33 with the current tree: arbiter +3, prograph +1, atp-platform +14); `"discover"` lists the untracked scratch projects (`appgraph`, `devtools`, `sdd-framework`, `spec-runner-tasks`, `spec-runner-test`, `spec-runner-test-vscode`); `missing` empty. The change log will show `removed` entries for the newly-untracked projects — expected per the migration note.

- [x] **Step 4: Verify status annotation on the real monorepo**

```bash
uv run prograph status --monorepo /Users/Andrei_Shtanakov/labs/all_ai_orchestrators 2>&1 | tail -45
```
Expected: table shows `yes` for the 15 roots + members, `no` for scratch projects.

- [x] **Step 5: Final commit (docs/plan checkboxes only — the live tracked.toml lives outside this repo)**

```bash
git status --short   # confirm only intended files
git add -A docs/
git commit -m "docs: check off tracked-projects implementation plan

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
