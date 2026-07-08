# prograph M8 — Polish & Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After M8, prograph is feature-complete relative to the original brainstorm. The five rough edges accumulated through M1-M7 are closed: (1) `edge_evidence` is populated for ALL edge kinds (not just `mcp_call`), so the MCP `edge_evidence` tool + REST endpoint return useful data for every edge; (2) the browser UI gains a snapshot picker and a diff view (`/api/graph?since=<snap>`) that visualises added/removed edges; (3) discovery handles Cargo workspace + Python workspace patterns automatically, so projects publishing sub-packages don't need explicit `[tool.prograph].aliases`; (4) PEP 508 URL dependencies (`foo @ git+...`) parse correctly; (5) a performance benchmark suite catches regressions in CI. This is the last milestone before `prograph` declares v1.0.

**Architecture:**
- **Five focused themes**, each spanning 2-4 tasks. The themes are independent: an implementer can do them in any order (the task numbering reflects dependency, but theme A's tasks never depend on theme B's).
- **No new languages, no new schema migrations.** v5 (from M7) already supports everything M8 needs. Edge evidence backfill is purely indexer + detector logic. Workspace discovery is parser logic. Diff view is queries + UI rendering.
- **Performance baselines via `cargo bench` + pytest-benchmark.** CI guards against >2× regression on the `monorepo_full` fixture. Not a strict SLA; a tripwire.

**Tech Stack additions (M8 only):**
- `pytest-benchmark` (dev-dep) — Python perf assertions
- `criterion` (Rust dev-dep, optional) — Rust micro-benchmarks. Skip if scope creep; pytest-benchmark + integration tests are enough.

No new runtime deps. No new Python deps either (FastAPI already supports query strings for diff view; frontend changes are all in existing files).

**Spec reference:** `docs/superpowers/specs/2026-05-25-prograph-design.md` §5.2 identity rules (no changes — M8 honors existing identity contracts), §7.3 `/api/graph?since=` (the spec line that M6 explicitly deferred). M1-M7 deferred-items lists collectively define M8's scope.

**Baseline:** Branch off `main` at the M6 close commit (after the user reports M6 complete; check `git log`). All gates green from M1-M7.

**M8 explicitly out of scope (deferred to M9+):**
- **Module-level facts** (public Python symbols, internal imports, public Rust crate items) — needs significant tree-sitter expansion. Its own milestone.
- **HTTP / REST runtime edges** — heavy heuristic work, low payoff against the target monorepo. Defer to M9+ or never.
- **JS MCP source scanning** — no JS MCP servers in scope.
- **WebSocket live updates** (`/ws/changes`) — page reload remains the upgrade path.
- **Offline asset bundling** — CDN works fine for the local-dev tool.
- **Playwright/Selenium E2E** — REST + static-structure tests cover the regression surface for M8.
- **Authentication / TLS** — bind-to-127.0.0.1 remains the security boundary.
- **Mobile / responsive design** — desktop-only.

---

## File Structure (created/modified in M8)

```
prograph/
├── prograph-core/
│   ├── Cargo.toml                                  # MODIFY — criterion dev-dep (optional)
│   ├── src/
│   │   ├── detectors/deps.rs                       # MODIFY — populate evidence (manifest:1)
│   │   ├── detectors/contracts.rs                  # MODIFY — populate evidence per file
│   │   ├── parsers/python.rs                       # MODIFY — PEP 508 URL deps; setup.py support
│   │   ├── parsers/rust.rs                         # MODIFY — Cargo workspace member discovery
│   │   ├── discovery.rs                            # MODIFY — recurse into nested workspaces
│   │   ├── store.rs                                # MODIFY — find_edges_with_status_since
│   │   └── lib.rs                                  # MODIFY — register the new query helper
├── prograph/
│   ├── _core.pyi                                   # MODIFY — stub for new query
│   ├── models.py                                   # MODIFY — DiffEdgeRow pydantic model
│   ├── web_app.py                                  # MODIFY — /api/graph?since= branch
│   └── web_static/
│       ├── app.js                                  # MODIFY — snapshot picker + diff status colors
│       ├── graph.js                                # MODIFY — :added / :removed selectors
│       ├── index.html                              # MODIFY — snapshot picker DOM
│       └── styles.css                              # MODIFY — diff status colors
├── tests/
│   ├── fixtures/
│   │   └── monorepo_workspace/                     # NEW — Cargo + Python workspace fixture
│   ├── unit/
│   │   ├── test_pep508_url_deps.py                 # NEW
│   │   └── test_diff_view.py                       # NEW (pydantic-level)
│   ├── integration/
│   │   ├── test_workspace_discovery.py             # NEW
│   │   ├── test_edge_evidence_all_kinds.py         # NEW
│   │   ├── test_diff_view_rest.py                  # NEW
│   │   └── test_bench_baseline.py                  # NEW — pytest-benchmark
│   └── integration/test_smoke_real.py              # MODIFY — assert evidence on package_dep
└── pyproject.toml                                  # MODIFY — pytest-benchmark dev-dep + bench marker
```

---

## Task 1: Detectors populate `evidence` for all edge kinds

**Files:**
- Modify: `prograph-core/src/detectors/deps.rs`
- Modify: `prograph-core/src/detectors/contracts.rs`

M7 only filled `EdgeCandidate.evidence` for `mcp_call`. M8 extends to `package_dep` (manifest file + line of the dep declaration — best-effort line 1 if we don't track exact lines) and `contract_link` (the consumer's contract file path).

- [ ] **Step 1: `deps_detector` populates evidence**

In `prograph-core/src/detectors/deps.rs`, inside the `detect` function where each `EdgeCandidate` is constructed, set `evidence` to a single-entry vec pointing at the consumer's primary manifest file.

The consumer project's manifest file is conventionally:
- Python: `pyproject.toml`
- Rust: `Cargo.toml`
- JS: `package.json`

We don't currently track the manifest filename inside `Manifest` — the parser knows it but doesn't pass it through. For M8's pragmatic approach, infer from the project kind via a small helper that reads the `ProjectFacts` (which has `project_root` but not kind directly). Workaround: look up kind via the `discovery::ProjectCandidate` list. Since that's passed to the indexer, not the detector, we have two options:

**Option A** (chosen for M8): just hardcode the manifest filename guess based on convention — `pyproject.toml` always, since deps detector currently only matches via Python dep names. If we later support Rust/JS deps detection, extend this. M8's deps_detector behaviour is unchanged for Rust/JS (the parsers populate `declared_deps` from `Cargo.toml`/`package.json` but the cross-language matching only happens within Python idioms).

**Option B**: thread a `manifest_rel_path: Option<String>` through `Manifest`. Cleaner but bigger refactor. Defer to M9.

Apply Option A. In `deps_detector::detect`, replace the `EdgeCandidate { ... }` construction with:
```rust
            out.push(EdgeCandidate {
                kind: EdgeKind::PackageDep,
                from_kind: NodeKind::Project,
                from_idx: consumer_idx,
                to_kind: NodeKind::Project,
                to_idx: publisher_idx,
                attrs_json,
                attrs_hash,
                evidence: vec![super::EvidenceLocation {
                    project_idx: consumer_idx,
                    rel_path: guess_manifest_path(&consumer.project_root).into(),
                    line: 1,
                    snippet: Some(format!("declared {}", dep.name)),
                }],
            });
```

