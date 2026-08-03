# Conformance Report Provenance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the approved spec
`docs/superpowers/specs/2026-08-03-conformance-report-provenance-design.md` (#25): per-project
git provenance captured at index time (schema v11), the D2 provenance block in
`conformance-report/v1`, an injectable clock, and two published JSON Schema contract
artifacts with sync tests — turning the report into versioned evidence steward can consume
offline.

**Architecture:** Two Rust tasks (migration + store surface; indexer capture + PyO3), then
Python: a new `prograph/conformance/provenance.py` (content hash, clock, `ReportProvenance`
assembly) feeding a reshaped `report.py` payload, wired in the CLI. Contract schemas are
static JSON files under `contracts/` locked to the code by structural sync tests. Golden
stays literally byte-exact via a frozen injectable clock plus a fixed snapshot timestamp
written into the test fixture's DB.

**Tech Stack:** Rust (rusqlite, PyO3 0.29 — rebuild with `uv run maturin develop` after ANY
Rust change), Python 3.11+ (pydantic v2, typer), `jsonschema` (new dev dep).

## Global Constraints

- uv only, never pip. New dev dep: `uv add --dev jsonschema`. No new runtime deps.
- Ruff line length **100**; type hints everywhere; pyrefly via explicit globs:
  `uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'`.
- Local checks ARE CI: `uv run pytest -v`, `cargo test --all-targets`,
  `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`,
  `uv run ruff format . && uv run ruff check .` — all green before every commit claim.
- **After editing any Rust source run `uv run maturin develop`** before pytest/CLI — Python
  imports the compiled `.so`, not the crate. Regenerate `prograph/_core.pyi` by hand.
- Git: branch `feat/report-provenance`, commits per task, PR at the end; no direct master
  commits; do not merge.
- Spec decisions are normative: D2 payload shape (exact key names below), D3 index-time
  capture, D4 hash format `prograph-snapshot/v1+sha256:<hex>`, D5 `complete` always `true`
  in v1, D6 structural-only schema guarantee, D8 injectable clock — no public CLI flag, no
  payload normalization.
- Timestamps: RFC3339 second-precision UTC `YYYY-MM-DDTHH:MM:SSZ` (matches the store's
  `current_iso_ts()` format). `snapshot.indexed_at` is copied verbatim from
  `SnapshotInfo.ts`.
- `tool.version` = `prograph.__version__` (currently "0.1.0"); `tool.name` = "prograph";
  `tool.schema` = "intended-graph/v1".

## File Structure

- Create: `prograph-core/src/migrations/v11.sql`
- Modify: `prograph-core/src/store.rs` (MIGRATIONS registry, `insert_project_git_state`,
  `project_git_states`), `prograph-core/src/indexer.rs` (`detect_git_state`, capture loop,
  dirty warnings), `prograph-core/src/lib.rs` + `prograph/_core.pyi`
  (`ProjectGitStateRow`, `project_git_states`)
- Create: `prograph/conformance/provenance.py`; Modify: `prograph/conformance/report.py`,
  `prograph/cli.py`
- Create: `contracts/intended-graph/v1/schema.json`,
  `contracts/conformance-report/v1/schema.json`, `tests/unit/test_contract_schemas.py`,
  `tests/unit/test_conformance_provenance.py`, `tests/integration/test_index_git_state.py`
- Modify: `tests/unit/test_conformance_report.py`,
  `tests/integration/test_cli_conformance.py`,
  `tests/fixtures/monorepo_conformance/golden/conformance.json` (regenerated), `CLAUDE.md`

---

### Task 1: Schema v11 + store surface (Rust)

**Files:**
- Create: `prograph-core/src/migrations/v11.sql`
- Modify: `prograph-core/src/store.rs`

**Interfaces:**
- Consumes: existing `SnapshotWriter` (tx-scoped inserts), `Store` query helpers.
- Produces (used by Task 2):
  - `SnapshotWriter::insert_project_git_state(&self, snapshot_id: i64, project_id: i64,
    git_commit: Option<&str>, git_dirty: Option<bool>) -> Result<()>`
  - `Store::project_git_states(&self, snapshot_id: i64)
    -> Result<Vec<(String, Option<String>, Option<bool>)>>` — `(project_name, commit,
    dirty)` sorted by name.

- [ ] **Step 1: Write the migration**

`prograph-core/src/migrations/v11.sql`:

```sql
-- prograph schema v11 — per-snapshot per-project git provenance
-- (conformance-report versioned evidence, spec 2026-08-03 D3).
CREATE TABLE IF NOT EXISTS project_git_states (
    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id),
    project_id  INTEGER NOT NULL REFERENCES projects(id),
    git_commit  TEXT,               -- HEAD sha at index time; NULL when not a git repo
    git_dirty   INTEGER,            -- 0/1; NULL when not a git repo
    PRIMARY KEY (snapshot_id, project_id)
);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (11, datetime('now'));
```

Register it in `prograph-core/src/store.rs` — append to the `MIGRATIONS` const array after
the v10 entry:

```rust
    (11, include_str!("migrations/v11.sql")),
```

- [ ] **Step 2: Write the failing Rust test** (inline `#[cfg(test)]` in `store.rs`, mirroring
the existing store tests' in-memory pattern)

```rust
#[test]
fn project_git_states_roundtrip_and_null_semantics() {
    let mut store = Store::open_in_memory().unwrap();
    let writer = store.begin_snapshot().unwrap();
    let snap = writer
        .insert_snapshot("2026-08-03T00:00:00Z", "/mono", None, "0.1.0")
        .unwrap();
    let pa = writer.insert_project(snap, "alpha", "./alpha", "python", "{}").unwrap();
    let pb = writer.insert_project(snap, "beta", "./beta", "python", "{}").unwrap();
    writer
        .insert_project_git_state(snap, pa, Some("abc123"), Some(false))
        .unwrap();
    writer.insert_project_git_state(snap, pb, None, None).unwrap();
    writer.commit().unwrap();

    let rows = store.project_git_states(snap).unwrap();
    assert_eq!(
        rows,
        vec![
            ("alpha".to_string(), Some("abc123".to_string()), Some(false)),
            ("beta".to_string(), None, None),
        ]
    );
    // Unknown snapshot id -> empty, not an error.
    assert!(store.project_git_states(snap + 99).unwrap().is_empty());
}
```

(Adapt the `Store::open_in_memory()` / `writer.commit()` call names to the file's existing
test helpers — use exactly what the neighbouring store tests use.)

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --all-targets project_git_states`
Expected: compile error (methods missing).

- [ ] **Step 4: Implement**

On `SnapshotWriter`:

```rust
/// v11: record a project's git state as captured at index time (spec D3).
pub fn insert_project_git_state(
    &self,
    snapshot_id: i64,
    project_id: i64,
    git_commit: Option<&str>,
    git_dirty: Option<bool>,
) -> Result<()> {
    self.tx.execute(
        "INSERT OR REPLACE INTO project_git_states
         (snapshot_id, project_id, git_commit, git_dirty)
         VALUES (?, ?, ?, ?)",
        rusqlite::params![
            snapshot_id,
            project_id,
            git_commit,
            git_dirty.map(|d| d as i64)
        ],
    )?;
    Ok(())
}
```

On `Store`:

```rust
/// v11: (project_name, git_commit, git_dirty) for a snapshot, sorted by name.
pub fn project_git_states(
    &self,
    snapshot_id: i64,
) -> Result<Vec<(String, Option<String>, Option<bool>)>> {
    let mut stmt = self.conn.prepare(
        "SELECT p.name, g.git_commit, g.git_dirty
         FROM project_git_states g
         JOIN projects p ON p.id = g.project_id
         WHERE g.snapshot_id = ?
         ORDER BY p.name",
    )?;
    let rows = stmt.query_map([snapshot_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<i64>>(2)?.map(|v| v != 0),
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
```

- [ ] **Step 5: Run tests, fmt, clippy, commit**

```sh
cargo test --all-targets
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings
git add prograph-core/src/migrations/v11.sql prograph-core/src/store.rs
git commit -m "feat(store): v11 per-snapshot project git provenance"
```

---

### Task 2: Indexer capture + PyO3 surface

**Files:**
- Modify: `prograph-core/src/indexer.rs`, `prograph-core/src/lib.rs`,
  `prograph-core/src/models.rs` (if pyclasses live there — follow where `DriftFindingRow`
  is defined), `prograph/_core.pyi`
- Test: inline Rust tests + Create `tests/integration/test_index_git_state.py`

**Interfaces:**
- Consumes: Task 1's store methods.
- Produces (used by Task 5):
  - Rust `fn detect_git_state(root: &Path) -> (Option<String>, Option<bool>)`
  - PyO3 `class ProjectGitStateRow { project_name: str, git_commit: str | None,
    git_dirty: bool | None }` and
    `def project_git_states(db_path: str, snapshot_id: int) -> list[ProjectGitStateRow]`
  - `.pyi` entries for both.

- [ ] **Step 1: Write the failing Rust test** (in `indexer.rs` tests, next to
`detect_git_commit_returns_none_for_non_git_dir`)

```rust
#[test]
fn detect_git_state_non_git_dir_is_none_none() {
    let dir = TempDir::new().unwrap();
    assert_eq!(detect_git_state(dir.path()), (None, None));
}

#[test]
fn detect_git_state_clean_and_dirty_repo() {
    use std::process::Command;
    let dir = TempDir::new().unwrap();
    let run = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args([
                "-c", "user.email=t@t", "-c", "user.name=t",
                "-c", "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {:?}", out);
    };
    run(&["init", "-q"]);
    std::fs::write(dir.path().join("f.txt"), "x").unwrap();
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "init"]);

    let (commit, dirty) = detect_git_state(dir.path());
    assert!(commit.is_some());
    assert_eq!(dirty, Some(false));

    std::fs::write(dir.path().join("f.txt"), "changed").unwrap();
    let (commit2, dirty2) = detect_git_state(dir.path());
    assert_eq!(commit2, commit); // commit recorded even when dirty — unlike detect_git_commit
    assert_eq!(dirty2, Some(true));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --all-targets detect_git_state` — compile error.

- [ ] **Step 3: Implement `detect_git_state`** (next to `detect_git_commit`, which stays
untouched — its dirty→None semantics are a snapshot-level reproducibility claim; this is
per-project evidence capture, spec D3)

```rust
/// Per-project git provenance at index time (spec D3): the commit is recorded even
/// when the tree is dirty — the separate dirty flag carries that fact. Both None when
/// the directory is not inside a git repository (or git is unavailable).
fn detect_git_state(root: &Path) -> (Option<String>, Option<bool>) {
    use std::process::Command;

    let status_out = match Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return (None, None),
    };
    let dirty = !status_out.stdout.is_empty();

    let commit = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    (commit, Some(dirty))
}
```

- [ ] **Step 4: Wire capture into the persist phase**

In `index_monorepo`'s Phase 5, `alive_projects` must be read **before**
`begin_snapshot()` (borrow rules — same pattern as `recent_changelog_labels` at
indexer.rs:220-222):

```rust
let alive_ids: HashMap<String, i64> = store
    .alive_projects()?
    .into_iter()
    .map(|(root, (id, _attrs))| (root, id))
    .collect();
```

After the project-diff loop (once `new_project_ids` is fully populated), for every indexed
fact resolve the project id and record its git state:

```rust
for fact in &facts {
    let Some(project_id) = new_project_ids
        .get(&fact.project_root)
        .copied()
        .or_else(|| alive_ids.get(&fact.project_root).copied())
    else {
        continue;
    };
    let rel = fact.project_root.strip_prefix("./").unwrap_or(&fact.project_root);
    let abs = monorepo_root.join(rel);
    let (commit, dirty) = detect_git_state(&abs);
    if dirty == Some(true) {
        // Machine-readable dirty warning (owner ruling, resolved question 2):
        // route the text exactly like declared.warnings and bump warning_count.
        let msg = format!(
            "{}: worktree dirty at index time — recorded provenance is not reproducible",
            fact.project_name
        );
        // <- push `msg` into the same sink indexer.rs uses for detection/declared
        //    warnings (see lines ~95-105) and `warning_count += 1`.
        let _ = msg;
    }
    writer.insert_project_git_state(snap_id, project_id, commit.as_deref(), dirty)?;
}
```

(The implementer must read indexer.rs:95-105 and route the warning text through the SAME
mechanism used for `declared.warnings` — do not invent a new sink; whatever happens to
those strings happens to this one.)

- [ ] **Step 5: PyO3 surface**

Define next to the other row pyclasses (follow `DriftFindingRow`'s file/pattern):

```rust
#[pyclass]
#[derive(Clone)]
pub struct ProjectGitStateRow {
    #[pyo3(get)]
    pub project_name: String,
    #[pyo3(get)]
    pub git_commit: Option<String>,
    #[pyo3(get)]
    pub git_dirty: Option<bool>,
}
```

Add the pyfunction (follow the existing query-helper pyfunctions in `lib.rs`):

```rust
#[pyfunction]
fn project_git_states(db_path: &str, snapshot_id: i64) -> PyResult<Vec<ProjectGitStateRow>> {
    let store = Store::open(Path::new(db_path)).map_err(to_py_err)?;
    let rows = store.project_git_states(snapshot_id).map_err(to_py_err)?;
    Ok(rows
        .into_iter()
        .map(|(project_name, git_commit, git_dirty)| ProjectGitStateRow {
            project_name,
            git_commit,
            git_dirty,
        })
        .collect())
}
```

(match the file's actual open/error-helper names), register
`m.add_class::<ProjectGitStateRow>()?;` and the `wrap_pyfunction!`, and append to
`prograph/_core.pyi`:

```python
class ProjectGitStateRow:
    project_name: str
    git_commit: str | None
    git_dirty: bool | None

def project_git_states(db_path: str, snapshot_id: int) -> list[ProjectGitStateRow]: ...
```

- [ ] **Step 6: Rebuild + Python integration test**

`uv run maturin develop`, then `tests/integration/test_index_git_state.py`:

```python
"""v11: per-project git provenance captured at index time."""

import shutil
import subprocess
from pathlib import Path

from typer.testing import CliRunner

from prograph import _core
from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_minimal"


def _git(cwd: Path, *args: str) -> None:
    subprocess.run(
        ["git", "-C", str(cwd), "-c", "user.email=t@t", "-c", "user.name=t",
         "-c", "commit.gpgsign=false", *args],
        check=True,
        capture_output=True,
    )


def test_git_state_captured_at_index_time(tmp_path: Path) -> None:
    dst = tmp_path / "mono"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    projects = sorted(p.name for p in dst.iterdir() if (p / "pyproject.toml").is_file())
    assert projects, "fixture must contain at least one python project"
    repo = dst / projects[0]
    _git(repo, "init", "-q")
    _git(repo, "add", ".")
    _git(repo, "commit", "-q", "-m", "init")

    assert runner.invoke(app, ["init", "--monorepo", str(dst)]).exit_code == 0
    assert runner.invoke(app, ["index", "--monorepo", str(dst)]).exit_code == 0
    db = str(dst / ".prograph" / "graph.db")
    snap = _core.latest_snapshot_info(db)
    assert snap is not None
    states = {s.project_name: s for s in _core.project_git_states(db, snap.id)}

    git_proj = states[projects[0]]
    assert git_proj.git_commit is not None and git_proj.git_dirty is False
    for name, st in states.items():
        if name != projects[0]:
            assert st.git_commit is None and st.git_dirty is None

    # Dirty the repo, reindex: dirty flag flips, commit stays recorded, warning counted.
    (repo / "dirty.txt").write_text("x", encoding="utf-8")
    before = _core.latest_snapshot_info(db)
    assert runner.invoke(app, ["index", "--monorepo", str(dst)]).exit_code == 0
    snap2 = _core.latest_snapshot_info(db)
    assert snap2 is not None and before is not None and snap2.id > before.id
    st2 = {s.project_name: s for s in _core.project_git_states(db, snap2.id)}[projects[0]]
    assert st2.git_dirty is True
    assert st2.git_commit == git_proj.git_commit
```

Run: `uv run pytest tests/integration/test_index_git_state.py -v` — PASS.

- [ ] **Step 7: Full Rust + Python gate, commit**

```sh
cargo test --all-targets
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings
uv run pytest -x -q
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph-core/src prograph/_core.pyi tests/integration/test_index_git_state.py
git commit -m "feat(indexer): capture per-project git state at index time (v11)"
```

---

### Task 3: `provenance.py` — content hash, clock, ReportProvenance

**Files:**
- Create: `prograph/conformance/provenance.py`
- Test: `tests/unit/test_conformance_provenance.py`

**Interfaces:**
- Consumes: `prograph._core` (`monorepo_overview`, `find_edges_filtered`,
  `latest_snapshot_info`, `project_git_states`, `project_by_name`, `describe_project`).
- Produces (used by Tasks 4–5):

```python
CANON_VERSION = "prograph-snapshot/v1"

def _utcnow() -> dt.datetime            # the injectable clock seam (D8)
def format_ts(t: dt.datetime) -> str    # "%Y-%m-%dT%H:%M:%SZ", UTC
def snapshot_content_hash(db_path: str) -> str   # "prograph-snapshot/v1+sha256:<hex>"

@dataclass(frozen=True)
class ReportProvenance:
    generated_at: str
    manifest_project: str | None
    manifest_path: str
    manifest_sha256: str
    snapshot_id: int
    snapshot_indexed_at: str
    snapshot_content_hash: str
    complete: bool
    tool_name: str
    tool_version: str
    tool_schema: str
    projects: Mapping[str, tuple[str | None, bool | None]]  # name -> (commit, dirty)

def build_provenance(
    db_path: str,
    monorepo_root: Path,
    manifest_path: Path,
    manifest_projects: Sequence[str],
    *,
    now: dt.datetime | None = None,
) -> ReportProvenance
```

- [ ] **Step 1: Write the failing tests**

`tests/unit/test_conformance_provenance.py`:

```python
"""Provenance assembly: content hash canon, clock formatting, dataclass shape."""

import datetime as dt
import shutil
from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app
from prograph.conformance.provenance import (
    CANON_VERSION,
    format_ts,
    snapshot_content_hash,
)

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_minimal"


def test_format_ts_utc_second_precision() -> None:
    t = dt.datetime(2026, 8, 3, 12, 0, 7, 123456, tzinfo=dt.timezone.utc)
    assert format_ts(t) == "2026-08-03T12:00:07Z"


def _indexed(tmp_path: Path) -> str:
    dst = tmp_path / "mono"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    assert runner.invoke(app, ["init", "--monorepo", str(dst)]).exit_code == 0
    assert runner.invoke(app, ["index", "--monorepo", str(dst)]).exit_code == 0
    return str(dst / ".prograph" / "graph.db")


def test_content_hash_versioned_and_deterministic(tmp_path: Path) -> None:
    db = _indexed(tmp_path)
    h1 = snapshot_content_hash(db)
    h2 = snapshot_content_hash(db)
    assert h1 == h2
    assert h1.startswith(f"{CANON_VERSION}+sha256:")
    assert len(h1.split(":", 1)[1]) == 64


def test_content_hash_same_content_different_snapshot_ids(tmp_path: Path) -> None:
    db = _indexed(tmp_path)
    h1 = snapshot_content_hash(db)
    # Re-index the unchanged tree: new snapshot id, identical structure -> same hash.
    mono = str(Path(db).parent.parent)
    assert runner.invoke(app, ["index", "--monorepo", mono]).exit_code == 0
    assert snapshot_content_hash(db) == h1
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/unit/test_conformance_provenance.py -v` — ModuleNotFoundError.

- [ ] **Step 3: Implement**

`prograph/conformance/provenance.py`:

```python
"""Report provenance (spec 2026-08-03: the conformance report as versioned evidence)."""

from __future__ import annotations

import datetime as dt
import hashlib
import json
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path

from prograph import __version__, _core

CANON_VERSION = "prograph-snapshot/v1"
TOOL_SCHEMA = "intended-graph/v1"


def _utcnow() -> dt.datetime:
    """Injectable clock seam (spec D8) — tests monkeypatch this, production uses it."""
    return dt.datetime.now(dt.timezone.utc)


def format_ts(t: dt.datetime) -> str:
    """RFC3339 second-precision UTC, matching the store's snapshot timestamps."""
    return t.astimezone(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def snapshot_content_hash(db_path: str) -> str:
    """Versioned canonical hash of the snapshot's node+edge sets (spec D4).

    Identity anchor, not freshness: identical observed structure hashes identically
    across snapshot ids; the canon version prefix keeps hashes from different
    serializations incomparable instead of falsely drifting.
    """
    overview = _core.monorepo_overview(db_path)
    projects = (
        sorted(
            ({"kind": p.kind, "name": p.name, "slug": p.slug} for p in overview.projects),
            key=lambda p: p["name"],
        )
        if overview is not None
        else []
    )
    edges = []
    for e in _core.find_edges_filtered(db_path):
        attrs = json.loads(e.attrs_json) if e.attrs_json else {}
        edges.append(
            {
                "attrs": attrs,
                "from": e.from_name,
                "kind": e.kind,
                "to": e.to_name,
                "to_kind": e.to_kind,
            }
        )
    edges.sort(key=lambda e: json.dumps(e, sort_keys=True))
    canon = json.dumps(
        {"edges": edges, "projects": projects}, sort_keys=True, separators=(",", ":")
    )
    return f"{CANON_VERSION}+sha256:{hashlib.sha256(canon.encode('utf-8')).hexdigest()}"


@dataclass(frozen=True)
class ReportProvenance:
    """Everything the D2 provenance block carries, assembled once in the CLI."""

    generated_at: str
    manifest_project: str | None
    manifest_path: str
    manifest_sha256: str
    snapshot_id: int
    snapshot_indexed_at: str
    snapshot_content_hash: str
    complete: bool
    tool_name: str
    tool_version: str
    tool_schema: str
    projects: Mapping[str, tuple[str | None, bool | None]]


def _resolve_manifest_project(
    db_path: str, monorepo_root: Path, manifest_path: Path
) -> tuple[str | None, str]:
    """(owning project, path relative to that project's root) — spec D2.

    Falls back to (None, path relative to the monorepo root or as given) when the
    manifest lies outside every indexed project root.
    """
    resolved = manifest_path.resolve()
    overview = _core.monorepo_overview(db_path)
    for p in overview.projects if overview is not None else []:
        pid = _core.project_by_name(db_path, p.name)
        desc = _core.describe_project(db_path, pid) if pid is not None else None
        if desc is None:
            continue
        root = (monorepo_root / desc.root_path.removeprefix("./")).resolve()
        try:
            return p.name, str(resolved.relative_to(root))
        except ValueError:
            continue
    try:
        return None, str(resolved.relative_to(monorepo_root.resolve()))
    except ValueError:
        return None, str(manifest_path)


def build_provenance(
    db_path: str,
    monorepo_root: Path,
    manifest_path: Path,
    manifest_projects: Sequence[str],
    *,
    now: dt.datetime | None = None,
) -> ReportProvenance:
    """Assemble the D2 provenance block for the latest snapshot."""
    snap = _core.latest_snapshot_info(db_path)
    if snap is None:
        raise ValueError(f"no snapshot in {db_path}")
    states = {
        s.project_name: (s.git_commit, s.git_dirty)
        for s in _core.project_git_states(db_path, snap.id)
    }
    project, rel_path = _resolve_manifest_project(db_path, monorepo_root, manifest_path)
    return ReportProvenance(
        generated_at=format_ts(now if now is not None else _utcnow()),
        manifest_project=project,
        manifest_path=rel_path,
        manifest_sha256=hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
        snapshot_id=snap.id,
        snapshot_indexed_at=snap.ts,
        snapshot_content_hash=snapshot_content_hash(db_path),
        complete=True,
        tool_name="prograph",
        tool_version=__version__,
        tool_schema=TOOL_SCHEMA,
        projects={name: states.get(name, (None, None)) for name in sorted(manifest_projects)},
    )
```

- [ ] **Step 4: Run tests, format, typecheck, commit**

```sh
uv run pytest tests/unit/test_conformance_provenance.py -v
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/conformance/provenance.py tests/unit/test_conformance_provenance.py
git commit -m "feat(conformance): provenance assembly — content hash, clock, dataclass"
```

---

### Task 4: Reshape the report payload

**Files:**
- Modify: `prograph/conformance/report.py`
- Test: `tests/unit/test_conformance_report.py` (rewrite the ARGS plumbing)

**Interfaces:**
- Consumes: `ReportProvenance` (Task 3), `ConformanceReport` (existing).
- Produces (used by Task 5): new signatures —

```python
def report_payload(report: ConformanceReport, provenance: ReportProvenance) -> dict[str, object]
def render_json(report: ConformanceReport, provenance: ReportProvenance) -> str
def render_text(report: ConformanceReport, provenance: ReportProvenance) -> str
```

Payload top-level (all D2 keys; `elements`/`findings`/`exceptions`/`summary` unchanged):

```json
{
  "schema": "conformance-report/v1",
  "system": "...",
  "generated_at": "2026-08-03T12:00:00Z",
  "manifest": {"project": "gamma", "path": "spec/intended-graph.yaml", "sha256": "..."},
  "snapshot": {"id": 1, "indexed_at": "2026-08-03T00:00:00Z",
                "content_hash": "prograph-snapshot/v1+sha256:...", "complete": true},
  "tool": {"name": "prograph", "version": "0.1.0", "schema": "intended-graph/v1"},
  "projects": {"alpha": {"commit": null, "dirty": null}}
}
```

- [ ] **Step 1: Update the tests** — in `tests/unit/test_conformance_report.py` replace the
`ARGS` dict with a `ReportProvenance` literal and extend the shape assertions:

```python
from prograph.conformance.provenance import ReportProvenance

PROV = ReportProvenance(
    generated_at="2026-08-03T12:00:00Z",
    manifest_project="gamma",
    manifest_path="spec/intended-graph.yaml",
    manifest_sha256="ab" * 32,
    snapshot_id=1,
    snapshot_indexed_at="2026-08-03T00:00:00Z",
    snapshot_content_hash="prograph-snapshot/v1+sha256:" + "cd" * 32,
    complete=True,
    tool_name="prograph",
    tool_version="0.1.0",
    tool_schema="intended-graph/v1",
    projects={"alpha": (None, None), "gamma": ("e" * 40, False)},
)


def test_payload_provenance_block() -> None:
    p = report_payload(REPORT, PROV)
    assert p["generated_at"] == "2026-08-03T12:00:00Z"
    assert p["manifest"] == {
        "project": "gamma", "path": "spec/intended-graph.yaml", "sha256": "ab" * 32,
    }
    assert p["snapshot"] == {
        "id": 1,
        "indexed_at": "2026-08-03T00:00:00Z",
        "content_hash": "prograph-snapshot/v1+sha256:" + "cd" * 32,
        "complete": True,
    }
    assert p["tool"] == {
        "name": "prograph", "version": "0.1.0", "schema": "intended-graph/v1",
    }
    assert p["projects"] == {
        "alpha": {"commit": None, "dirty": None},
        "gamma": {"commit": "e" * 40, "dirty": False},
    }
```

Every existing call `report_payload(REPORT, **ARGS)` / `render_json(REPORT, **ARGS)` /
`render_text(REPORT, **ARGS)` becomes `...(REPORT, PROV)`. The byte-stability test and the
text test keep their assertions (the text needles list gains `"generated"`).

- [ ] **Step 2: Run to verify failure** — signature mismatch.

- [ ] **Step 3: Implement** — in `report.py`: replace the three keyword-only provenance
parameters with `provenance: ReportProvenance`; build the payload's provenance keys from
the dataclass exactly as the JSON above; `render_text` header becomes:

```python
    lines: list[str] = [
        f"# Conformance: {report.system}",
        f"manifest: {provenance.manifest_path} (sha256 {provenance.manifest_sha256[:12]}…)"
        + (f" [project {provenance.manifest_project}]" if provenance.manifest_project else ""),
        f"snapshot: {provenance.snapshot_id} (indexed {provenance.snapshot_indexed_at})",
        f"generated: {provenance.generated_at}",
        "",
        "## Elements",
    ]
```

- [ ] **Step 4: Run tests, format, typecheck, commit**

```sh
uv run pytest tests/unit/test_conformance_report.py -v
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/conformance/report.py tests/unit/test_conformance_report.py
git commit -m "feat(conformance): D2 provenance block in the report payload"
```

---

### Task 5: CLI wiring + deterministic golden

**Files:**
- Modify: `prograph/cli.py`, `tests/integration/test_cli_conformance.py`
- Regenerate: `tests/fixtures/monorepo_conformance/golden/conformance.json`

**Interfaces:**
- Consumes: `build_provenance` (Task 3), new render signatures (Task 4).

- [ ] **Step 1: Wire the CLI** — in the `conformance` command replace the
`raw_snap`/`sha256`/`display_path` block and the render call with:

```python
    from prograph.conformance.provenance import build_provenance

    manifest_projects = sorted({c.project for c in loaded.components})
    try:
        prov = build_provenance(db, root, manifest_path, manifest_projects)
    except _json.JSONDecodeError as exc:
        tool_error(f"corrupted attrs_json in snapshot {paths.db_path}: {exc}")
        return  # unreachable
    except ValueError as exc:
        tool_error(str(exc))
        return  # unreachable

    render = render_json if format_ == "json" else render_text
    sys.stdout.write(render(report, prov))
    raise typer.Exit(code=exit_code(report, fail_on_set, verdict_set))
```

(`hashlib`, the old `display_path` logic and the `latest_snapshot_info` call in the command
body are removed — provenance owns them now. The existing
`except _json.JSONDecodeError` wrapper around `load_observed` stays.)

- [ ] **Step 2: Make the fixture deterministic** — in
`tests/integration/test_cli_conformance.py`, extend the module-scoped `indexed` fixture:
after the `index` invoke, pin the snapshot timestamp (the DB is the fixed input; no
production code is touched):

```python
import sqlite3

FIXED_INDEXED_AT = "2026-08-03T00:00:00Z"
FIXED_NOW = dt.datetime(2026, 8, 3, 12, 0, 0, tzinfo=dt.timezone.utc)

# inside the `indexed` fixture, after the index invoke:
    conn = sqlite3.connect(dst / ".prograph" / "graph.db")
    try:
        conn.execute("UPDATE snapshots SET ts = ?", (FIXED_INDEXED_AT,))
        conn.commit()
    finally:
        conn.close()
```

(add `import datetime as dt` to the file's imports). Fixture projects are not git repos, so
`projects.*` is deterministically `{commit: null, dirty: null}`; `content_hash` is
deterministic by construction.

- [ ] **Step 3: Freeze the clock where output is compared** — golden + byte-shape tests
monkeypatch the D8 seam (function-scoped `monkeypatch` works with the module-scoped
fixture):

```python
def test_json_matches_golden(indexed: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        "prograph.conformance.provenance._utcnow", lambda: FIXED_NOW
    )
    res = runner.invoke(
        app,
        ["conformance", "--monorepo", str(indexed), "--project", "gamma",
         "--format", "json"],
    )
    if os.environ.get("PROGRAPH_UPDATE_GOLDEN") == "1":
        GOLDEN.parent.mkdir(parents=True, exist_ok=True)
        GOLDEN.write_text(res.stdout, encoding="utf-8")
    assert res.stdout == GOLDEN.read_text(encoding="utf-8")
```

Add one provenance-content assertion to the default JSON run:

```python
def test_provenance_block_present(indexed: Path) -> None:
    _, payload = _json_run(indexed)
    assert payload["manifest"]["project"] == "gamma"
    assert payload["manifest"]["path"] == "spec/intended-graph.yaml"
    assert payload["snapshot"]["indexed_at"] == FIXED_INDEXED_AT
    assert payload["snapshot"]["complete"] is True
    assert payload["snapshot"]["content_hash"].startswith("prograph-snapshot/v1+sha256:")
    assert payload["tool"] == {
        "name": "prograph", "version": "0.1.0", "schema": "intended-graph/v1",
    }
    assert set(payload["projects"]) == {"alpha", "beta", "delta", "gamma"}
    assert payload["projects"]["gamma"] == {"commit": None, "dirty": None}
```

(`delta` appears because it is a manifest component's project — outside the workspace, so
`{null, null}` per D3.)

- [ ] **Step 4: Regenerate the golden, review by eye**

```sh
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest \
  tests/integration/test_cli_conformance.py::test_json_matches_golden -v
git diff tests/fixtures/monorepo_conformance/golden/conformance.json
```

Expected diff: ONLY the new top-level `generated_at`, the reshaped `manifest`
(`+project`), the reshaped `snapshot` (`+indexed_at`, `+content_hash`, `+complete`), new
`tool` and `projects` blocks. Elements/findings/exceptions/summary byte-identical. Anything
else — stop and investigate.

- [ ] **Step 5: Full suite, format, typecheck, commit**

```sh
uv run pytest -v
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/cli.py tests/integration/test_cli_conformance.py \
  tests/fixtures/monorepo_conformance/golden/conformance.json
git commit -m "feat(conformance): wire provenance into the CLI; deterministic golden"
```

---

### Task 6: Published contract schemas + sync tests

**Files:**
- Create: `contracts/intended-graph/v1/schema.json`,
  `contracts/conformance-report/v1/schema.json`
- Test: `tests/unit/test_contract_schemas.py`

**Interfaces:**
- Consumes: fixtures (`monorepo_conformance`, `ws005_manifest`), `report_payload` outputs.
- Produces: the two vendorable artifacts steward pins (spec D6). The guarantee is
  **structural only** — integrity checks stay in the loader; the sync test asserts the
  boundary in both directions.

- [ ] **Step 1: Add the dev dependency**

```sh
uv add --dev jsonschema
```

- [ ] **Step 2: Write `contracts/intended-graph/v1/schema.json`** (structural mirror of
`conformance/manifest.py`'s models — strict everywhere):

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "intended-graph/v1",
  "title": "Intended graph manifest v1 (structural schema)",
  "description": "Structural layer only. Cross-object integrity (global id uniqueness, endpoint existence, exception-target resolvability, constraint rule grammar, the two-file-endpoint ban) is enforced by prograph's loader and attested by a successfully produced conformance report.",
  "type": "object",
  "additionalProperties": false,
  "required": ["schema", "system", "components"],
  "properties": {
    "schema": {"const": "intended-graph/v1"},
    "system": {"type": "string"},
    "components": {
      "type": "array",
      "minItems": 1,
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "project", "kind", "owner", "responsibility"],
        "properties": {
          "id": {"type": "string"},
          "project": {"type": "string"},
          "kind": {"enum": ["service", "module", "cli", "ui", "contract", "store"]},
          "owner": {"type": "string"},
          "responsibility": {"type": "string"},
          "scope": {"type": "string"},
          "evidence": {"type": "array", "items": {"type": "string"}}
        }
      }
    },
    "interfaces": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "producer", "consumer", "detector"],
        "properties": {
          "id": {"type": "string"},
          "producer": {"type": "string"},
          "consumer": {"type": "string"},
          "detector": {"enum": ["import", "mcp", "contract", "declared", "manual-evidence"]},
          "protocol": {"type": "string"},
          "evidence": {"type": "array", "items": {"type": "string"}}
        }
      }
    },
    "constraints": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "rule", "detector"],
        "properties": {
          "id": {"type": "string"},
          "rule": {"type": "string"},
          "detector": {"enum": ["import", "mcp", "contract", "declared", "manual-evidence"]},
          "evidence": {"type": "array", "items": {"type": "string"}}
        }
      }
    },
    "resources": {"type": "array", "items": {"type": "string"}},
    "exceptions": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "target", "reason", "owner", "expires"],
        "properties": {
          "id": {"type": "string"},
          "target": {"type": "string"},
          "reason": {"type": "string"},
          "owner": {"type": "string"},
          "expires": {"type": "string", "format": "date"}
        }
      }
    }
  }
}
```

- [ ] **Step 3: Write `contracts/conformance-report/v1/schema.json`**:

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "conformance-report/v1",
  "title": "Conformance report v1 (versioned evidence)",
  "type": "object",
  "additionalProperties": false,
  "required": ["schema", "system", "generated_at", "manifest", "snapshot", "tool",
                "projects", "elements", "findings", "exceptions", "summary"],
  "properties": {
    "schema": {"const": "conformance-report/v1"},
    "system": {"type": "string"},
    "generated_at": {"type": "string"},
    "manifest": {
      "type": "object",
      "additionalProperties": false,
      "required": ["project", "path", "sha256"],
      "properties": {
        "project": {"type": ["string", "null"]},
        "path": {"type": "string"},
        "sha256": {"type": "string", "pattern": "^[0-9a-f]{64}$"}
      }
    },
    "snapshot": {
      "type": "object",
      "additionalProperties": false,
      "required": ["id", "indexed_at", "content_hash", "complete"],
      "properties": {
        "id": {"type": "integer"},
        "indexed_at": {"type": "string"},
        "content_hash": {
          "type": "string",
          "pattern": "^prograph-snapshot/v1\\+sha256:[0-9a-f]{64}$"
        },
        "complete": {"type": "boolean"}
      }
    },
    "tool": {
      "type": "object",
      "additionalProperties": false,
      "required": ["name", "version", "schema"],
      "properties": {
        "name": {"type": "string"},
        "version": {"type": "string"},
        "schema": {"const": "intended-graph/v1"}
      }
    },
    "projects": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "additionalProperties": false,
        "required": ["commit", "dirty"],
        "properties": {
          "commit": {"type": ["string", "null"]},
          "dirty": {"type": ["boolean", "null"]}
        }
      }
    },
    "elements": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "type", "detector", "verdict", "reason", "waived_by"],
        "properties": {
          "id": {"type": "string"},
          "type": {"enum": ["interface", "constraint"]},
          "detector": {"enum": ["import", "mcp", "contract", "declared", "manual-evidence"]},
          "verdict": {"enum": ["conformant", "violation", "unknown"]},
          "reason": {
            "enum": ["manual-evidence", "unsupported-resolution", "outside-workspace",
                      "orphan-component", null]
          },
          "waived_by": {"type": ["string", "null"]}
        }
      }
    },
    "findings": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["class", "element", "detail", "suppressed_by"],
        "properties": {
          "class": {
            "enum": ["missing-required-edge", "forbidden-edge", "undeclared-edge",
                      "orphan-component", "expired-waiver", "manual-obligation"]
          },
          "element": {"type": ["string", "null"]},
          "detail": {"type": "string"},
          "suppressed_by": {"type": ["string", "null"]}
        }
      }
    },
    "exceptions": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "target", "expires", "status"],
        "properties": {
          "id": {"type": "string"},
          "target": {"type": "string"},
          "expires": {"type": "string"},
          "status": {"enum": ["active", "expired"]}
        }
      }
    },
    "summary": {
      "type": "object",
      "additionalProperties": false,
      "required": ["verdicts", "findings"],
      "properties": {
        "verdicts": {
          "type": "object",
          "additionalProperties": false,
          "required": ["conformant", "violation", "unknown"],
          "properties": {
            "conformant": {"type": "integer"},
            "violation": {"type": "integer"},
            "unknown": {"type": "integer"}
          }
        },
        "findings": {
          "type": "object",
          "additionalProperties": false,
          "required": ["missing-required-edge", "forbidden-edge", "undeclared-edge",
                        "orphan-component", "expired-waiver", "manual-obligation"],
          "properties": {
            "missing-required-edge": {"type": "integer"},
            "forbidden-edge": {"type": "integer"},
            "undeclared-edge": {"type": "integer"},
            "orphan-component": {"type": "integer"},
            "expired-waiver": {"type": "integer"},
            "manual-obligation": {"type": "integer"}
          }
        }
      }
    }
  }
}
```

- [ ] **Step 4: Write the sync tests**

`tests/unit/test_contract_schemas.py`:

```python
"""D6 sync tests: the published contracts ARE what the code implements — structural layer."""

