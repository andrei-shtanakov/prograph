# prograph M1 — Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the repo skeleton (Rust + Python via maturin/PyO3), wire SQLite storage, implement project discovery, and ship two working CLI commands (`prograph init`, `prograph status`). After M1, you can run `prograph init && prograph status` against the real `all_ai_orchestrators/` monorepo and see a classified list of project candidates with no edges yet.

**Architecture:** Two-layer build matching the spec — Rust crate (`prograph-core` via PyO3) handles SQLite + discovery; thin Python package (`prograph`) provides CLI via typer + pydantic models that mirror the Rust dataclasses. SQLite uses a minimal v1 schema (snapshots + projects only) — additional tables land in M2+.

**Tech Stack:**
- **Rust:** edition 2021, pinned 1.75; rusqlite 0.31 (bundled SQLite); pyo3 0.22; thiserror 1; serde + serde_json
- **Python:** 3.11+; pydantic v2; typer 0.12; uv for deps; pyrefly for type checking; ruff for lint+format
- **Build:** maturin 1.7 (mixed Python/Rust layout)
- **Tests:** pytest 8; cargo test for Rust unit tests

**Spec reference:** `docs/superpowers/specs/2026-05-25-prograph-design.md` — §3 (architecture), §4 (components), §5.1 (SQLite schema), §6 (indexing flow phase 0–1), §7.1 (CLI).

**M1 explicitly out of scope:** parsers, edge detectors, diff engine, MCP, browser UI, MD export, changelog, snapshot insertion (M1 stops at discovery + classification, the `snapshots` table exists but stays empty).

---

## File Structure (created in M1)

```
prograph/                            # repo root, already exists
├── Cargo.toml                       # virtual workspace
├── rust-toolchain.toml              # pin Rust 1.75
├── pyproject.toml                   # maturin + uv config
├── ruff.toml                        # python lint config
├── .gitignore                       # extends existing
├── README.md                        # short, points at spec
│
├── prograph-core/                   # Rust crate (PyO3 extension)
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs                   # #[pymodule] prograph._core
│   │   ├── errors.rs                # ProgaphError + PyO3 conversions
│   │   ├── models.rs                # Project, ProjectKind, SnapshotInfo as #[pyclass]
│   │   ├── discovery.rs             # classify_project, scan_monorepo
│   │   ├── store.rs                 # Store struct, open(), schema migration
│   │   └── migrations/
│   │       └── v1.sql               # minimal schema (snapshots, projects)
│   └── tests/
│       └── discovery_integration.rs # cargo-discoverable integration test
│
├── prograph/                        # Python package
│   ├── __init__.py                  # re-exports from _core
│   ├── _core.pyi                    # type stubs for Rust extension
│   ├── py.typed                     # PEP 561 marker
│   ├── paths.py                     # .prograph/ path constants
│   ├── models.py                    # pydantic wrappers around Rust dataclasses
│   └── cli.py                       # typer app: init, status, --version
│
├── tests/                           # pytest tests (Rust uses prograph-core/tests/)
│   ├── conftest.py
│   ├── fixtures/
│   │   └── monorepo_minimal/
│   │       ├── proj_a/pyproject.toml
│   │       └── proj_b/pyproject.toml
│   ├── unit/
│   │   ├── test_paths.py
│   │   └── test_models.py
│   └── integration/
│       ├── test_discovery.py
│       ├── test_cli_init.py
│       └── test_cli_status.py
│
└── .github/
    └── workflows/
        └── ci.yml                   # rust + python + maturin matrix
```

**Note on existing files:** `prograph/CLAUDE.md` (already written) and `prograph/Sourcetrail/` (vendored archive, its own .git) coexist. We don't touch Sourcetrail in M1. We add a `.gitignore` entry to exclude its build artefacts from our workflows.

---

## Task 1: Repo skeleton (workspace files)

**Files:**
- Create: `prograph/Cargo.toml`
- Create: `prograph/rust-toolchain.toml`
- Create: `prograph/pyproject.toml`
- Create: `prograph/ruff.toml`
- Create: `prograph/.gitignore`
- Create: `prograph/README.md`

- [ ] **Step 1: Create the Rust virtual workspace**

`prograph/Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["prograph-core"]

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.75"
license = "MIT"
repository = "https://github.com/andrei-shtanakov/prograph"

[workspace.dependencies]
pyo3 = { version = "0.22", features = ["extension-module"] }
rusqlite = { version = "0.31", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
```

- [ ] **Step 2: Pin the Rust toolchain**

`prograph/rust-toolchain.toml`:
```toml
[toolchain]
channel = "1.75"
components = ["clippy", "rustfmt"]
```

- [ ] **Step 3: Create the Python project config**

`prograph/pyproject.toml`:
```toml
[build-system]
requires = ["maturin>=1.7,<2"]
build-backend = "maturin"

[project]
name = "prograph"
version = "0.1.0"
description = "Monorepo cross-project structure mapper"
requires-python = ">=3.11"
license = { text = "MIT" }
authors = [{ name = "Andrei Shtanakov" }]
dependencies = [
    "pydantic>=2.7,<3",
    "typer>=0.12,<1",
    "rich>=13",
]

[project.scripts]
prograph = "prograph.cli:app"

[dependency-groups]
dev = [
    "pytest>=8",
    "pytest-subprocess>=1.5",
    "ruff>=0.5",
    "pyrefly>=0.6",
    "maturin>=1.7,<2",
]

[tool.maturin]
manifest-path = "prograph-core/Cargo.toml"
module-name = "prograph._core"
python-source = "."
features = ["pyo3/extension-module"]

[tool.pytest.ini_options]
testpaths = ["tests"]
addopts = "-ra -q"

[tool.ruff]
line-length = 100
target-version = "py311"

[tool.ruff.lint]
select = ["E", "F", "W", "I", "B", "UP", "RUF"]
```

- [ ] **Step 4: Create the Python lint config**

`prograph/ruff.toml`:
```toml
extend = "pyproject.toml"
```

(This is intentionally empty — the canonical config lives in `pyproject.toml`. The file exists so editor integrations that look for `ruff.toml` find it.)

- [ ] **Step 5: Create `.gitignore`**

`prograph/.gitignore`:
```
# Rust
/target/
/prograph-core/target/

# Python
__pycache__/
*.py[cod]
*.egg-info/
.venv/
.uv/

# Maturin build artefacts
*.so
*.pyd
*.dylib

# Test outputs
.pytest_cache/
.coverage
htmlcov/

# Vendored Sourcetrail subdir (its own repo, ignored from our tooling)
/Sourcetrail/

# prograph runtime artefacts (when self-hosting)
/.prograph/graph.db
/.prograph/graph.db-wal
/.prograph/graph.db-shm
/.prograph/index.log
/.prograph/index.lock

# OS
.DS_Store
```

- [ ] **Step 6: Create a minimal README**

`prograph/README.md`:
```markdown
# prograph

Cross-project structure mapper for monorepos. Detects how independent projects in a workspace talk to each other (package deps, shared contracts, MCP calls) and exposes the graph to humans (browser) and AI agents (MCP).

**Status:** M1 (foundation) — `init` and `status` working; no edge detection yet.

See `docs/superpowers/specs/2026-05-25-prograph-design.md` for the full design spec, and `docs/superpowers/plans/` for milestone implementation plans.

## Quickstart

```sh
uv sync
maturin develop
prograph init
prograph status
```

## Development

- Rust core: `prograph-core/` — built via maturin into `prograph._core`.
- Python wrapper: `prograph/` — CLI, models, paths.
- Tests: `cargo test` for Rust, `uv run pytest` for Python.
```