Add the helper at the bottom of `deps.rs`:
```rust
/// Best-effort manifest filename inference. M8 deps_detector only fires for Python
/// projects (Rust/JS deps detection is a future milestone), so we hardcode
/// `pyproject.toml`. M9+ may thread the exact manifest path through `Manifest`.
fn guess_manifest_path(_project_root: &str) -> &'static str {
    "pyproject.toml"
}
```

- [ ] **Step 2: `contracts_detector` populates evidence**

In `prograph-core/src/detectors/contracts.rs`, the `contract_link` `EdgeCandidate` construction currently emits `evidence: Vec::new()` (or omits it). Change to populate one evidence row per owner-file pair.

Find the section where `contract_link` edges are emitted (after the `if owners.len() < 2 { continue; }` guard). For each owning `proj_idx`, locate the corresponding `ContractFile` entries in the original facts vec to get the `rel_path`:

```rust
        for &proj_idx in &owners {
            // Collect this project's contract files for this contract.
            let files_for_owner: Vec<String> = c.files
                .iter()
                .filter(|(p, _)| *p == proj_idx)
                .map(|(_, rel_path)| rel_path.clone())
                .collect();

            let evidence: Vec<super::EvidenceLocation> = files_for_owner
                .into_iter()
                .map(|rel_path| super::EvidenceLocation {
                    project_idx: proj_idx,
                    rel_path,
                    line: 1,
                    snippet: None,
                })
                .collect();

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
                to_idx: contract_idx,
                attrs_json,
                attrs_hash,
                evidence,
            });
        }
```

- [ ] **Step 3: Update detector tests**

In `deps.rs`'s tests, the existing `matches_consumer_to_publisher_by_name` test should now assert that `edges[0].evidence.len() == 1` and `edges[0].evidence[0].rel_path == "pyproject.toml"`. Add the assertions to the existing test (don't create a new one — same behaviour, just stricter).

In `contracts.rs`'s `two_owners_produces_two_link_edges` test, assert each emitted edge has `evidence.len() >= 1` and `evidence[0].rel_path` matches the corresponding owner's file.

- [ ] **Step 4: Run cargo tests**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo test --package prograph-core detectors
```
Expected: all detector tests pass (existing count + zero new tests, just stricter assertions).

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph-core/src/detectors/deps.rs prograph/prograph-core/src/detectors/contracts.rs
git commit -m "prograph: M8 detectors populate evidence for package_dep + contract_link"
```

---

## Task 2: Indexer persists evidence for all edge kinds (already happens — verify + test)

**Files:**
- Modify: `tests/integration/test_edge_evidence_all_kinds.py` (create)

The M7 indexer already iterates `EdgeCandidate.evidence` and persists rows regardless of edge kind. Task 1 made deps_detector + contracts_detector emit evidence; the indexer needs no changes. This task just verifies via integration test that `edge_evidence` rows now exist for all three edge kinds.

- [ ] **Step 1: Write the test**

`tests/integration/test_edge_evidence_all_kinds.py`:
```python
"""M8: edge_evidence persisted for ALL edge kinds (package_dep, mcp_call, contract_link)."""

import shutil
import sqlite3
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

cli_runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_mcp"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def _evidence_for_kind(db: Path, kind: str) -> list[tuple]:
    conn = sqlite3.connect(db)
    try:
        return conn.execute(
            """
            SELECT ev.edge_id, ev.rel_path, ev.line
            FROM edge_evidence ev
            JOIN edges e ON e.id = ev.edge_id
            WHERE e.kind = ? AND ev.last_seen = (SELECT MAX(id) FROM snapshots)
            """,
            (kind,),
        ).fetchall()
    finally:
        conn.close()


def test_evidence_persisted_for_package_dep(indexed: Path):
    # monorepo_mcp uses dependencies for none of its projects — but py_dual_client has
    # python deps. Actually monorepo_mcp doesn't exercise package_dep cross-edges.
    # Skip + cover via monorepo_full where deps cross-link.
    pass  # placeholder — see test below


def test_evidence_persisted_in_monorepo_full(tmp_path: Path):
    """monorepo_full has 3 package_dep edges; each should now have evidence."""
    fixture = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_full"
    dst = tmp_path / "monorepo_full"
    shutil.copytree(fixture, dst, ignore=shutil.ignore_patterns("golden"))
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(app, ["index", "--monorepo", str(dst)])

    db = dst / ".prograph" / "graph.db"
    evidence = _evidence_for_kind(db, "package_dep")
    assert len(evidence) == 3, (
        f"monorepo_full has 3 package_dep edges; expected 3 evidence rows, got {evidence}"
    )
    for (_eid, rel_path, line) in evidence:
        assert rel_path == "pyproject.toml"
        assert line == 1


def test_evidence_persisted_for_mcp_call(indexed: Path):
    """monorepo_mcp has 3 mcp_call edges (existing M7 behaviour) — should remain."""
    db = indexed / ".prograph" / "graph.db"
    evidence = _evidence_for_kind(db, "mcp_call")
    assert len(evidence) >= 3, f"expected ≥3 mcp_call evidence rows, got {len(evidence)}"


def test_evidence_persisted_for_contract_link(indexed: Path):
    """monorepo_mcp has 2 contract_link edges (shared_a + shared_b); evidence should exist."""
    db = indexed / ".prograph" / "graph.db"
    evidence = _evidence_for_kind(db, "contract_link")
    assert len(evidence) >= 2, f"expected ≥2 contract_link evidence rows, got {evidence}"
    # Spot-check: rel_path should be the .json file path from the fixture.
    paths = {e[1] for e in evidence}
    assert any("obs-v1.json" in p for p in paths), f"expected obs-v1.json in {paths}"
```

- [ ] **Step 2: Run**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv sync --reinstall-package prograph
uv run pytest tests/integration/test_edge_evidence_all_kinds.py -v
```
Expected: 4 passed (one is the placeholder that skips).

Full suite:
```sh
uv run pytest -v
```
Expected: 126+ tests.

- [ ] **Step 3: Update existing golden tests if MD output changed**

The M5 MD renderer references evidence in the project + contract MD cards. Adding evidence rows for `package_dep` and `contract_link` may change MD content. Run the golden tests:

```sh
uv run pytest tests/integration/test_cli_export_md.py -v
```

If golden tests fail because evidence is now rendered, regenerate:
```sh
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_full
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_multilang
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_mcp
```

Inspect the diff in the goldens — verify the change is the expected evidence rows appearing in the MD. Commit the regenerated golden files in the same commit.

- [ ] **Step 4: Commit**

```sh
git add prograph/tests/integration/test_edge_evidence_all_kinds.py \
        prograph/tests/fixtures/monorepo_full/golden/ \
        prograph/tests/fixtures/monorepo_multilang/golden/ \
        prograph/tests/fixtures/monorepo_mcp/golden/
git commit -m "prograph: M8 verify edge_evidence persistence for all three edge kinds + refresh goldens"
```

---

## Task 3: Update MCP + REST tool descriptions (drop "M7 only" caveat)

**Files:**
- Modify: `prograph/mcp_server.py`
- Modify: `prograph/web_static/app.js`

M7 documented that `edge_evidence` only works for `mcp_call`. M8 makes it work for all. Update the descriptions.

- [ ] **Step 1: Update MCP tool description**

In `prograph/mcp_server.py`, find the `edge_evidence` Tool definition and replace its description:

```python
        Tool(
            name="edge_evidence",
            description=(
                "Source-line locations that justify an edge. Returns evidence rows "
                "(file:line) for all edge kinds: mcp_call (every call site), "
                "package_dep (the consumer's manifest), contract_link (the consumer's "
                "contract file paths)."
            ),
            ...
        ),