import json
from pathlib import Path

import jsonschema
import pytest
import yaml

from prograph.conformance.manifest import ManifestError, load_manifest
from prograph.conformance.report import report_payload

REPO = Path(__file__).resolve().parent.parent.parent
MANIFEST_SCHEMA = json.loads(
    (REPO / "contracts" / "intended-graph" / "v1" / "schema.json").read_text(encoding="utf-8")
)
REPORT_SCHEMA = json.loads(
    (REPO / "contracts" / "conformance-report" / "v1" / "schema.json").read_text(
        encoding="utf-8"
    )
)


def _yaml_as_json(path: Path) -> object:
    """YAML -> JSON-types canonicalization (dates become ISO strings)."""
    raw = yaml.safe_load(path.read_text(encoding="utf-8"))
    return json.loads(json.dumps(raw, default=str))


ACCEPTED_MANIFESTS = [
    REPO / "tests" / "fixtures" / "monorepo_conformance" / "gamma" / "spec"
    / "intended-graph.yaml",
    REPO / "tests" / "fixtures" / "monorepo_conformance" / "green-manifest.yaml",
    REPO / "tests" / "fixtures" / "ws005_manifest" / "intended-graph.yaml",
]


@pytest.mark.parametrize("path", ACCEPTED_MANIFESTS, ids=lambda p: p.parent.name)
def test_loader_accepted_manifests_validate_structurally(path: Path) -> None:
    load_manifest(path)  # loader accepts (raises otherwise)
    jsonschema.validate(_yaml_as_json(path), MANIFEST_SCHEMA)