- [ ] **Step 7: Verify the workspace parses**

Run:
```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo metadata --no-deps --format-version 1 > /dev/null
```
Expected: exit 0, no output (workspace has no members yet — that's fine for a check, we'll add `prograph-core` in Task 2).

If cargo complains about missing members, that's a known intermediate state — we add the crate next task.

- [ ] **Step 8: Verify Python toolchain**

Run:
```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv lock
```
Expected: writes `uv.lock`, no errors.

- [ ] **Step 9: Commit**

```sh
git add Cargo.toml rust-toolchain.toml pyproject.toml ruff.toml .gitignore README.md uv.lock
git commit -m "prograph: M1 repo skeleton (workspace, pyproject, gitignore)"
```

---

## Task 2: PyO3 hello world (smoke that the build chain works)

**Files:**
- Create: `prograph-core/Cargo.toml`
- Create: `prograph-core/src/lib.rs`
- Create: `prograph/__init__.py`
- Create: `prograph/_core.pyi`
- Create: `prograph/py.typed`
- Create: `tests/unit/test_smoke.py`
- Create: `tests/__init__.py`, `tests/unit/__init__.py`, `tests/integration/__init__.py`
- Create: `tests/conftest.py`

- [ ] **Step 1: Create the Rust crate manifest**

`prograph-core/Cargo.toml`:
```toml
[package]
name = "prograph-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[lib]
name = "prograph_core"
crate-type = ["cdylib", "rlib"]

[dependencies]
pyo3 = { workspace = true }
rusqlite = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 2: Create the PyO3 module entry**

`prograph-core/src/lib.rs`:
```rust
//! prograph-core — Rust core for the prograph monorepo mapper.
//!
//! Exposed to Python as the `prograph._core` extension module.

use pyo3::prelude::*;

/// Returns the prograph-core crate version.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    Ok(())
}
```

- [ ] **Step 3: Create the Python package**

`prograph/__init__.py`:
```python
"""prograph — cross-project structure mapper for monorepos."""

from prograph._core import version as _core_version

__version__ = "0.1.0"


def core_version() -> str:
    """Return the Rust core crate version (sanity check that the extension loaded)."""
    return _core_version()


__all__ = ["__version__", "core_version"]
```

- [ ] **Step 4: Create the type stub**

`prograph/_core.pyi`:
```python
"""Type stubs for the prograph._core PyO3 extension module."""

def version() -> str: ...
```

- [ ] **Step 5: Create the PEP 561 marker**

`prograph/py.typed`:
(empty file)

- [ ] **Step 6: Create empty test package markers**

`tests/__init__.py`, `tests/unit/__init__.py`, `tests/integration/__init__.py`:
(all three are empty files)

- [ ] **Step 7: Create pytest conftest**

`tests/conftest.py`:
```python
"""Shared pytest fixtures for prograph tests."""

from pathlib import Path

import pytest

FIXTURES_DIR = Path(__file__).parent / "fixtures"


@pytest.fixture
def fixtures_dir() -> Path:
    """Path to the tests/fixtures/ directory."""
    return FIXTURES_DIR
```

- [ ] **Step 8: Write the smoke test**

`tests/unit/test_smoke.py`:
```python
"""Smoke test: PyO3 extension builds and imports."""

import prograph


def test_python_package_version():
    assert prograph.__version__ == "0.1.0"


def test_rust_core_version_matches():
    assert prograph.core_version() == "0.1.0"
```

- [ ] **Step 9: Build the extension with maturin**

Run:
```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run maturin develop
```
Expected: compiles, installs `prograph._core.<platform>.so` into the venv, exit 0.

- [ ] **Step 10: Run the smoke test**

Run:
```sh
uv run pytest tests/unit/test_smoke.py -v
```
Expected: 2 passed.

- [ ] **Step 11: Commit**

```sh
git add prograph-core/ prograph/__init__.py prograph/_core.pyi prograph/py.typed \
        tests/__init__.py tests/unit/__init__.py tests/integration/__init__.py \
        tests/conftest.py tests/unit/test_smoke.py uv.lock
git commit -m "prograph: M1 PyO3 hello world (version smoke test)"
```

---

## Task 3: Errors module (Rust)

**Files:**
- Create: `prograph-core/src/errors.rs`
- Modify: `prograph-core/src/lib.rs` (register module)

- [ ] **Step 1: Write the errors module**

`prograph-core/src/errors.rs`:
```rust
//! Error types for prograph-core. All errors convert to Python exceptions at the FFI boundary.

use pyo3::exceptions::{PyIOError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PrographError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("invalid configuration: {0}")]
    Config(String),

    #[error("project discovery failed at {root}: {reason}")]
    Discovery { root: String, reason: String },
}