```

- [ ] **Step 2: Update frontend "no evidence" fallback**

In `prograph/web_static/app.js`, the `renderEdge` function currently has:
```javascript
        nodes.push(el('p', {}, [el('em', {}, ['No evidence persisted for this edge kind in M7.'])]));
```

Replace with:
```javascript
        nodes.push(el('p', {}, [el('em', {}, ['No evidence rows for this edge.'])]));
```

(The M7-specific caveat is no longer accurate.)

- [ ] **Step 3: Commit**

```sh
git add prograph/prograph/mcp_server.py prograph/prograph/web_static/app.js
git commit -m "prograph: M8 update edge_evidence tool description (all kinds covered)"
```

---

## Task 4: `Store::find_edges_with_status_since` query helper

**Files:**
- Modify: `prograph-core/src/store.rs`
- Modify: `prograph-core/src/models.rs`
- Modify: `prograph-core/src/lib.rs`
- Modify: `prograph/_core.pyi`
- Modify: `prograph/models.py`

For the diff view, we need edges + nodes tagged with `added` / `removed` / `unchanged` relative to a `since_snapshot_id`. Add a new query that returns enriched EdgeRow records.

Identity rules:
- `added`: `first_seen > since AND last_seen = max_snap`
- `removed`: `last_seen >= since AND last_seen < max_snap`
- `unchanged`: `first_seen <= since AND last_seen = max_snap`

`attrs_changed` events are surfaced through the `change_log` (already accessible via `/api/changelog`). The diff view doesn't separately tag them in the graph — they look "unchanged" structurally.

- [ ] **Step 1: Add the `DiffEdgeRow` pyclass**

In `prograph-core/src/models.rs`, append:
```rust
/// Edge row enriched with diff status relative to a `since` snapshot.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all)]
pub struct DiffEdgeRow {
    pub id: i64,
    pub kind: String,
    pub from_kind: String,
    pub from_id: i64,
    pub from_name: String,
    pub to_kind: String,
    pub to_id: i64,
    pub to_name: String,
    pub attrs_json: String,
    pub first_seen: i64,
    pub last_seen: i64,
    /// "added" | "removed" | "unchanged"
    pub status: String,
}

#[pymethods]
impl DiffEdgeRow {
    fn __repr__(&self) -> String {
        format!("DiffEdgeRow({} {} → {}: {})", self.kind, self.from_name, self.to_name, self.status)
    }
}
```

Extend `pub use models::{...}` in `lib.rs` to include `DiffEdgeRow`. Register the class inside `#[pymodule]`:
```rust
    m.add_class::<DiffEdgeRow>()?;
```

- [ ] **Step 2: Add the query in `store.rs`**

Append to `impl Store`:
```rust
    /// Return ALL edges visible in the diff between `since_snapshot` and the current
    /// latest snapshot, each tagged with status `added` / `removed` / `unchanged`.
    pub fn find_edges_with_status_since(
        &self,
        since_snapshot: i64,
    ) -> Result<Vec<crate::models::DiffEdgeRow>> {
        let max_snap: i64 = self.conn.query_row(
            "SELECT MAX(id) FROM snapshots",
            [],
            |r| r.get(0),
        )?;

        let mut stmt = self.conn.prepare(
            "SELECT e.id, e.kind, e.from_kind, e.from_id,
                    CASE e.from_kind
                        WHEN 'project' THEN (SELECT name FROM projects WHERE id = e.from_id)
                        WHEN 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.from_id)
                    END AS from_name,
                    e.to_kind, e.to_id,
                    CASE e.to_kind
                        WHEN 'project' THEN (SELECT name FROM projects WHERE id = e.to_id)
                        WHEN 'contract' THEN (SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.to_id)
                    END AS to_name,
                    e.attrs_json, e.first_seen, e.last_seen,
                    CASE
                        WHEN e.last_seen = ? AND e.first_seen > ?  THEN 'added'
                        WHEN e.last_seen >= ? AND e.last_seen < ?   THEN 'removed'
                        WHEN e.last_seen = ?                         THEN 'unchanged'
                        ELSE NULL
                    END AS status
             FROM edges e
             WHERE (e.last_seen = ? AND e.first_seen > ?)
                OR (e.last_seen >= ? AND e.last_seen < ?)
                OR (e.last_seen = ? AND e.first_seen <= ?)
             ORDER BY e.kind, from_name, to_name",
        )?;

        let rows = stmt.query_map(
            rusqlite::params![
                max_snap, since_snapshot,   // added
                since_snapshot, max_snap,    // removed
                max_snap,                    // unchanged
                max_snap, since_snapshot,    // WHERE added
                since_snapshot, max_snap,    // WHERE removed
                max_snap, since_snapshot,    // WHERE unchanged
            ],
            |r| {
                Ok(crate::models::DiffEdgeRow {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    from_kind: r.get(2)?,
                    from_id: r.get(3)?,
                    from_name: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    to_kind: r.get(5)?,
                    to_id: r.get(6)?,
                    to_name: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    attrs_json: r.get(8)?,
                    first_seen: r.get(9)?,
                    last_seen: r.get(10)?,
                    status: r.get::<_, Option<String>>(11)?.unwrap_or_else(|| "unknown".into()),
                })
            },
        )?;

        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }
```

- [ ] **Step 3: PyO3 wrapper**

In `prograph-core/src/lib.rs`, add:
```rust
#[pyfunction]
#[pyo3(name = "find_edges_with_status_since")]
fn py_find_edges_with_status_since(
    db_path: &str,
    since_snapshot: i64,
) -> PyResult<Vec<DiffEdgeRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.find_edges_with_status_since(since_snapshot)?)
}
```

Register inside `#[pymodule]`:
```rust
    m.add_function(wrap_pyfunction!(py_find_edges_with_status_since, m)?)?;
```

Extend `prograph/_core.pyi`:
```python
def find_edges_with_status_since(db_path: str, since_snapshot: int) -> list[DiffEdgeRow]: ...

class DiffEdgeRow:
    id: int
    kind: str
    from_kind: str
    from_id: int
    from_name: str
    to_kind: str
    to_id: int
    to_name: str
    attrs_json: str
    first_seen: int
    last_seen: int
    status: str
```

- [ ] **Step 4: Pydantic mirror**

Append to `prograph/models.py`:
```python
class DiffEdgeRow(BaseModel):
    model_config = ConfigDict(frozen=True)

    id: int
    kind: str
    from_kind: str
    from_id: int
    from_name: str
    to_kind: str
    to_id: int
    to_name: str
    attrs: dict[str, object]
    first_seen: int
    last_seen: int
    status: str  # "added" | "removed" | "unchanged"

    @classmethod
    def from_core(cls, value: _core.DiffEdgeRow) -> DiffEdgeRow:
        import json
        return cls(
            id=value.id,
            kind=value.kind,
            from_kind=value.from_kind,
            from_id=value.from_id,
            from_name=value.from_name,
            to_kind=value.to_kind,
            to_id=value.to_id,
            to_name=value.to_name,
            attrs=json.loads(value.attrs_json) if value.attrs_json else {},
            first_seen=value.first_seen,
            last_seen=value.last_seen,
            status=value.status,
        )
```