def test_structural_rejections_agree(tmp_path: Path) -> None:
    """Structural defects: loader rejects AND schema rejects."""
    base = _yaml_as_json(ACCEPTED_MANIFESTS[1])
    assert isinstance(base, dict)
    for mutate in (
        lambda d: d.update(extra_key="boom"),
        lambda d: d.update(schema="intended-graph/v2"),
        lambda d: d["components"][0].update(kind="banana"),
        lambda d: d["components"][0].pop("owner"),
    ):
        doc = json.loads(json.dumps(base))
        mutate(doc)
        with pytest.raises(jsonschema.ValidationError):
            jsonschema.validate(doc, MANIFEST_SCHEMA)
        p = tmp_path / "m.yaml"
        p.write_text(yaml.safe_dump(doc), encoding="utf-8")
        with pytest.raises(ManifestError):
            load_manifest(p)


def test_integrity_rejections_pass_schema_documenting_the_boundary(tmp_path: Path) -> None:
    """Integrity-only defects: loader rejects, schema PASSES — the documented D6 split."""
    base = _yaml_as_json(ACCEPTED_MANIFESTS[1])
    assert isinstance(base, dict)
    dup = json.loads(json.dumps(base))
    dup["components"].append(dict(dup["components"][0]))  # duplicate id
    dangling = json.loads(json.dumps(base))
    dangling["interfaces"] = [
        {"id": "I-90", "producer": "no.such", "consumer": dangling["components"][0]["id"],
         "detector": "import"}
    ]
    for doc in (dup, dangling):
        jsonschema.validate(doc, MANIFEST_SCHEMA)  # structurally fine
        p = tmp_path / "m.yaml"
        p.write_text(yaml.safe_dump(doc), encoding="utf-8")
        with pytest.raises(ManifestError):
            load_manifest(p)