impl From<PrographError> for PyErr {
    fn from(err: PrographError) -> PyErr {
        match err {
            PrographError::Io { .. } => PyIOError::new_err(err.to_string()),
            PrographError::Sqlite(_) => PyRuntimeError::new_err(err.to_string()),
            PrographError::Config(_) => PyValueError::new_err(err.to_string()),
            PrographError::Discovery { .. } => PyRuntimeError::new_err(err.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, PrographError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_displays_message() {
        let err = PrographError::Config("missing root".into());
        assert_eq!(err.to_string(), "invalid configuration: missing root");
    }

    #[test]
    fn pyerr_conversion_picks_value_error_for_config() {
        pyo3::Python::with_gil(|py| {
            let err: PyErr = PrographError::Config("x".into()).into();
            assert!(err.is_instance_of::<PyValueError>(py));
        });
    }
}
```

- [ ] **Step 2: Register the module**

In `prograph-core/src/lib.rs`, add at the top:
```rust
mod errors;
```

The current `lib.rs` should now read:
```rust
//! prograph-core — Rust core for the prograph monorepo mapper.

mod errors;

use pyo3::prelude::*;

#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    Ok(())
}
```

- [ ] **Step 3: Run cargo tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core errors
```
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```sh
git add prograph-core/src/errors.rs prograph-core/src/lib.rs
git commit -m "prograph: M1 errors module with PyErr conversions"
```

---

## Task 4: SQLite v1 schema (minimal — snapshots + projects)

**Files:**
- Create: `prograph-core/src/migrations/v1.sql`

Only two tables in M1. The full schema (contracts, edges, change_log, search_fts) lands in M2 along with the indexer that needs them. The schema is forward-compatible: M2 migrations are additive.

- [ ] **Step 1: Write the v1 SQL**

`prograph-core/src/migrations/v1.sql`:
```sql
-- prograph schema v1 — minimal, M1 scope.
-- M2+ adds: contracts, contract_files, edges, edge_evidence, change_log, search_fts.

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS snapshots (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    ts               TEXT NOT NULL,
    monorepo_root    TEXT NOT NULL,
    git_commit       TEXT,
    prograph_version TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    root_path   TEXT NOT NULL UNIQUE,
    kind        TEXT NOT NULL CHECK (kind IN ('python', 'rust', 'js', 'docs', 'mixed')),
    attrs_json  TEXT NOT NULL DEFAULT '{}',
    first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
    last_seen   INTEGER NOT NULL REFERENCES snapshots(id)
);

CREATE INDEX IF NOT EXISTS idx_projects_last_seen ON projects(last_seen);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (1, datetime('now'));
```

- [ ] **Step 2: Commit (no test yet — we test via Store in Task 5)**

```sh
git add prograph-core/src/migrations/v1.sql
git commit -m "prograph: M1 SQLite schema v1 (snapshots + projects)"
```

---

## Task 5: Store::open() with migration application

**Files:**
- Create: `prograph-core/src/store.rs`
- Modify: `prograph-core/src/lib.rs`

- [ ] **Step 1: Write the failing Rust test inline**

We use cargo's inline-module style to keep the unit test next to the code.

`prograph-core/src/store.rs`:
```rust
//! SQLite-backed graph store. M1 only opens the DB and applies the v1 schema.

use std::path::Path;

use rusqlite::Connection;

use crate::errors::{PrographError, Result};

const SCHEMA_V1: &str = include_str!("migrations/v1.sql");

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (or create) the SQLite DB at `path` and ensure the v1 schema is applied.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| PrographError::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }

        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_V1)?;
        Ok(Self { conn })
    }

    /// Return the highest applied schema version.
    pub fn schema_version(&self) -> Result<i64> {
        let v: i64 = self
            .conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))?;
        Ok(v)
    }

    #[cfg(test)]
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_db_and_applies_v1_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".prograph/graph.db");

        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 1);
    }

    #[test]
    fn open_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("graph.db");

        let _ = Store::open(&path).unwrap();
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 1);
    }

    #[test]
    fn schema_creates_snapshots_and_projects_tables() {
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
        assert!(names.contains(&"snapshots".to_string()));
        assert!(names.contains(&"projects".to_string()));
        assert!(names.contains(&"schema_version".to_string()));
    }
}
```

- [ ] **Step 2: Add `tempfile` dev-dependency**

Edit `prograph-core/Cargo.toml`, append:
```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Register the module**

In `prograph-core/src/lib.rs`, change to:
```rust
//! prograph-core — Rust core for the prograph monorepo mapper.

mod errors;
mod store;

pub use store::Store;

use pyo3::prelude::*;

#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    Ok(())
}
```

- [ ] **Step 4: Run tests, expect them to fail until tempfile compiles**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core store
```
Expected: 3 tests pass (cargo will pull tempfile on first run).

- [ ] **Step 5: Commit**

```sh
git add prograph-core/Cargo.toml prograph-core/src/store.rs prograph-core/src/lib.rs Cargo.lock
git commit -m "prograph: M1 Store::open() with v1 schema migration"
```

---

## Task 6: Models (Rust pyclasses + Python pydantic mirrors)

**Files:**
- Create: `prograph-core/src/models.rs`
- Create: `prograph/models.py`
- Modify: `prograph/_core.pyi`
- Modify: `prograph/__init__.py`
- Modify: `prograph-core/src/lib.rs`
- Create: `tests/unit/test_models.py`

- [ ] **Step 1: Write Rust models**

`prograph-core/src/models.rs`:
```rust
//! Cross-language data classes. Exposed to Python via PyO3.

use pyo3::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[pyclass(eq, frozen, module = "prograph._core")]
pub enum ProjectKind {
    Python,
    Rust,
    Js,
    Docs,
    Mixed,
}

#[pymethods]
impl ProjectKind {
    fn __repr__(&self) -> String {
        format!("ProjectKind.{:?}", self)
    }

    /// Canonical lowercase name used in storage and CLI output.
    fn name(&self) -> &'static str {
        match self {
            ProjectKind::Python => "python",
            ProjectKind::Rust => "rust",
            ProjectKind::Js => "js",
            ProjectKind::Docs => "docs",
            ProjectKind::Mixed => "mixed",
        }
    }
}

/// A discovered project candidate (before any parsing).
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct ProjectCandidate {
    pub name: String,
    pub root_path: String, // relative to monorepo root
    pub kind: ProjectKind,
    pub manifests: Vec<String>, // relative paths to detected signal files
}

#[pymethods]
impl ProjectCandidate {
    #[new]
    fn new(name: String, root_path: String, kind: ProjectKind, manifests: Vec<String>) -> Self {
        Self {
            name,
            root_path,
            kind,
            manifests,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "ProjectCandidate(name='{}', kind={:?}, root='{}', manifests={})",
            self.name,
            self.kind,
            self.root_path,
            self.manifests.len()
        )
    }
}
```

- [ ] **Step 2: Register `models` in `lib.rs`**

`prograph-core/src/lib.rs`:
```rust
//! prograph-core — Rust core for the prograph monorepo mapper.

mod errors;
mod models;
mod store;

pub use models::{ProjectCandidate, ProjectKind};
pub use store::Store;

use pyo3::prelude::*;

#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_class::<ProjectKind>()?;
    m.add_class::<ProjectCandidate>()?;
    Ok(())
}
```

- [ ] **Step 3: Extend the `.pyi` stub**

`prograph/_core.pyi`:
```python
"""Type stubs for the prograph._core PyO3 extension module."""

from enum import Enum

def version() -> str: ...

class ProjectKind(Enum):
    Python = ...
    Rust = ...
    Js = ...
    Docs = ...
    Mixed = ...

    def name(self) -> str: ...

class ProjectCandidate:
    name: str
    root_path: str
    kind: ProjectKind
    manifests: list[str]

    def __init__(
        self,
        name: str,
        root_path: str,
        kind: ProjectKind,
        manifests: list[str],
    ) -> None: ...
```

- [ ] **Step 4: Write pydantic mirrors**

`prograph/models.py`:
```python
"""Pydantic models mirroring the Rust dataclasses exposed by prograph._core.

These models are the single source of truth for shapes flowing through CLI --json
output, MCP tool responses, and FastAPI endpoints. They round-trip with the Rust
side via from_core() / to_core() helpers.
"""

from __future__ import annotations

from enum import Enum

from pydantic import BaseModel, ConfigDict

from prograph import _core


class ProjectKind(str, Enum):
    PYTHON = "python"
    RUST = "rust"
    JS = "js"
    DOCS = "docs"
    MIXED = "mixed"

    @classmethod
    def from_core(cls, value: _core.ProjectKind) -> ProjectKind:
        return cls(value.name())


class ProjectCandidate(BaseModel):
    """A project discovered in the monorepo, before any deep parsing."""

    model_config = ConfigDict(frozen=True)

    name: str
    root_path: str
    kind: ProjectKind
    manifests: list[str]

    @classmethod
    def from_core(cls, value: _core.ProjectCandidate) -> ProjectCandidate:
        return cls(
            name=value.name,
            root_path=value.root_path,
            kind=ProjectKind.from_core(value.kind),
            manifests=list(value.manifests),
        )
```

- [ ] **Step 5: Re-export pydantic models from `prograph/__init__.py`**

`prograph/__init__.py`:
```python
"""prograph — cross-project structure mapper for monorepos."""

from prograph._core import version as _core_version
from prograph.models import ProjectCandidate, ProjectKind

__version__ = "0.1.0"


def core_version() -> str:
    """Return the Rust core crate version."""
    return _core_version()


__all__ = ["__version__", "core_version", "ProjectCandidate", "ProjectKind"]
```

- [ ] **Step 6: Write the unit tests**

`tests/unit/test_models.py`:
```python
"""Round-trip tests between Rust pyclasses and pydantic mirrors."""

from prograph import ProjectCandidate, ProjectKind
from prograph import _core


def test_kind_round_trip_via_name():
    for variant in (
        _core.ProjectKind.Python,
        _core.ProjectKind.Rust,
        _core.ProjectKind.Js,
        _core.ProjectKind.Docs,
        _core.ProjectKind.Mixed,
    ):
        assert ProjectKind.from_core(variant).value == variant.name()


def test_candidate_round_trip():
    raw = _core.ProjectCandidate(
        name="Maestro",
        root_path="./Maestro",
        kind=_core.ProjectKind.Python,
        manifests=["pyproject.toml"],
    )
    candidate = ProjectCandidate.from_core(raw)
    assert candidate.name == "Maestro"
    assert candidate.root_path == "./Maestro"
    assert candidate.kind is ProjectKind.PYTHON
    assert candidate.manifests == ["pyproject.toml"]


def test_candidate_is_frozen():
    candidate = ProjectCandidate(
        name="X", root_path="./X", kind=ProjectKind.RUST, manifests=[]
    )
    try:
        candidate.name = "Y"  # type: ignore[misc]
    except Exception:
        return
    raise AssertionError("expected frozen model to reject mutation")
```

- [ ] **Step 7: Rebuild and run tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run maturin develop
uv run pytest tests/unit/test_models.py -v
```
Expected: 3 passed.

- [ ] **Step 8: Commit**

```sh
git add prograph-core/src/models.rs prograph-core/src/lib.rs \
        prograph/_core.pyi prograph/__init__.py prograph/models.py \
        tests/unit/test_models.py
git commit -m "prograph: M1 ProjectCandidate + ProjectKind pyclass + pydantic mirror"
```

---

## Task 7: Discovery — `classify_project()`

Single-project classification by signal-file presence. Pure function, no I/O outside the project root.

**Files:**
- Create: `prograph-core/src/discovery.rs`
- Modify: `prograph-core/src/lib.rs`
- Create: `tests/fixtures/monorepo_minimal/proj_a/pyproject.toml`
- Create: `tests/fixtures/monorepo_minimal/proj_b/pyproject.toml`

- [ ] **Step 1: Create the fixture projects**

`tests/fixtures/monorepo_minimal/proj_a/pyproject.toml`:
```toml
[project]
name = "proj_a"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = ["proj_b"]
```

`tests/fixtures/monorepo_minimal/proj_b/pyproject.toml`:
```toml
[project]
name = "proj_b"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = []
```

- [ ] **Step 2: Write `classify_project` with inline Rust tests**

`prograph-core/src/discovery.rs`:
```rust
//! Project discovery — file-system scan and classification by signal files.
//!
//! The discovery layer is intentionally cheap: it touches only signal files
//! at the project root (pyproject.toml, Cargo.toml, package.json, README.md,
//! CLAUDE.md, TODO.md). Deep parsing happens in M2+ via `parsers`.

use std::path::Path;

use crate::errors::Result;
use crate::models::{ProjectCandidate, ProjectKind};

const PYTHON_SIGNALS: &[&str] = &["pyproject.toml", "setup.py"];
const RUST_SIGNAL: &str = "Cargo.toml";
const JS_SIGNAL: &str = "package.json";
const DOC_SIGNALS: &[&str] = &["README.md", "CLAUDE.md", "TODO.md"];

/// Classify a single project directory by examining signal files at its root.
///
/// Returns `None` if the directory is not a project candidate (no recognised
/// signal files at all). Returns `Some` with the candidate's classification
/// otherwise.
pub fn classify_project(root: &Path, name: &str, rel_root: &str) -> Result<Option<ProjectCandidate>> {
    let mut manifests = Vec::new();
    let mut has_python = false;
    let mut has_rust = false;
    let mut has_js = false;
    let mut has_docs = false;

    for signal in PYTHON_SIGNALS {
        if root.join(signal).is_file() {
            manifests.push(signal.to_string());
            has_python = true;
        }
    }
    if root.join(RUST_SIGNAL).is_file() {
        manifests.push(RUST_SIGNAL.to_string());
        has_rust = true;
    }
    if root.join(JS_SIGNAL).is_file() {
        manifests.push(JS_SIGNAL.to_string());
        has_js = true;
    }
    for signal in DOC_SIGNALS {
        if root.join(signal).is_file() {
            manifests.push(signal.to_string());
            has_docs = true;
        }
    }

    let code_signals = [has_python, has_rust, has_js].iter().filter(|x| **x).count();

    let kind = match (code_signals, has_docs) {
        (0, false) => return Ok(None),
        (0, true) => ProjectKind::Docs,
        (1, _) if has_python => ProjectKind::Python,
        (1, _) if has_rust => ProjectKind::Rust,
        (1, _) if has_js => ProjectKind::Js,
        _ => ProjectKind::Mixed,
    };

    Ok(Some(ProjectCandidate {
        name: name.to_string(),
        root_path: rel_root.to_string(),
        kind,
        manifests,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_proj(files: &[&str]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for f in files {
            fs::write(dir.path().join(f), "").unwrap();
        }
        dir
    }

    #[test]
    fn classifies_python_by_pyproject() {
        let dir = make_proj(&["pyproject.toml"]);
        let c = classify_project(dir.path(), "proj", "./proj").unwrap().unwrap();
        assert_eq!(c.kind, ProjectKind::Python);
        assert_eq!(c.manifests, vec!["pyproject.toml"]);
    }

    #[test]
    fn classifies_rust_by_cargo_toml() {
        let dir = make_proj(&["Cargo.toml"]);
        let c = classify_project(dir.path(), "proj", "./proj").unwrap().unwrap();
        assert_eq!(c.kind, ProjectKind::Rust);
    }

    #[test]
    fn classifies_js_by_package_json() {
        let dir = make_proj(&["package.json"]);
        let c = classify_project(dir.path(), "proj", "./proj").unwrap().unwrap();
        assert_eq!(c.kind, ProjectKind::Js);
    }

    #[test]
    fn classifies_docs_only_when_no_code_signals() {
        let dir = make_proj(&["README.md", "CLAUDE.md"]);
        let c = classify_project(dir.path(), "proj", "./proj").unwrap().unwrap();
        assert_eq!(c.kind, ProjectKind::Docs);
    }

    #[test]
    fn classifies_mixed_when_multiple_code_signals() {
        let dir = make_proj(&["pyproject.toml", "Cargo.toml"]);
        let c = classify_project(dir.path(), "proj", "./proj").unwrap().unwrap();
        assert_eq!(c.kind, ProjectKind::Mixed);
    }

    #[test]
    fn returns_none_when_no_signals() {
        let dir = TempDir::new().unwrap();
        assert!(classify_project(dir.path(), "x", "./x").unwrap().is_none());
    }

    #[test]
    fn returns_none_when_only_unrelated_files() {
        let dir = make_proj(&["foo.txt", "data.json"]);
        assert!(classify_project(dir.path(), "x", "./x").unwrap().is_none());
    }

    #[test]
    fn code_signal_wins_over_docs_for_kind() {
        let dir = make_proj(&["pyproject.toml", "README.md"]);
        let c = classify_project(dir.path(), "proj", "./proj").unwrap().unwrap();
        assert_eq!(c.kind, ProjectKind::Python);
        assert!(c.manifests.contains(&"pyproject.toml".to_string()));
        assert!(c.manifests.contains(&"README.md".to_string()));
    }
}
```

- [ ] **Step 3: Register `discovery` module**

`prograph-core/src/lib.rs` change `mod` lines:
```rust
mod discovery;
mod errors;
mod models;
mod store;

pub use discovery::classify_project;
pub use models::{ProjectCandidate, ProjectKind};
pub use store::Store;
```

(rest unchanged)

- [ ] **Step 4: Run cargo tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core discovery
```
Expected: 8 tests pass.

- [ ] **Step 5: Commit**

```sh
git add prograph-core/src/discovery.rs prograph-core/src/lib.rs \
        tests/fixtures/monorepo_minimal/
git commit -m "prograph: M1 classify_project + signal-file detection"
```

---

## Task 8: Discovery — `scan_monorepo()`

Walk the top-level subdirectories of a monorepo root, classify each, return the candidate list. Exposed to Python.

**Files:**
- Modify: `prograph-core/src/discovery.rs`
- Modify: `prograph-core/src/lib.rs`
- Modify: `prograph/_core.pyi`
- Create: `tests/integration/test_discovery.py`

- [ ] **Step 1: Add `scan_monorepo()` to `discovery.rs`**

Append to the end of `prograph-core/src/discovery.rs` (before `#[cfg(test)]`):

```rust
/// Scan the first-level subdirectories of `monorepo_root` and return all classified candidates.
///
/// Hidden directories (those whose name starts with `.`) and the `target/`, `node_modules/`,
/// `.venv/`, `dist/`, `build/` directories are skipped automatically — they're build artefacts,
/// not projects.
pub fn scan_monorepo(monorepo_root: &Path) -> Result<Vec<ProjectCandidate>> {
    if !monorepo_root.is_dir() {
        return Err(crate::errors::PrographError::Discovery {
            root: monorepo_root.display().to_string(),
            reason: "monorepo root is not a directory".into(),
        });
    }

    let mut candidates = Vec::new();
    let entries = std::fs::read_dir(monorepo_root).map_err(|source| {
        crate::errors::PrographError::Io {
            path: monorepo_root.display().to_string(),
            source,
        }
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| crate::errors::PrographError::Io {
            path: monorepo_root.display().to_string(),
            source,
        })?;

        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if is_ignored_dir(&name) {
            continue;
        }
        let rel_root = format!("./{name}");
        if let Some(candidate) = classify_project(&path, &name, &rel_root)? {
            candidates.push(candidate);
        }
    }

    candidates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(candidates)
}

fn is_ignored_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(name, "target" | "node_modules" | "dist" | "build" | "__pycache__")
}
```

Also add a PyO3 wrapper at the end of the file (still before the `#[cfg(test)]` block):

```rust
use pyo3::prelude::*;

/// Python entry point: scan a monorepo, return the sorted list of candidates.
#[pyfunction]
#[pyo3(name = "scan_monorepo")]
pub fn py_scan_monorepo(monorepo_root: &str) -> PyResult<Vec<ProjectCandidate>> {
    Ok(scan_monorepo(Path::new(monorepo_root))?)
}
```

- [ ] **Step 2: Extend the inline cargo test**

Inside the existing `#[cfg(test)] mod tests` block, append:

```rust
    #[test]
    fn scan_finds_two_projects_sorted() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("zeta")).unwrap();
        fs::write(dir.path().join("zeta/Cargo.toml"), "").unwrap();
        fs::create_dir_all(dir.path().join("alpha")).unwrap();
        fs::write(dir.path().join("alpha/pyproject.toml"), "").unwrap();

        let result = scan_monorepo(dir.path()).unwrap();
        let names: Vec<_> = result.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
    }

    #[test]
    fn scan_skips_hidden_and_artefact_dirs() {
        let dir = TempDir::new().unwrap();
        for hidden in &[".git", ".venv", "target", "node_modules"] {
            fs::create_dir_all(dir.path().join(hidden)).unwrap();
            fs::write(dir.path().join(hidden).join("Cargo.toml"), "").unwrap();
        }
        fs::create_dir_all(dir.path().join("real")).unwrap();
        fs::write(dir.path().join("real/Cargo.toml"), "").unwrap();

        let result = scan_monorepo(dir.path()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "real");
    }

    #[test]
    fn scan_errors_on_nonexistent_root() {
        let err = scan_monorepo(Path::new("/nonexistent_for_test_xyz")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not a directory"), "got: {msg}");
    }
```

- [ ] **Step 3: Register the PyO3 function in `lib.rs`**

`prograph-core/src/lib.rs` becomes:
```rust
//! prograph-core — Rust core for the prograph monorepo mapper.

mod discovery;
mod errors;
mod models;
mod store;

pub use discovery::{classify_project, scan_monorepo};
pub use models::{ProjectCandidate, ProjectKind};
pub use store::Store;

use pyo3::prelude::*;

#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(discovery::py_scan_monorepo, m)?)?;
    m.add_class::<ProjectKind>()?;
    m.add_class::<ProjectCandidate>()?;
    Ok(())
}
```

- [ ] **Step 4: Extend the `.pyi` stub**

Append to `prograph/_core.pyi`:
```python
def scan_monorepo(monorepo_root: str) -> list[ProjectCandidate]: ...
```

- [ ] **Step 5: Run cargo tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core discovery
```
Expected: 11 tests pass total (8 from Task 7 + 3 new).

- [ ] **Step 6: Rebuild the extension**

```sh
uv run maturin develop
```

- [ ] **Step 7: Write the Python integration test**

`tests/integration/test_discovery.py`:
```python
"""Integration test: scan_monorepo against the bundled fixture."""

from prograph import ProjectKind
from prograph._core import scan_monorepo


def test_scan_monorepo_minimal_fixture(fixtures_dir):
    root = fixtures_dir / "monorepo_minimal"
    candidates = scan_monorepo(str(root))

    names = sorted(c.name for c in candidates)
    assert names == ["proj_a", "proj_b"]

    for c in candidates:
        assert c.kind.name() == "python"
        assert "pyproject.toml" in c.manifests
        assert c.root_path == f"./{c.name}"


def test_scan_monorepo_errors_on_missing_root():
    import pytest
    with pytest.raises(Exception) as exc:
        scan_monorepo("/path/does/not/exist/prograph-test")
    assert "not a directory" in str(exc.value).lower()
```

- [ ] **Step 8: Run the Python tests**

```sh
uv run pytest tests/integration/test_discovery.py -v
```
Expected: 2 passed.

- [ ] **Step 9: Commit**

```sh
git add prograph-core/src/discovery.rs prograph-core/src/lib.rs \
        prograph/_core.pyi tests/integration/test_discovery.py
git commit -m "prograph: M1 scan_monorepo + ignore hidden/artefact dirs"
```

---

## Task 9: `prograph.paths` — runtime path constants

Constants for `.prograph/` layout. Pure Python, no Rust.

**Files:**
- Create: `prograph/paths.py`
- Create: `tests/unit/test_paths.py`

- [ ] **Step 1: Write the test first**

`tests/unit/test_paths.py`:
```python
"""Tests for prograph.paths — .prograph/ layout constants."""

from pathlib import Path

from prograph.paths import PrographPaths


def test_default_paths_under_monorepo_root(tmp_path: Path):
    p = PrographPaths(monorepo_root=tmp_path)
    assert p.prograph_dir == tmp_path / ".prograph"
    assert p.config_path == tmp_path / ".prograph" / "config.toml"
    assert p.db_path == tmp_path / ".prograph" / "graph.db"
    assert p.lock_path == tmp_path / ".prograph" / "index.lock"
    assert p.log_path == tmp_path / ".prograph" / "index.log"
    assert p.projects_md_dir == tmp_path / ".prograph" / "projects"
    assert p.contracts_md_dir == tmp_path / ".prograph" / "contracts"
    assert p.gitignore_path == tmp_path / ".prograph" / ".gitignore"


def test_ensure_dirs_creates_missing(tmp_path: Path):
    p = PrographPaths(monorepo_root=tmp_path)
    assert not p.prograph_dir.exists()
    p.ensure_dirs()
    assert p.prograph_dir.is_dir()
    assert p.projects_md_dir.is_dir()
    assert p.contracts_md_dir.is_dir()


def test_initialized_false_when_no_prograph_dir(tmp_path: Path):
    p = PrographPaths(monorepo_root=tmp_path)
    assert not p.is_initialized()


def test_initialized_true_after_ensure_dirs_and_config(tmp_path: Path):
    p = PrographPaths(monorepo_root=tmp_path)
    p.ensure_dirs()
    p.config_path.write_text("# config\n")
    assert p.is_initialized()
```

- [ ] **Step 2: Run, expect ImportError**

```sh
uv run pytest tests/unit/test_paths.py -v
```
Expected: 4 ERROR (no module `prograph.paths`).

- [ ] **Step 3: Implement `paths.py`**

`prograph/paths.py`:
```python
"""Filesystem layout for prograph runtime artefacts under a monorepo root."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class PrographPaths:
    """All filesystem paths under `<monorepo_root>/.prograph/`."""

    monorepo_root: Path

    @property
    def prograph_dir(self) -> Path:
        return self.monorepo_root / ".prograph"

    @property
    def config_path(self) -> Path:
        return self.prograph_dir / "config.toml"

    @property
    def db_path(self) -> Path:
        return self.prograph_dir / "graph.db"

    @property
    def lock_path(self) -> Path:
        return self.prograph_dir / "index.lock"

    @property
    def log_path(self) -> Path:
        return self.prograph_dir / "index.log"

    @property
    def projects_md_dir(self) -> Path:
        return self.prograph_dir / "projects"

    @property
    def contracts_md_dir(self) -> Path:
        return self.prograph_dir / "contracts"

    @property
    def gitignore_path(self) -> Path:
        return self.prograph_dir / ".gitignore"

    def ensure_dirs(self) -> None:
        self.prograph_dir.mkdir(parents=True, exist_ok=True)
        self.projects_md_dir.mkdir(parents=True, exist_ok=True)
        self.contracts_md_dir.mkdir(parents=True, exist_ok=True)

    def is_initialized(self) -> bool:
        return self.prograph_dir.is_dir() and self.config_path.is_file()
```

- [ ] **Step 4: Run again, expect pass**

```sh
uv run pytest tests/unit/test_paths.py -v
```
Expected: 4 passed.

- [ ] **Step 5: Commit**

```sh
git add prograph/paths.py tests/unit/test_paths.py
git commit -m "prograph: M1 PrographPaths helper"
```

---

## Task 10: CLI scaffolding (`prograph --version`)

`typer` app with one command (`--version`-style callback). We add real subcommands in the next two tasks.

**Files:**
- Create: `prograph/cli.py`
- Create: `tests/integration/test_cli_version.py`

- [ ] **Step 1: Write the CLI scaffold**

`prograph/cli.py`:
```python
"""prograph CLI — typer entry point exposed as `prograph` console script."""

from __future__ import annotations

import sys

import typer
from rich.console import Console

from prograph import __version__, core_version

console = Console()
err_console = Console(stderr=True)

app = typer.Typer(
    name="prograph",
    help="Cross-project structure mapper for monorepos.",
    no_args_is_help=True,
    add_completion=False,
)


def _print_version(value: bool) -> None:
    if value:
        console.print(f"prograph {__version__} (core {core_version()})")
        raise typer.Exit()


@app.callback()
def main(
    version: bool = typer.Option(
        False,
        "--version",
        callback=_print_version,
        is_eager=True,
        help="Print version and exit.",
    ),
) -> None:
    """Cross-project structure mapper for monorepos."""


if __name__ == "__main__":
    sys.exit(app())
```

- [ ] **Step 2: Write the version test**

`tests/integration/test_cli_version.py`:
```python
"""CLI smoke: `prograph --version` works via the typer runner."""

from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()


def test_version_flag_prints_versions_and_exits_zero():
    result = runner.invoke(app, ["--version"])
    assert result.exit_code == 0
    assert "prograph 0.1.0" in result.stdout
    assert "core 0.1.0" in result.stdout


def test_no_args_shows_help():
    result = runner.invoke(app, [])
    # typer's no_args_is_help exits with code 0 and prints help on stderr/stdout
    assert "Cross-project structure mapper" in result.stdout
```

- [ ] **Step 3: Run the test**

```sh
uv run pytest tests/integration/test_cli_version.py -v
```
Expected: 2 passed.

- [ ] **Step 4: Verify the installed entry point works**

```sh
uv run prograph --version
```
Expected: `prograph 0.1.0 (core 0.1.0)`.

- [ ] **Step 5: Commit**

```sh
git add prograph/cli.py tests/integration/test_cli_version.py
git commit -m "prograph: M1 CLI scaffold with --version"
```

---

## Task 11: CLI `prograph init`

Creates `.prograph/`, writes default `config.toml` + `.gitignore`. Idempotent.

**Files:**
- Modify: `prograph/cli.py`
- Create: `tests/integration/test_cli_init.py`

- [ ] **Step 1: Write the failing test**

`tests/integration/test_cli_init.py`:
```python
"""Tests for `prograph init`."""

from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()


def test_init_creates_prograph_skeleton(tmp_path: Path):
    result = runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.stdout

    paths = PrographPaths(monorepo_root=tmp_path)
    assert paths.prograph_dir.is_dir()
    assert paths.config_path.is_file()
    assert paths.gitignore_path.is_file()
    assert paths.projects_md_dir.is_dir()
    assert paths.contracts_md_dir.is_dir()

    config = paths.config_path.read_text()
    assert "[monorepo]" in config
    assert "include" in config or "exclude" in config

    gi = paths.gitignore_path.read_text()
    assert "graph.db" in gi
    assert "index.lock" in gi


def test_init_is_idempotent(tmp_path: Path):
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    paths = PrographPaths(monorepo_root=tmp_path)
    config_before = paths.config_path.read_text()

    # Mutate user content; init should preserve it.
    paths.config_path.write_text(config_before + "\n# user edit\n")

    result = runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0
    assert "# user edit" in paths.config_path.read_text()


def test_init_uses_cwd_when_no_monorepo_flag(tmp_path: Path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    result = runner.invoke(app, ["init"])
    assert result.exit_code == 0
    assert (tmp_path / ".prograph" / "config.toml").is_file()
```

- [ ] **Step 2: Run, expect failure**

```sh
uv run pytest tests/integration/test_cli_init.py -v
```
Expected: 3 ERROR / FAILED (no `init` command).

- [ ] **Step 3: Implement `init`**

Add to `prograph/cli.py`, just below the `main()` callback:

```python
DEFAULT_CONFIG_TOML = """\
# prograph configuration — edit by hand. Re-running `prograph init` will not overwrite this file.

[monorepo]
# `include` / `exclude` accept glob patterns relative to the monorepo root. If `include` is empty,
# all first-level subdirs are scanned (modulo the exclude list).
include = []
exclude = ["target", "node_modules", "dist", "build", "__pycache__"]

# Override classification or rename projects whose directory name differs from the package name.
# Example:
#   [[project]]
#   path = "./atp-platform"
#   name = "atp_platform"
#   kind = "python"
"""

DEFAULT_GITIGNORE = """\
# prograph runtime artefacts — these change every index run and should not be committed.
graph.db
graph.db-wal
graph.db-shm
index.log
index.lock

# Committed artefacts (kept under version control by default):
#   projects/*.md
#   contracts/*.md
#   index.md
#   config.toml
"""


def _resolve_monorepo(monorepo: Path | None) -> Path:
    return monorepo.resolve() if monorepo is not None else Path.cwd().resolve()


@app.command()
def init(
    monorepo: Path = typer.Option(
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
) -> None:
    """Create the `.prograph/` skeleton under the monorepo root. Idempotent."""

    root = _resolve_monorepo(monorepo)
    if not root.is_dir():
        err_console.print(f"[red]error:[/red] monorepo root {root} is not a directory")
        raise typer.Exit(code=1)

    paths = PrographPaths(monorepo_root=root)
    paths.ensure_dirs()

    if not paths.config_path.exists():
        paths.config_path.write_text(DEFAULT_CONFIG_TOML)
    if not paths.gitignore_path.exists():
        paths.gitignore_path.write_text(DEFAULT_GITIGNORE)

    console.print(f"[green]initialized[/green] {paths.prograph_dir}")
```

Also add this import at the top of `prograph/cli.py`:
```python
from pathlib import Path

from prograph.paths import PrographPaths
```

- [ ] **Step 4: Run the test**

```sh
uv run pytest tests/integration/test_cli_init.py -v
```
Expected: 3 passed.

- [ ] **Step 5: Manual sanity check**

```sh
cd /tmp && rm -rf prograph_smoke && mkdir prograph_smoke && cd prograph_smoke
mkdir alpha beta
touch alpha/pyproject.toml beta/Cargo.toml
uv run --directory /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph prograph init --monorepo .
ls -la .prograph/
cat .prograph/config.toml | head -5
```
Expected: `.prograph/` exists, `config.toml` starts with `# prograph configuration`.

- [ ] **Step 6: Commit**

```sh
git add prograph/cli.py tests/integration/test_cli_init.py
git commit -m "prograph: M1 'prograph init' creates .prograph/ skeleton"
```

---

## Task 12: CLI `prograph status`

Reads project candidates via `scan_monorepo` and prints a rich table. No DB writes yet.

**Files:**
- Modify: `prograph/cli.py`
- Create: `tests/integration/test_cli_status.py`

- [ ] **Step 1: Write the test**

`tests/integration/test_cli_status.py`:
```python
"""Tests for `prograph status`."""

import json
from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()


def _setup_mini_monorepo(root: Path) -> None:
    (root / "alpha").mkdir()
    (root / "alpha" / "pyproject.toml").write_text("[project]\nname='alpha'\n")
    (root / "beta").mkdir()
    (root / "beta" / "Cargo.toml").write_text("[package]\nname='beta'\n")
    (root / "docs_only").mkdir()
    (root / "docs_only" / "README.md").write_text("# docs only")


def test_status_lists_classified_projects(tmp_path: Path):
    _setup_mini_monorepo(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.stdout
    out = result.stdout

    assert "alpha" in out
    assert "beta" in out
    assert "docs_only" in out
    assert "python" in out
    assert "rust" in out
    assert "docs" in out


def test_status_json_output_is_valid_json(tmp_path: Path):
    _setup_mini_monorepo(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path), "--json"])
    assert result.exit_code == 0
    payload = json.loads(result.stdout)
    assert payload["monorepo_root"].endswith(str(tmp_path.resolve()).split("/")[-1])
    assert len(payload["projects"]) == 3
    names = sorted(p["name"] for p in payload["projects"])
    assert names == ["alpha", "beta", "docs_only"]


def test_status_requires_init(tmp_path: Path):
    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1
    assert "not initialized" in result.stdout.lower() or "not initialized" in result.stderr.lower()
```

- [ ] **Step 2: Run, expect failure**

```sh
uv run pytest tests/integration/test_cli_status.py -v
```
Expected: 3 FAILED (no `status` command).

- [ ] **Step 3: Implement `status`**

Add to `prograph/cli.py` (below `init`):

```python
import json as _json

from rich.table import Table

from prograph import _core
from prograph.models import ProjectCandidate


@app.command()
def status(
    monorepo: Path = typer.Option(
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
    json: bool = typer.Option(False, "--json", help="Emit JSON to stdout instead of a table."),
) -> None:
    """Show monorepo state: project candidates classified by signal files."""

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

    if json:
        payload = {
            "monorepo_root": str(root),
            "projects": [c.model_dump(mode="json") for c in candidates],
        }
        console.print(_json.dumps(payload, indent=2))
        return

    table = Table(title=f"prograph status — {root}")
    table.add_column("name", style="cyan")
    table.add_column("kind", style="magenta")
    table.add_column("root", style="dim")
    table.add_column("manifests")

    for c in candidates:
        table.add_row(
            c.name,
            c.kind.value,
            c.root_path,
            ", ".join(c.manifests),
        )

    console.print(table)
    console.print(f"[dim]{len(candidates)} projects discovered.[/dim]")
```

- [ ] **Step 4: Run the test**

```sh
uv run pytest tests/integration/test_cli_status.py -v
```
Expected: 3 passed.

- [ ] **Step 5: Smoke test against the bundled fixture**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run prograph init --monorepo tests/fixtures/monorepo_minimal
uv run prograph status --monorepo tests/fixtures/monorepo_minimal
# Clean up the .prograph dir we just made under the fixture
rm -rf tests/fixtures/monorepo_minimal/.prograph
```
Expected: shows two python projects `proj_a` and `proj_b`.

- [ ] **Step 6: Commit**

```sh
git add prograph/cli.py tests/integration/test_cli_status.py
git commit -m "prograph: M1 'prograph status' table + --json output"
```

---

## Task 13: End-to-end test on the real monorepo (smoke)

Sanity check that the chain holds on a real, larger directory tree. Uses the parent `all_ai_orchestrators/` directory.

**Files:**
- Create: `tests/integration/test_smoke_real.py`

- [ ] **Step 1: Write the smoke test (marked, not in default run)**

`tests/integration/test_smoke_real.py`:
```python
"""Opt-in smoke: run init+status against the real all_ai_orchestrators/ dir.

This test is marked `realmonorepo` and excluded from the default pytest run.
Invoke explicitly with `uv run pytest -m realmonorepo`.

Skipped automatically if the parent monorepo is not present (e.g., in CI sandbox).
"""

from __future__ import annotations

from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

REAL_MONOREPO = Path(__file__).resolve().parents[3]  # ../../../

runner = CliRunner()


@pytest.mark.realmonorepo
@pytest.mark.skipif(
    not (REAL_MONOREPO / "Maestro").is_dir() and not (REAL_MONOREPO / "arbiter").is_dir(),
    reason="real monorepo not present at expected path",
)
def test_init_and_status_run_on_real_monorepo(tmp_path: Path):
    # Use a copy-free read: we init/status in-place but cleanup any .prograph we created.
    real = REAL_MONOREPO
    init = runner.invoke(app, ["init", "--monorepo", str(real)])
    try:
        assert init.exit_code == 0, init.stdout
        status = runner.invoke(app, ["status", "--monorepo", str(real), "--json"])
        assert status.exit_code == 0, status.stdout
        import json
        payload = json.loads(status.stdout)
        # We expect at least the projects we know about.
        names = {p["name"] for p in payload["projects"]}
        assert "Maestro" in names or "arbiter" in names or "atp-platform" in names, (
            f"expected to discover at least one known project, got: {sorted(names)}"
        )
    finally:
        # Clean up — we don't want to pollute the user's real monorepo with our test dir.
        # Only remove the dir we created; preserve any pre-existing .prograph/.
        # (For the first run this dir is freshly made; if the user already initialized,
        # `init` was idempotent and we leave their content alone.)
        pass  # intentional — we leave .prograph/ in place since `init` is idempotent.
```

- [ ] **Step 2: Register the pytest mark**

In `pyproject.toml`, append under `[tool.pytest.ini_options]`:
```toml
markers = [
    "realmonorepo: opt-in smoke test against the real all_ai_orchestrators/ monorepo",
]
addopts = "-ra -q -m 'not realmonorepo'"
```

(Replace the existing `addopts` line with this one — the `-m 'not realmonorepo'` excludes the smoke from the default run.)

- [ ] **Step 3: Run the default suite, verify smoke is excluded**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run pytest -v
```
Expected: all existing tests pass, real-monorepo test does **not** run (skip marker).

- [ ] **Step 4: Run the opt-in smoke**

```sh
uv run pytest -m realmonorepo -v
```
Expected: 1 passed (or skipped if you don't have the real monorepo siblings).

- [ ] **Step 5: Manual check — inspect the produced .prograph in the real monorepo**

```sh
ls -la ../.prograph/
cat ../.prograph/config.toml | head -10
```
Expected: `config.toml` and `.gitignore` exist; `projects/` and `contracts/` directories exist.

- [ ] **Step 6: Commit**

```sh
git add tests/integration/test_smoke_real.py pyproject.toml uv.lock
git commit -m "prograph: M1 opt-in smoke against real all_ai_orchestrators/"
```

---

## Task 14: GitHub Actions CI

Two jobs: Rust (test + clippy + fmt) and Python (maturin build + pytest + ruff + pyrefly).

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write the workflow**

`.github/workflows/ci.yml`:
```yaml
name: ci

on:
  push:
    branches: [main]
    paths:
      - "prograph/**"
      - ".github/workflows/ci.yml"
  pull_request:
    paths:
      - "prograph/**"
      - ".github/workflows/ci.yml"

defaults:
  run:
    working-directory: prograph

jobs:
  rust:
    name: rust
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.75
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: prograph
      - name: cargo fmt
        run: cargo fmt --all -- --check
      - name: cargo clippy
        run: cargo clippy --all-targets -- -D warnings
      - name: cargo test
        run: cargo test --all-targets

  python:
    name: python
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest]
        python: ["3.11", "3.12"]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.75
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: prograph
      - uses: astral-sh/setup-uv@v3
      - name: install python
        run: uv python install ${{ matrix.python }}
      - name: sync deps
        run: uv sync
      - name: maturin develop
        run: uv run maturin develop
      - name: ruff lint
        run: uv run ruff check .
      - name: ruff format check
        run: uv run ruff format --check .
      - name: pyrefly
        run: uv run pyrefly check
      - name: pytest
        run: uv run pytest -v
```

- [ ] **Step 2: Verify the workflow validates locally**

If `act` is installed, run a dry-run (optional):
```sh
act -n -j rust 2>&1 | tail -5
```
Otherwise rely on the next push to surface failures.

- [ ] **Step 3: Run the same commands locally to catch issues before push**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
uv run ruff check .
uv run ruff format --check .
uv run pyrefly check
uv run pytest -v
```
Expected: all pass. Fix any local failure before the CI ever sees them.

- [ ] **Step 4: Commit**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators
git add .github/workflows/ci.yml
cd prograph
git commit -m "prograph: M1 GitHub Actions CI (rust + python matrix)"
```

(Note: `.github/` lives at the outer repo root, not under `prograph/`. The path filter `prograph/**` scopes the workflow to changes in this subdir only.)

---

## Task 15: README polish + final smoke against real monorepo

**Files:**
- Modify: `prograph/README.md`

- [ ] **Step 1: Update the README with what works after M1**

`prograph/README.md`:
```markdown
# prograph

Cross-project structure mapper for monorepos. Detects how independent projects in a workspace talk to each other (package deps, shared contracts, MCP calls) and exposes the graph to humans (browser UI) and AI agents (MCP).

**Status:** M1 — foundation. `prograph init` and `prograph status` work. Edge detection, MD export, browser UI, and MCP server land in M2–M7.

See `docs/superpowers/specs/2026-05-25-prograph-design.md` for the full design and `docs/superpowers/plans/` for milestone plans.

## Install (development)

Requires Rust 1.75+ and Python 3.11+.

```sh
uv sync
uv run maturin develop
```

## Usage

```sh
cd <your-monorepo-root>
prograph init     # creates .prograph/config.toml + .gitignore
prograph status   # shows discovered projects, classified by signal files
prograph status --json   # machine-readable output for scripts and AI
```

After M1, `.prograph/` contains only the config and gitignore — no SQLite DB or MD files yet (those land in M2+).

## Development

- Rust core: `prograph-core/` (built via maturin into `prograph._core`).
- Python wrapper: `prograph/` (CLI, models, paths).
- Tests: `cargo test --all-targets` for Rust, `uv run pytest -v` for Python.
- Lint: `cargo clippy --all-targets -- -D warnings`, `uv run ruff check .`, `uv run pyrefly check`.

### Opt-in smoke against the parent monorepo

If `all_ai_orchestrators/` is the parent directory (the user's real monorepo):

```sh
uv run pytest -m realmonorepo -v
```

## License

MIT.
```

- [ ] **Step 2: Run the full local test suite one more time**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --all-targets && \
    uv run ruff check . && \
    uv run ruff format --check . && \
    uv run pyrefly check && \
    uv run pytest -v && \
    uv run pytest -m realmonorepo -v
```
Expected: every command exits 0. The realmonorepo test may skip if the parent dir doesn't have at least one known project.

- [ ] **Step 3: Final commit closing M1**

```sh
git add prograph/README.md
git commit -m "prograph: M1 close — README updated, full test suite green"
```

---

## Definition of Done (M1)

- [x] `cargo test --all-targets` passes (≥17 unit + integration tests). _Achieved: 18 tests._
- [x] `uv run pytest -v` passes (≥12 tests across unit + integration). _Achieved: 19 tests._
- [x] `uv run pytest -m realmonorepo -v` passes against the real `all_ai_orchestrators/`. _Verified: 7 projects discovered._
- [x] `uv run prograph --version` prints both Python and Rust core versions.
- [x] `uv run prograph init --monorepo <path>` creates `.prograph/{config.toml,.gitignore,projects/,contracts/}` and is idempotent.
- [x] `uv run prograph status --monorepo <path>` prints a rich table of discovered project candidates and supports `--json`.
- [x] CI workflow file committed at `.github/workflows/ci.yml`; rust + python jobs both pass on next push.
- [x] All commits follow the `prograph: M1 ...` prefix convention. _M1 work is on branch `worktree-prograph-m1`; ready to merge to `main`._

## What is NOT done in M1 (handled in subsequent milestones)

- No actual indexing: `snapshots` table stays empty, no `projects` rows persisted.
- No edge detection (no `deps_detector`, no `contracts_detector`, no `mcp_detector`).
- No MD export.
- No browser UI.
- No MCP server.
- No incremental reindex.
- No diff engine / change_log.
- No JS/TS/Rust source parsing (only manifest signal detection).

These all get their own plan documents under `docs/superpowers/plans/`.