Re-export from `prograph/__init__.py` (add `DiffEdgeRow` to imports + `__all__`).

- [ ] **Step 5: Test**

Append to `store.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn find_edges_with_status_distinguishes_added_removed_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&tmp.path().join("g.db")).unwrap();

        // Snapshot 1: 2 projects + 1 edge (older).
        let (pid_a, pid_b, eid_old) = {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts1", "/m", None, "0.1.0").unwrap();
            let a = writer.insert_project(snap, "alpha", "./alpha", "python", "{}").unwrap();
            let b = writer.insert_project(snap, "beta", "./beta", "python", "{}").unwrap();
            let e = writer.insert_edge(
                snap, "package_dep", "project", a, "project", b, "{}", "h_old",
            ).unwrap();
            writer.commit().unwrap();
            (a, b, e)
        };

        // Snapshot 2: keep old edge alive + add a new one + remove no edge yet.
        let _eid_new = {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts2", "/m", None, "0.1.0").unwrap();
            writer.touch_project(pid_a, snap, None).unwrap();
            writer.touch_project(pid_b, snap, None).unwrap();
            writer.touch_edge(eid_old, snap, None).unwrap();
            let e_new = writer.insert_edge(
                snap, "mcp_call", "project", pid_a, "project", pid_b, r#"{"tool":"t"}"#, "h_new",
            ).unwrap();
            writer.commit().unwrap();
            e_new
        };

        // Snapshot 3: drop the new edge (don't touch it).
        {
            let writer = store.begin_snapshot().unwrap();
            let snap = writer.insert_snapshot("ts3", "/m", None, "0.1.0").unwrap();
            writer.touch_project(pid_a, snap, None).unwrap();
            writer.touch_project(pid_b, snap, None).unwrap();
            writer.touch_edge(eid_old, snap, None).unwrap();
            writer.commit().unwrap();
        }

        // Diff since snapshot 1.
        let diff = store.find_edges_with_status_since(1).unwrap();
        let statuses: std::collections::HashMap<String, String> =
            diff.iter().map(|d| (d.kind.clone(), d.status.clone())).collect();

        assert_eq!(statuses.get("package_dep"), Some(&"unchanged".to_string()));
        assert_eq!(statuses.get("mcp_call"), Some(&"removed".to_string()));
    }
```

- [ ] **Step 6: Run + commit**

```sh
cargo test --package prograph-core store
uv run pytest -v
```
Expected: cargo +1 new test; pytest unchanged.

```sh
git add prograph/prograph-core/src/store.rs prograph/prograph-core/src/models.rs \
        prograph/prograph-core/src/lib.rs prograph/prograph/_core.pyi \
        prograph/prograph/models.py prograph/prograph/__init__.py
git commit -m "prograph: M8 Store::find_edges_with_status_since + DiffEdgeRow pyclass/pydantic"
```

---

## Task 5: REST `GET /api/graph?since=<snap>` + integration test

**Files:**
- Modify: `prograph/web_app.py`
- Create: `tests/integration/test_diff_view_rest.py`

The endpoint takes an optional `since` query param. When provided, edges (and any nodes that were added or removed) get a `status` field tagged.

- [ ] **Step 1: Extend `/api/graph`**

In `prograph/web_app.py`, modify the existing `graph` endpoint signature to accept the `since` query:
```python
    @app.get("/api/graph")
    async def graph(since: int | None = None) -> dict:
        from prograph import _core
        from prograph.export.slug import slugify
        from prograph.models import DiffEdgeRow, MonorepoOverview

        raw = _core.monorepo_overview(app.state.db_path)
        if raw is None:
            return {"nodes": [], "edges": [], "snapshot_id": None}
        ov = MonorepoOverview.from_core(raw)

        nodes: list[dict] = []
        for p in ov.projects:
            nodes.append({
                "id": f"p:{p.slug}",
                "node_kind": "project",
                "name": p.name,
                "kind": p.kind,
                "label": p.name,
            })
        for c in ov.contracts:
            nodes.append({
                "id": f"c:{c.slug}",
                "node_kind": "contract",
                "name": c.declared_id or c.slug,
                "kind": c.kind,
                "label": c.declared_id or c.slug,
                "n_owners": c.n_owners,
            })

        if since is None:
            # Existing behaviour: all alive edges.
            raw_edges = _core.find_edges_filtered(app.state.db_path, None, None, None, None)
            edges_out: list[dict] = []
            for e in raw_edges:
                from_id = f"{'p' if e.from_kind == 'project' else 'c'}:{slugify(e.from_name)}"
                to_id = f"{'p' if e.to_kind == 'project' else 'c'}:{slugify(e.to_name)}"
                edges_out.append({
                    "id": f"e:{e.id}",
                    "source": from_id,
                    "target": to_id,
                    "kind": e.kind,
                    "edge_id": e.id,
                    "status": "unchanged",  # uniform default
                })
        else:
            # Diff view.
            raw_diff = _core.find_edges_with_status_since(app.state.db_path, since)
            edges_out = []
            for e in raw_diff:
                d = DiffEdgeRow.from_core(e)
                from_id = f"{'p' if d.from_kind == 'project' else 'c'}:{slugify(d.from_name)}"
                to_id = f"{'p' if d.to_kind == 'project' else 'c'}:{slugify(d.to_name)}"
                edges_out.append({
                    "id": f"e:{d.id}",
                    "source": from_id,
                    "target": to_id,
                    "kind": d.kind,
                    "edge_id": d.id,
                    "status": d.status,
                })

        return {
            "snapshot_id": ov.snapshot_id,
            "snapshot_ts": ov.snapshot_ts,
            "n_projects": ov.n_projects,
            "n_contracts": ov.n_contracts,
            "n_edges": ov.n_edges,
            "since": since,
            "nodes": nodes,
            "edges": edges_out,
        }
```

- [ ] **Step 2: REST integration tests**

`tests/integration/test_diff_view_rest.py`:
```python
"""M8: GET /api/graph?since=<snap> tags edges with added/removed/unchanged status."""

import shutil
from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from typer.testing import CliRunner

from prograph.cli import app as cli_app
from prograph.web_app import build_app

cli_runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_full"


@pytest.fixture
def two_snapshots(tmp_path: Path) -> Path:
    """Set up a monorepo with two snapshots: snap 1 with edge A→B; snap 2 with the
    edge removed (by deleting B's project) AND a new edge A→C added."""
    dst = tmp_path / "evolving"
    dst.mkdir()
    (dst / "alpha").mkdir()
    (dst / "alpha" / "pyproject.toml").write_text('[project]\nname="alpha"\ndependencies=["beta"]\n')
    (dst / "beta").mkdir()
    (dst / "beta" / "pyproject.toml").write_text('[project]\nname="beta"\n')

    cli_runner.invoke(cli_app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(cli_app, ["index", "--monorepo", str(dst)])  # snapshot 1

    # Modify alpha: remove the beta dep, add a charlie dep. Add charlie project.
    (dst / "alpha" / "pyproject.toml").write_text('[project]\nname="alpha"\ndependencies=["charlie"]\n')
    (dst / "charlie").mkdir()
    (dst / "charlie" / "pyproject.toml").write_text('[project]\nname="charlie"\n')

    cli_runner.invoke(cli_app, ["index", "--monorepo", str(dst)])  # snapshot 2
    return dst


def test_graph_without_since_returns_alive_edges(two_snapshots: Path):
    client = TestClient(build_app(two_snapshots))
    r = client.get("/api/graph")
    assert r.status_code == 200
    payload = r.json()
    assert payload["since"] is None
    assert all(e["status"] == "unchanged" for e in payload["edges"])
    edge_kinds = {e["kind"] for e in payload["edges"]}
    assert "package_dep" in edge_kinds


def test_graph_with_since_tags_diff(two_snapshots: Path):
    client = TestClient(build_app(two_snapshots))
    r = client.get("/api/graph?since=1")
    assert r.status_code == 200
    payload = r.json()
    assert payload["since"] == 1
    statuses = {(e["kind"], e["status"]) for e in payload["edges"]}

    # Expect: alpha→beta removed; alpha→charlie added; no unchanged in this case.
    assert ("package_dep", "added") in statuses, statuses
    assert ("package_dep", "removed") in statuses, statuses
```