def test_report_payloads_validate() -> None:
    from tests.unit.test_conformance_report import PROV, REPORT

    jsonschema.validate(report_payload(REPORT, PROV), REPORT_SCHEMA)


def test_golden_report_validates() -> None:
    golden = REPO / "tests" / "fixtures" / "monorepo_conformance" / "golden" / (
        "conformance.json"
    )
    jsonschema.validate(json.loads(golden.read_text(encoding="utf-8")), REPORT_SCHEMA)
```

(If `from tests.unit.test_conformance_report import ...` fails under the repo's pytest
config, move `REPORT`/`PROV` construction into a small local helper duplicating the
literals — note it in the report.)

- [ ] **Step 5: Run, format, typecheck, commit**

```sh
uv run pytest tests/unit/test_contract_schemas.py -v
uv run pytest -q
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add contracts tests/unit/test_contract_schemas.py pyproject.toml uv.lock
git commit -m "feat(contracts): publish intended-graph/v1 + conformance-report/v1 schemas"
```

---

### Task 7: Docs + full gate + PR

**Files:**
- Modify: `CLAUDE.md`, `docs/superpowers/specs/2026-08-03-conformance-report-provenance-design.md`

- [ ] **Step 1: `CLAUDE.md` updates**

1. Architecture, Rust list: migrations line becomes `migrations/v1.sql..v11.sql` and note
   `v11 = per-snapshot project git provenance`; store query helpers gain
   `project_git_states`.
2. Architecture, Python list: `conformance/` bullet gains `provenance.py` (content hash +
   report provenance, injectable clock).
3. New top-level subsection after the golden-tests block:

```markdown
### Published contracts