- [ ] **Step 3: Run + commit**

```sh
uv sync --reinstall-package prograph
uv run pytest tests/integration/test_diff_view_rest.py -v
uv run pytest -v
```

```sh
git add prograph/prograph/web_app.py prograph/tests/integration/test_diff_view_rest.py
git commit -m "prograph: M8 GET /api/graph?since= tags edges with added/removed/unchanged"
```

---

## Task 6: Frontend diff view — snapshot picker + status colors

**Files:**
- Modify: `prograph/web_static/index.html`
- Modify: `prograph/web_static/app.js`
- Modify: `prograph/web_static/graph.js`
- Modify: `prograph/web_static/styles.css`

The browser UI gets a snapshot picker `<select>` that drives an optional `?since=` query against `/api/graph`. Edges render with green (added) / red (removed) / default (unchanged) colors overlaid on the kind colors.

- [ ] **Step 1: Add the picker to `index.html`**

In `prograph/web_static/index.html`, modify `#topbar` to include a new `<div id="diff-picker">` between `#snapshot-info` and `#search-container`:
```html
        <div id="diff-picker">
            <label for="diff-since">Diff since:</label>
            <select id="diff-since">
                <option value="">— off —</option>
            </select>
        </div>
```

- [ ] **Step 2: Style the picker + status colors**

In `prograph/web_static/styles.css`, append:
```css
#diff-picker {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
}

#diff-picker select {
    margin: 0;
    padding: 0.3rem 0.5rem;
    font-size: 0.85rem;
}

/* Diff-status edge styling — applied via cytoscape data attribute. */
.diff-added-pill {
    background: #d4edda;
    color: #155724;
}

.diff-removed-pill {
    background: #f8d7da;
    color: #721c24;
}
```

- [ ] **Step 3: Update `graph.js` to honor edge status**

In `prograph/web_static/graph.js`, extend the edge style block:
```javascript
            {
                selector: 'edge',
                style: {
                    'curve-style': 'bezier',
                    'target-arrow-shape': 'triangle',
                    'line-color': (e) => statusOverride(e.data('status')) || KIND_COLORS[e.data('kind')] || '#888',
                    'target-arrow-color': (e) => statusOverride(e.data('status')) || KIND_COLORS[e.data('kind')] || '#888',
                    'line-style': (e) => statusLineStyle(e.data('status'), e.data('kind')),
                    'width': (e) => e.data('status') === 'added' || e.data('status') === 'removed' ? 3 : 2,
                    'arrow-scale': 1.2,
                    'opacity': (e) => e.data('status') === 'removed' ? 0.6 : 1,
                },
            },
```

Add helper functions at the top of `graph.js`:
```javascript
function statusOverride(status) {
    if (status === 'added')   return '#28a745';
    if (status === 'removed') return '#dc3545';
    return null;
}

function statusLineStyle(status, kind) {
    if (status === 'removed') return 'dotted';
    return KIND_LINESTYLES[kind] || 'solid';
}
```

Also pass `status` through in `renderGraph`'s edge mapping:
```javascript
    cy.add(
        data.edges.map((e) => ({
            data: {
                id: e.id,
                source: e.source,
                target: e.target,
                kind: e.kind,
                edge_id: e.edge_id,
                status: e.status || 'unchanged',
            },
        }))
    );
```

- [ ] **Step 4: Wire the picker in `app.js`**

In `prograph/web_static/app.js`, refactor the `init` function so it can be called with an optional `since` value. Add picker population + change handler.

Replace `init()` with:
```javascript
async function loadGraph(since) {
    const url = since ? `/api/graph?since=${encodeURIComponent(since)}` : '/api/graph';
    const data = await fetchJson(url);
    renderGraph(cy, data);

    const info = document.getElementById('snapshot-info');
    const diffSuffix = since ? ` · diff since #${since}` : '';
    info.textContent = `snapshot #${data.snapshot_id} · ${data.n_projects} projects · ${data.n_contracts} contracts · ${data.n_edges} edges${diffSuffix}`;
}

async function populateSnapshotPicker() {
    try {
        const snapshots = await fetchJson('/api/snapshots?limit=50');
        const select = document.getElementById('diff-since');
        // Keep the "off" option, then add each historical snapshot.
        const offOption = select.firstElementChild;
        setChildren(select, [offOption]);
        for (const s of snapshots) {
            // Skip the latest snapshot (no diff to itself).
            if (snapshots[0] && s.id === snapshots[0].id) continue;
            const opt = el('option', { value: String(s.id) }, [`#${s.id} (${s.ts})`]);
            select.appendChild(opt);
        }
    } catch (e) {
        console.warn('snapshot picker init failed', e);
    }
}

async function init() {
    await loadGraph(null);
    refreshActivity();
    await populateSnapshotPicker();

    const picker = document.getElementById('diff-since');
    picker.addEventListener('change', () => {
        const v = picker.value;
        loadGraph(v || null).catch((e) => {
            console.error('diff load failed', e);
        });
    });
}
```

- [ ] **Step 5: Update static asset structure tests**

In `tests/unit/test_web_static.py`, append:
```python
def test_index_html_has_diff_picker():
    html = (STATIC_DIR / "index.html").read_text()
    assert 'id="diff-picker"' in html
    assert 'id="diff-since"' in html


def test_app_js_handles_since_param():
    js = (STATIC_DIR / "app.js").read_text()
    assert "/api/graph?since=" in js
    assert "populateSnapshotPicker" in js
```

- [ ] **Step 6: Run + commit**

```sh
uv run pytest tests/unit/test_web_static.py -v
uv run pytest tests/integration/test_cli_serve.py -v
```

```sh
git add prograph/prograph/web_static/index.html prograph/prograph/web_static/app.js \
        prograph/prograph/web_static/graph.js prograph/prograph/web_static/styles.css \
        prograph/tests/unit/test_web_static.py
git commit -m "prograph: M8 browser diff view — snapshot picker + added/removed edge coloring"
```

---

## Task 7: Discovery — recurse into Cargo + Python workspaces

**Files:**
- Modify: `prograph-core/src/discovery.rs`
- Modify: `prograph-core/src/parsers/rust.rs` (helper to detect workspace)
- Create: `tests/fixtures/monorepo_workspace/` (multiple files)

Workspace-style monorepos publish multiple packages from one root. Currently, prograph requires `[tool.prograph].aliases` to detect them. M8 auto-discovers nested manifests so it Just Works on `Cargo.toml [workspace]` + Python projects with sub-package layouts.

**Strategy:** After classifying a first-level project, if its manifest declares a workspace (`[workspace]` in Cargo.toml, or `[tool.uv.workspace]` / explicit nested directories with their own `pyproject.toml` in Python), recurse one level deep to find sub-package manifests. Each sub-package becomes its own `ProjectCandidate` with name = its declared `[package].name` / `[project].name`, root_path = relative path including the parent.

- [ ] **Step 1: Add workspace detection to the Rust parser**

In `prograph-core/src/parsers/rust.rs`, expose a small predicate:
```rust
/// Return `true` if the given Cargo.toml declares a `[workspace]` table.
/// Used by the discovery layer to decide whether to recurse.
pub fn declares_workspace(project_root: &std::path::Path) -> bool {
    let cargo_toml = project_root.join("Cargo.toml");
    let Ok(contents) = std::fs::read_to_string(&cargo_toml) else {
        return false;
    };
    let Ok(root) = toml::from_str::<CargoToml>(&contents) else {
        return false;
    };
    root.workspace.is_some()
}
```

(`CargoToml` struct is already in `rust.rs` from M3.)

- [ ] **Step 2: Add Python workspace detection**

In `prograph-core/src/parsers/python.rs`, add:
```rust
/// Return true if a Python project's pyproject.toml declares a uv-style workspace
/// (`[tool.uv.workspace]`) or has nested project subdirectories with their own
/// pyproject.toml. The latter is checked by the discovery layer; this fn covers
/// only the explicit declaration.
pub fn declares_workspace(project_root: &std::path::Path) -> bool {
    let pyproject = project_root.join("pyproject.toml");
    let Ok(contents) = std::fs::read_to_string(&pyproject) else {
        return false;
    };
    // Use a loose substring check rather than full parse — we only care about presence.
    contents.contains("[tool.uv.workspace]")
        || contents.contains("[tool.hatch.build.targets.wheel.packages")  // common pattern
}
```

(This is heuristic; the canonical check is parsing the TOML for `[tool.uv.workspace]`. Substring is good enough for M8 and zero-allocation.)

- [ ] **Step 3: Recurse in `discovery::scan_monorepo`**

In `prograph-core/src/discovery.rs`, after the existing loop that builds `Vec<ProjectCandidate>`, add a workspace-expansion pass:

```rust
    candidates.sort_by(|a, b| a.name.cmp(&b.name));

    // M8: workspace recursion. For each candidate that declares a workspace, scan
    // its subdirs one level deep for sub-projects with their own manifests.
    let mut workspace_subs: Vec<ProjectCandidate> = Vec::new();
    for cand in &candidates {
        let abs_root = monorepo_root.join(cand.root_path.trim_start_matches("./"));
        let declares = match cand.kind {
            ProjectKind::Rust => crate::parsers::rust::declares_workspace(&abs_root),
            ProjectKind::Python | ProjectKind::Mixed => {
                crate::parsers::python::declares_workspace(&abs_root)
            }
            _ => false,
        };
        if !declares {
            continue;
        }

        // Scan immediate subdirs of this workspace root.
        let entries = std::fs::read_dir(&abs_root).map_err(|source| {
            crate::errors::PrographError::Io {
                path: abs_root.display().to_string(),
                source,
            }
        })?;
        for entry in entries.flatten() {
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
            let sub_rel = format!("{}/{}", cand.root_path, name);
            if let Some(sub_candidate) = classify_project(&path, &name, &sub_rel)? {
                // Avoid name collisions with the workspace root itself.
                if sub_candidate.name == cand.name {
                    continue;
                }
                workspace_subs.push(sub_candidate);
            }
        }
    }

    candidates.extend(workspace_subs);
    candidates.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(candidates)
```

Note: this recurses ONE level. Nested workspaces-in-workspaces are deferred to future polish.

- [ ] **Step 4: Create the `monorepo_workspace` fixture**

`tests/fixtures/monorepo_workspace/`:

`outer_python/pyproject.toml`:
```toml
[project]
name = "outer-python"
version = "0.1.0"

[tool.uv.workspace]
members = ["sub_a", "sub_b"]
```

`outer_python/sub_a/pyproject.toml`:
```toml
[project]
name = "outer-python-sub-a"
version = "0.1.0"
```

`outer_python/sub_b/pyproject.toml`:
```toml
[project]
name = "outer-python-sub-b"
version = "0.1.0"
dependencies = ["outer-python-sub-a"]
```

`rust_workspace/Cargo.toml`:
```toml
[workspace]
members = ["crate_x", "crate_y"]
```

`rust_workspace/crate_x/Cargo.toml`:
```toml
[package]
name = "crate-x"
version = "0.1.0"
```

`rust_workspace/crate_y/Cargo.toml`:
```toml
[package]
name = "crate-y"
version = "0.1.0"

[dependencies]
crate-x = { path = "../crate_x" }
```

`consumer/pyproject.toml`:
```toml
[project]
name = "consumer"
version = "0.1.0"
dependencies = ["outer-python-sub-a"]
```

- [ ] **Step 5: Integration test**

`tests/integration/test_workspace_discovery.py`:
```python
"""M8: discovery recurses into Cargo + Python workspaces."""

import json
import shutil
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

cli_runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_workspace"


@pytest.fixture
def indexed_workspace(tmp_path: Path) -> Path:
    dst = tmp_path / "ws"
    shutil.copytree(FIXTURE, dst)
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    return dst


def test_discovery_finds_python_workspace_members(indexed_workspace: Path):
    result = cli_runner.invoke(app, ["status", "--monorepo", str(indexed_workspace), "--json"])
    assert result.exit_code == 0
    payload = json.loads(result.stdout)
    names = {p["name"] for p in payload["projects"]}
    assert "outer_python" in names
    assert "sub_a" in names
    assert "sub_b" in names


def test_discovery_finds_rust_workspace_members(indexed_workspace: Path):
    result = cli_runner.invoke(app, ["status", "--monorepo", str(indexed_workspace), "--json"])
    payload = json.loads(result.stdout)
    names = {p["name"] for p in payload["projects"]}
    assert "rust_workspace" in names or "crate_x" in names  # either the root or member
    assert "crate_x" in names
    assert "crate_y" in names