`contracts/intended-graph/v1/schema.json` and `contracts/conformance-report/v1/schema.json`
are the vendorable structural schemas consumers pin (steward `GC-ARCH-*`). They are
**structural only** — cross-object integrity lives in `conformance/manifest.py`; sync with
the code is enforced by `tests/unit/test_contract_schemas.py` in both directions. Change
either side only together with the other.
```

4. Plans list: add `- Report provenance plan:
   docs/superpowers/plans/2026-08-03-conformance-report-provenance.md`.

- [ ] **Step 2: Flip the spec status line** to
`Status: **Approved (#25) — implemented (this branch)**` keeping the review-history note.

- [ ] **Step 3: Full gate, commit, PR**

```sh
cargo test --all-targets && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings
uv run pytest -v
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add CLAUDE.md docs/superpowers/specs/2026-08-03-conformance-report-provenance-design.md
git commit -m "docs: report provenance shipped — CLAUDE.md + spec status"
git push -u origin feat/report-provenance
gh pr create --title "feat: conformance report provenance + published contract schemas (spec #25)" \
  --body "Implements docs/superpowers/specs/2026-08-03-conformance-report-provenance-design.md ..."
```

Then action the Copilot review; do NOT merge (owner merges). After merge: notify steward
(their `GC-ARCH-*` work can now vendor both schemas and consume the provenance block —
follow-up comment on steward#36, not a new issue).

---

## Self-Review (performed while writing)

- **Spec coverage:** D1 (payload extended in place, frozen after — Task 4), D2 (exact block
  — Tasks 3–5, asserted in unit + integration + schema), D3 (v11 + index-time capture,
  `detect_git_state` distinct from `detect_git_commit`, null semantics for non-git and
  outside-workspace — Tasks 1–2; dirty warning routed through the existing sink per
  resolved question 2), D4 (versioned canon prefix, identity-not-freshness docstring —
  Task 3), D5 (`complete: True` producer assertion — Task 3), D6 (two schemas, structural
  boundary asserted in BOTH directions — Task 6), D7 (prograph only carries facts; no
  policy code anywhere in this plan), D8 (`_utcnow` seam, monkeypatched in tests, no CLI
  flag, no normalization — golden literally byte-exact via frozen clock + DB-pinned
  `indexed_at` — Tasks 3, 5).
- **Placeholder scan:** none — every step has executable content; the one intentional
  implementer-judgment point (warning sink routing) names the exact lines to mirror.
- **Type consistency:** `ReportProvenance` fields match between Task 3 definition, Task 4
  payload/tests, Task 5 CLI and Task 6 schema; `project_git_states` shapes match between
  store (tuples), PyO3 row class, `.pyi`, and `build_provenance` consumption;
  `format_ts`/`current_iso_ts` produce the same `YYYY-MM-DDTHH:MM:SSZ` format.