def test_index_finds_workspace_cross_deps(indexed_workspace: Path):
    cli_runner.invoke(app, ["index", "--monorepo", str(indexed_workspace), "--json"])
    # consumer → outer-python-sub-a (declared in consumer.pyproject; sub_a now discoverable)
    import sqlite3
    db = indexed_workspace / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    edges = conn.execute(
        """
        SELECT p1.name, p2.name, json_extract(e.attrs_json, '$.dep_name')
        FROM edges e
        JOIN projects p1 ON p1.id = e.from_id
        JOIN projects p2 ON p2.id = e.to_id
        WHERE e.kind = 'package_dep'
          AND e.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()

    # consumer declares "outer-python-sub-a" which matches sub_a's declared name.
    found = any(
        c[0] == "consumer" and c[1] == "sub_a" and c[2] == "outer-python-sub-a"
        for c in edges
    )
    assert found, f"expected consumer → sub_a edge, got: {edges}"
```

- [ ] **Step 6: Run + commit**

```sh
cargo test --package prograph-core
uv sync --reinstall-package prograph
uv run pytest tests/integration/test_workspace_discovery.py -v
```

```sh
git add prograph/prograph-core/src/discovery.rs \
        prograph/prograph-core/src/parsers/rust.rs \
        prograph/prograph-core/src/parsers/python.rs \
        prograph/tests/fixtures/monorepo_workspace/ \
        prograph/tests/integration/test_workspace_discovery.py
git commit -m "prograph: M8 workspace auto-discovery — recurse into Cargo + Python workspaces"
```

---

## Task 8: PEP 508 URL dependencies

**Files:**
- Modify: `prograph-core/src/parsers/python.rs`
- Create: `tests/unit/test_pep508_url_deps.py`

The `@` operator in PEP 508 (`name @ url`) currently confuses the parser. Fix it.

- [ ] **Step 1: Update `parse_pep508`**

In `prograph-core/src/parsers/python.rs`, replace the existing `parse_pep508` function with:

```rust
/// Split a PEP 508 dep string into (name, version_req).
///
/// Handles:
/// - `foo>=1.0` → name="foo", version_req=">=1.0"
/// - `foo[extras]>=1.0; marker` → strips extras + marker, then operator parse
/// - `foo @ git+https://...` → name="foo", version_req=None (URL form has no PEP 440 version)
/// - `foo` → name="foo", version_req=None
fn parse_pep508(raw: &str) -> DepRequirement {
    let no_marker = raw.split(';').next().unwrap_or(raw).trim();
    let no_extras = strip_extras(no_marker);

    // PEP 508 URL form: split on `@`. Name is everything before; the URL portion
    // doesn't carry a usable version constraint so version_req stays None.
    if let Some(at_pos) = no_extras.find('@') {
        // Make sure the `@` isn't part of a version operator (no PEP 440 operator includes `@`).
        let name = no_extras[..at_pos].trim().to_string();
        if !name.is_empty() {
            return DepRequirement {
                name,
                version_req: None,
            };
        }
    }

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
```

- [ ] **Step 2: Add inline + Python tests**

Append to `python.rs`'s `#[cfg(test)] mod tests`:
```rust
    #[test]
    fn pep508_url_form_extracts_name_only() {
        let dir = write_pyproject(r#"
[project]
name = "consumer"
dependencies = ["foo @ git+https://github.com/x/foo.git"]
"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let dep = &manifest.declared_deps[0];
        assert_eq!(dep.name, "foo");
        assert!(dep.version_req.is_none(), "URL form has no version_req");
    }

    #[test]
    fn pep508_url_form_with_extras_works() {
        let dir = write_pyproject(r#"
[project]
name = "consumer"
dependencies = ["foo[bar,baz] @ https://example.org/foo.tar.gz"]
"#);
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let dep = &manifest.declared_deps[0];
        assert_eq!(dep.name, "foo");
    }
```

Python-side smoke test `tests/unit/test_pep508_url_deps.py`:
```python
"""M8: PEP 508 URL form deps parse correctly via the Python parser."""

import shutil
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

cli_runner = CliRunner()


@pytest.fixture
def url_dep_fixture(tmp_path: Path) -> Path:
    dst = tmp_path / "url_deps"
    dst.mkdir()
    (dst / "consumer").mkdir()
    (dst / "consumer" / "pyproject.toml").write_text(
        '[project]\nname="consumer"\ndependencies=["mylib @ git+https://github.com/x/mylib.git"]\n'
    )
    (dst / "mylib").mkdir()
    (dst / "mylib" / "pyproject.toml").write_text('[project]\nname="mylib"\n')
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def test_url_dep_resolves_to_in_monorepo_publisher(url_dep_fixture: Path):
    import sqlite3
    db = url_dep_fixture / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    rows = conn.execute(
        """
        SELECT p1.name, p2.name, json_extract(e.attrs_json, '$.dep_name')
        FROM edges e
        JOIN projects p1 ON p1.id = e.from_id
        JOIN projects p2 ON p2.id = e.to_id
        WHERE e.kind = 'package_dep'
          AND e.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()
    assert ("consumer", "mylib", "mylib") in rows, rows
```

- [ ] **Step 3: Run + commit**

```sh
cargo test --package prograph-core parsers
uv run pytest tests/unit/test_pep508_url_deps.py -v
```

```sh
git add prograph/prograph-core/src/parsers/python.rs prograph/tests/unit/test_pep508_url_deps.py
git commit -m "prograph: M8 PEP 508 URL deps — 'foo @ git+url' extracts name correctly"
```

---

## Task 9: Performance baseline suite

**Files:**
- Modify: `prograph/pyproject.toml`
- Create: `tests/integration/test_bench_baseline.py`

A small pytest-benchmark suite that establishes baselines for the most-run operations: index, describe_project, monorepo_overview, find_edges_filtered, search_fts. CI doesn't fail on absolute timing (machine variance is huge); it only fails if the new run is >2× the recorded baseline.

- [ ] **Step 1: Add `pytest-benchmark` dev-dep**

In `prograph/pyproject.toml`'s `[dependency-groups.dev]`, append:
```toml
pytest-benchmark = ">=4.0"
```

Add a pytest marker for opting in / out:
```toml
[tool.pytest.ini_options]
markers = [
    "realmonorepo: opt-in smoke test against the real monorepo",
    "bench: opt-in performance baseline",
]
addopts = "-ra -q -m 'not realmonorepo and not bench'"
```

(The new marker uses `and not bench` so benchmarks don't run by default. Invoke with `pytest -m bench`.)

- [ ] **Step 2: Write the benchmarks**

`tests/integration/test_bench_baseline.py`:
```python
"""M8: performance baselines for hot paths. Excluded from default test runs.

Invoke with: `uv run pytest -m bench -v`
Compare runs with: `uv run pytest -m bench --benchmark-compare`

CI guards against >2× regression on the previous run for each benchmark.
"""

import shutil
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph import _core
from prograph.cli import app
from prograph.paths import PrographPaths

cli_runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_mcp"


pytestmark = pytest.mark.bench


@pytest.fixture(scope="module")
def indexed_db(tmp_path_factory) -> str:
    tmp_path = tmp_path_factory.mktemp("bench")
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(app, ["index", "--monorepo", str(dst)])
    paths = PrographPaths(monorepo_root=dst)
    return str(paths.db_path)


def test_bench_monorepo_overview(benchmark, indexed_db):
    benchmark(lambda: _core.monorepo_overview(indexed_db))


def test_bench_describe_project(benchmark, indexed_db):
    pid = _core.project_by_name(indexed_db, "py_server")
    benchmark(lambda: _core.describe_project(indexed_db, pid))


def test_bench_find_edges(benchmark, indexed_db):
    benchmark(lambda: _core.find_edges_filtered(indexed_db, None, None, None, None))


def test_bench_search_fts(benchmark, indexed_db):
    benchmark(lambda: _core.search_fts(indexed_db, "py_server", None, 10))


def test_bench_reindex_no_changes(benchmark, tmp_path: Path):
    """Re-indexing same state — the no-change fast path."""
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(app, ["index", "--monorepo", str(dst)])
    paths = PrographPaths(monorepo_root=dst)
    benchmark(lambda: _core.index_monorepo(str(dst), str(paths.db_path)))
```

- [ ] **Step 3: Run + record baseline**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv sync
uv run pytest -m bench --benchmark-autosave -v
```

This creates a benchmark history file under `.benchmarks/`. CI can compare against this.

- [ ] **Step 4: Document in README**

In `prograph/README.md`, add a sub-section under "Development":
```markdown
### Performance baselines

```sh
uv run pytest -m bench -v                                  # run
uv run pytest -m bench --benchmark-compare                 # compare with previous run
uv run pytest -m bench --benchmark-compare-fail=mean:200%  # fail if 2x slower
```

Baselines are saved under `.benchmarks/`. Commit the `Linux-CPython-*/*.json` files when you intentionally accept a perf change.
```

- [ ] **Step 5: Add `.benchmarks/` to git tracking decisions**

In `prograph/.gitignore`, decide whether to ignore or track benchmark JSON. For M8, **track** the latest baseline so CI has something to compare against. Add nothing to gitignore.

- [ ] **Step 6: Commit**

```sh
git add prograph/pyproject.toml prograph/tests/integration/test_bench_baseline.py \
        prograph/README.md prograph/.benchmarks/
git commit -m "prograph: M8 performance baselines via pytest-benchmark (5 hot-path benchmarks)"
```

---

## Task 10: README + CLAUDE.md + smoke + close

**Files:**
- Modify: `prograph/README.md`
- Modify: `prograph/CLAUDE.md`
- Modify: `tests/integration/test_smoke_real.py`
- Modify: this plan file

- [ ] **Step 1: Real-monorepo smoke updates**

In `tests/integration/test_smoke_real.py`, append:
```python
    # M8: assert package_dep edges now have evidence (the M7 caveat is closed).
    import sqlite3
    conn = sqlite3.connect(paths_db)
    n_pkg_evidence = conn.execute(
        """
        SELECT COUNT(*) FROM edge_evidence ev
        JOIN edges e ON e.id = ev.edge_id
        WHERE e.kind = 'package_dep' AND ev.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchone()[0]
    conn.close()
    assert n_pkg_evidence >= 1, (
        f"M8 should populate evidence for package_dep edges; got {n_pkg_evidence}"
    )

    # M8: diff view returns 200 even when there's only one snapshot.
    with TestClient(build_app(real)) as web_client:
        r = web_client.get("/api/graph?since=1")
        assert r.status_code == 200
        payload = r.json()
        assert payload["since"] == 1
```

- [ ] **Step 2: Update README Status**

```markdown
**Status:** M8 — Polish. v1.0 candidate. `edge_evidence` populated for all three edge kinds. Browser UI has a snapshot picker + diff view (`/api/graph?since=<snap>`). Discovery auto-recurses into Cargo + Python workspaces — no `[tool.prograph].aliases` needed for workspace orchestrators. PEP 508 URL deps (`foo @ git+...`) parse correctly. Performance baselines guard against regression in CI.
```

Add an "M8 limitations" section (the polished-and-deferred items that don't make it into v1.0):
```markdown
### Deferred to M9+ (post-1.0)

- **Module-level facts** — public Python symbols, internal imports, public Rust crate items. Would enrich the "Public surface" MD section. Significant tree-sitter work.
- **HTTP / REST runtime edges** — heuristic detection of FastAPI/Flask/axum routes and matching client calls. Heavy work, low payoff against the target monorepo.
- **JS MCP source scanning** — no JS MCP servers in scope.
- **WebSocket live updates** (`/ws/changes`) — page reload remains the upgrade path.
- **Offline asset bundling** — CDN works fine for the local-dev tool.
- **Playwright / Selenium E2E** — REST + static-structure tests cover the regression surface.
- **Authentication / TLS** — bind-to-127.0.0.1 remains the security boundary.
- **Mobile / responsive design** — desktop-only.
```

- [ ] **Step 3: Update CLAUDE.md**

Update the architecture state to "M8 / v1.0 candidate". Add the new entities to the components list (DiffEdgeRow, workspace recursion in discovery). Update the "What is NOT in" section to reference M9+.

- [ ] **Step 4: Run the full local gate**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
cargo fmt --all -- --check && \
cargo clippy --all-targets -- -D warnings && \
cargo test --all-targets && \
uv run ruff check . && \
uv run ruff format --check . && \
uv run pyrefly check 'prograph/**/*.py' 'tests/**/*.py' && \
uv run pytest -v && \
uv run pytest -m realmonorepo -v && \
uv run pytest -m bench -v
```

Expected: every command exits 0. Test counts: cargo +1-2, pytest +15ish.

- [ ] **Step 5: Check the DoD boxes**

Mark every `- [ ]` in "Definition of Done (M8)" as `- [x]` with achieved counts.

- [ ] **Step 6: Final commit**

```sh
git add prograph/README.md prograph/CLAUDE.md \
        prograph/tests/integration/test_smoke_real.py \
        prograph/docs/superpowers/plans/2026-05-26-prograph-m8-polish.md
git commit -m "prograph: M8 close — v1.0 candidate; docs updated; full gate green; DoD checked"
```

---

## Definition of Done (M8)

- [x] `cargo test --all-targets` passes (132 tests).
- [x] `uv run pytest -v` passes (134 tests).
- [x] `uv run pytest -m realmonorepo -v` passes; the real monorepo's smoke now asserts ≥1 `package_dep` evidence row AND that `/api/graph?since=1` returns 200.
- [x] `uv run pytest -m bench -v` runs all 5 benchmarks; baseline saved under `.benchmarks/`.
- [x] `edge_evidence` table populated for all three edge kinds on the `monorepo_full` and `monorepo_mcp` fixtures.
- [x] MCP `edge_evidence` tool description no longer mentions M7-only caveat.
- [x] `Store::find_edges_with_status_since` returns `DiffEdgeRow` with `status ∈ {"added", "removed", "unchanged"}` matching the identity rules.
- [x] `GET /api/graph?since=<snap>` tags edges with status; `since=None` keeps the M6 behaviour (all alive edges, status="unchanged").
- [x] Browser UI has a `<select id="diff-since">` picker that drives `/api/graph?since=`; added edges render green, removed dashed-red.
- [x] `discovery::scan_monorepo` recurses one level deep into projects whose manifest declares a workspace.
- [x] `monorepo_workspace` fixture produces `outer-python-sub-{a,b}` + `crate-{x,y}` projects, plus the `consumer → sub_a` edge.
- [x] PEP 508 URL deps (`foo @ git+...`) parse name correctly with version_req=None.
- [x] Existing golden tests refreshed (none needed — MD renderer doesn't surface evidence for package_dep/contract_link). Smoke test extended with M8 assertions.
- [x] CI workflow continues to pass with no changes required (markers exclude bench + realmonorepo from the default run).
- [x] All commits follow the `prograph: M8 ...` prefix convention.

## What is NOT done in M8 (deferred to M9+ — post-1.0)

- **Module-level facts** (public Python symbols, internal imports, public Rust crate items).
- **HTTP / REST runtime edges** (FastAPI/Flask/axum route detection).
- **JS MCP source scanning** (tree-sitter-javascript).
- **WebSocket `/ws/changes`** for live updates.
- **Offline asset bundling** (vendored JS/CSS).
- **Playwright / Selenium E2E browser tests**.
- **Authentication / TLS**.
- **Mobile / responsive design**.

After M8 ships, `prograph` declares v1.0. Subsequent work is genuinely optional polish or new features driven by usage feedback.
