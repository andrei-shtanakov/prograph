# prograph M6 — Browser UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After M6, `prograph serve` starts a local web server at `http://127.0.0.1:7700` with an interactive graph view of the monorepo. Projects + contracts render as nodes, three edge kinds (`package_dep`, `mcp_call`, `contract_link`) render with distinct colors, clicking a node opens a side panel with full details, and a search box queries FTS. The same backend that powers MCP (M7) now also speaks REST so a human-facing UI can consume it.

**Architecture:**
- **REST API in Python via FastAPI** — `prograph/web_app.py`. Each endpoint is a 5-15 line wrapper over the existing `_core` queries (same shape as M7's MCP tool dispatchers). The 8 endpoints mirror the 8 MCP tools but speak HTTP JSON.
- **Static frontend ships with the Python wheel** under `prograph/web_static/` — `index.html`, `app.js`, `graph.js`, `dom.js`, `styles.css`. No build step, no bundler. Loaded via `<script type="module">`. Cytoscape and Pico CSS pulled from a CDN at runtime.
- **DOM construction is XSS-safe by design** — `dom.js` exposes a small `el(tag, attrs, children)` helper that uses `document.createElement` + `textContent`. **No `innerHTML` assignments anywhere in the frontend.** Untrusted server data flows through `textContent` only.
- **Cytoscape.js graph layout** — `cose-bilkent` for spring-force placement, project nodes as rounded rectangles, contract nodes as diamonds, edges colored by kind (deps=gray, mcp=teal, contract=amber). Click handlers populate a right-side panel by fetching `/api/projects/{id}` or `/api/edges/{id}`.
- **`prograph serve` CLI** — typer command that boots uvicorn against the FastAPI app. Defaults to `127.0.0.1:7700`; binding to non-loopback emits a warning per spec §7.3.
- **No browser-level UI tests in M6.** Integration tests use FastAPI's `TestClient` for the REST layer; the static front-end is verified by smoke-loading the HTML and asserting it contains the expected element IDs (`#graph`, `#sidepanel`, `#search-box`). Playwright/E2E browser automation is M8 polish.

**Tech Stack additions (M6 only):**
- `fastapi` (≥0.110) — REST framework
- `uvicorn[standard]` (≥0.29) — ASGI server
- `httpx` is FastAPI's TestClient transitive dep — verify during Task 1

Front-end uses **runtime CDN-loaded dependencies** — no Python deps for those:
- cytoscape.js 3.30
- cytoscape-cose-bilkent 4.1
- Pico CSS 2.0 (minimalist styling)

No new Rust deps.

**Spec reference:** `docs/superpowers/specs/2026-05-25-prograph-design.md` §7.3 — REST endpoint table (the 8 endpoints shipped here). §3 architecture (browser UI is the human-facing peer of the MCP server). §11 open question #4 cites cytoscape.js as one of three candidates — M6 commits to it.

**Baseline:** Branch off `main` at the M7 close commit (the user reported M7 is complete; check `git log` for the exact SHA). 128+ cargo + 100+ pytest passing; CI green; `prograph mcp` exposes 8 tools.

**M6 explicitly out of scope (deferred to M8+):**
- **Diff view** (`GET /api/graph?since=<snap>`) — surfaces structural changes between two snapshots. Schema supports it (last_seen ranges); endpoint is M6 stretch, deferred for now.
- **WebSocket `/ws/changes`** for live updates — spec §7.3 marks it post-MVP. Defer.
- **Authentication / TLS** — local-dev only.
- **Browser-level E2E tests** (Playwright/Selenium) — M8 polish.
- **Offline asset bundling** — CDN is fine for M6; vendored JS bundle is M8.
- **Mobile / responsive design** — desktop-only.

---

## File Structure (created/modified in M6)

```
prograph/
├── prograph/
│   ├── web_app.py                          # NEW — FastAPI app factory + 8 endpoints
│   ├── web_static/                         # NEW — frontend bundle
│   │   ├── index.html                      # NEW
│   │   ├── dom.js                          # NEW — XSS-safe DOM construction helper
│   │   ├── app.js                          # NEW — UI logic, fetch helpers, event wiring
│   │   ├── graph.js                        # NEW — cytoscape graph setup + rendering
│   │   └── styles.css                      # NEW
│   ├── cli.py                              # MODIFY — add `prograph serve` command
│   ├── pyproject.toml                      # MODIFY — add fastapi + uvicorn deps
│   └── paths.py                            # unchanged
├── tests/
│   ├── unit/
│   │   └── test_web_static.py              # NEW — sanity-check the static HTML
│   └── integration/
│       └── test_cli_serve.py               # NEW — FastAPI TestClient against the 8 endpoints
```

No Rust changes whatsoever. M6 is pure Python + static frontend.

---

## Task 1: FastAPI + uvicorn workspace deps

**Files:**
- Modify: `prograph/pyproject.toml`

- [ ] **Step 1: Add the two new deps**

In `prograph/pyproject.toml`, append to `[project.dependencies]`:
```toml
fastapi = ">=0.110"
uvicorn = {version = ">=0.29", extras = ["standard"]}
```

(`uvicorn[standard]` pulls `websockets`, `httptools`, `watchfiles` — slightly heavier but standard.)

- [ ] **Step 2: Sync + verify imports**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv sync --reinstall-package prograph
uv run python -c "import fastapi, uvicorn; print('fastapi', fastapi.__version__, 'uvicorn', uvicorn.__version__)"
```

Both should print versions without error.

- [ ] **Step 3: Verify httpx is available for TestClient**

```sh
uv run python -c "from fastapi.testclient import TestClient; print('TestClient OK')"
```

If this fails (FastAPI versions vary in whether httpx is mandatory), add `httpx = ">=0.27"` to `[dependency-groups.dev]` in pyproject.toml.

- [ ] **Step 4: Commit**

```sh
git add prograph/pyproject.toml prograph/uv.lock
git commit -m "prograph: M6 add fastapi + uvicorn dependencies"
```

---

## Task 2: FastAPI app scaffold + lifespan

**Files:**
- Create: `prograph/web_app.py`

Build the FastAPI app factory with lifespan management (resolves the DB path from CLI argv, validates the monorepo is initialised). One trivial GET endpoint (`/api/health`) lands here to verify the scaffold works; the 8 real endpoints land in Tasks 3-10.

- [ ] **Step 1: Write the scaffold**

`prograph/web_app.py`:
```python
"""prograph web server — REST API + static UI for the snapshot graph.

The app is created via `build_app(monorepo_root)` so test code can construct it
with an arbitrary monorepo root without going through CLI argv parsing.
"""

from __future__ import annotations

from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI, HTTPException
from fastapi.responses import HTMLResponse
from fastapi.staticfiles import StaticFiles

from prograph.paths import PrographPaths

_STATIC_DIR = Path(__file__).parent / "web_static"


def build_app(monorepo_root: Path) -> FastAPI:
    """Construct a FastAPI app bound to the given monorepo.

    Validates that `.prograph/graph.db` exists at startup — `prograph serve`
    will refuse to start otherwise.
    """
    paths = PrographPaths(monorepo_root=monorepo_root)

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        if not paths.is_initialized():
            raise RuntimeError(
                f"prograph not initialized at {paths.prograph_dir}. "
                "Run `prograph init && prograph index` first."
            )
        if not paths.db_path.exists():
            raise RuntimeError(
                f"no snapshot at {paths.db_path}. Run `prograph index` first."
            )
        yield

    app = FastAPI(
        title="prograph",
        description="Monorepo cross-project structure mapper (M6 browser UI).",
        version="0.1.0",
        lifespan=lifespan,
    )

    # Stash the db_path on app.state for endpoint access.
    app.state.db_path = str(paths.db_path)
    app.state.monorepo_root = str(monorepo_root)

    @app.get("/api/health")
    async def health() -> dict:
        return {"status": "ok", "monorepo_root": app.state.monorepo_root}

    # Mount static assets at /static; serve index.html at root.
    if _STATIC_DIR.is_dir():
        app.mount("/static", StaticFiles(directory=_STATIC_DIR), name="static")

        @app.get("/", response_class=HTMLResponse)
        async def root() -> str:
            index_path = _STATIC_DIR / "index.html"
            if not index_path.exists():
                raise HTTPException(status_code=500, detail="frontend not bundled")
            return index_path.read_text(encoding="utf-8")
    else:
        @app.get("/", response_class=HTMLResponse)
        async def root_no_static() -> str:
            return "<h1>prograph</h1><p>Static UI not bundled; use /api/* endpoints.</p>"

    return app
```

- [ ] **Step 2: Sanity test**

```sh
uv run python -c "
from prograph.web_app import build_app
from pathlib import Path
app = build_app(Path('/tmp/nonexistent'))
print('app routes:', [r.path for r in app.routes if hasattr(r, 'path')])
"
```

Expected: prints app routes including `/api/health` and `/`. Lifespan validation only runs at server startup, not at app construction.

- [ ] **Step 3: Commit**

```sh
git add prograph/prograph/web_app.py
git commit -m "prograph: M6 FastAPI app scaffold — lifespan validates .prograph/graph.db"
```

---

## Task 3: `GET /api/graph`

**Files:**
- Modify: `prograph/web_app.py`

The main endpoint the frontend hits on load. Returns `{nodes: [...], edges: [...], snapshot_id: N}` where:
- `nodes` is a union of projects + contracts (each tagged with `node_kind`)
- `edges` is the alive edge set with from/to node ids

This requires building a node id namespace. We use `"p:<project_slug>"` for projects and `"c:<contract_slug>"` for contracts so cytoscape gets unique ids regardless of underlying entity type.

- [ ] **Step 1: Implement the endpoint**

In `prograph/web_app.py`, inside `build_app`, append:
```python
    @app.get("/api/graph")
    async def graph() -> dict:
        from prograph import _core
        from prograph.export.slug import slugify
        from prograph.models import MonorepoOverview

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

        # Edges — reuse find_edges_filtered with no filters → all alive edges.
        raw_edges = _core.find_edges_filtered(app.state.db_path, None, None, None, None)
        edges_out: list[dict] = []
        for e in raw_edges:
            from_id = f"{'p' if e.from_kind == 'project' else 'c'}:{slugify(e.from_name)}"
            if e.to_kind == "contract":
                to_id = f"c:{slugify(e.to_name)}"
            else:
                to_id = f"p:{slugify(e.to_name)}"
            edges_out.append({
                "id": f"e:{e.id}",
                "source": from_id,
                "target": to_id,
                "kind": e.kind,
                "edge_id": e.id,
            })

        return {
            "snapshot_id": ov.snapshot_id,
            "snapshot_ts": ov.snapshot_ts,
            "n_projects": ov.n_projects,
            "n_contracts": ov.n_contracts,
            "n_edges": ov.n_edges,
            "nodes": nodes,
            "edges": edges_out,
        }
```

- [ ] **Step 2: Smoke test by hand**

```sh
cd /tmp && rm -rf m6smoke && mkdir m6smoke && cd m6smoke
mkdir alpha beta
printf '[project]\nname = "alpha"\n' > alpha/pyproject.toml
printf '[project]\nname = "beta"\ndependencies = ["alpha"]\n' > beta/pyproject.toml

cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run prograph init --monorepo /tmp/m6smoke
uv run prograph index --monorepo /tmp/m6smoke
uv run python -c "
from fastapi.testclient import TestClient
from prograph.web_app import build_app
from pathlib import Path
client = TestClient(build_app(Path('/tmp/m6smoke')))
import json
print(json.dumps(client.get('/api/graph').json(), indent=2))
"
rm -rf /tmp/m6smoke
```

Expected: prints `{nodes: [alpha, beta], edges: [beta→alpha], snapshot_id: 1, ...}`.

- [ ] **Step 3: Commit**

```sh
git add prograph/prograph/web_app.py
git commit -m "prograph: M6 GET /api/graph — projects + contracts nodes, edges with from/to slug ids"
```

---

## Task 4: `GET /api/projects/by-name/{name}` + `by-id/{id}`

**Files:**
- Modify: `prograph/web_app.py`

- [ ] **Step 1: Implement both endpoints**

In `build_app`, append:
```python
    @app.get("/api/projects/by-name/{name}")
    async def project_by_name(name: str) -> dict:
        from prograph import _core
        from prograph.models import ProjectDescription

        pid = _core.project_by_name(app.state.db_path, name)
        if pid is None:
            raise HTTPException(status_code=404, detail=f"project not found: {name}")
        raw = _core.describe_project(app.state.db_path, pid)
        if raw is None:
            raise HTTPException(status_code=404, detail=f"project_id {pid} not in latest snapshot")
        return ProjectDescription.from_core(raw).model_dump(mode="json")

    @app.get("/api/projects/by-id/{project_id}")
    async def project_by_id(project_id: int) -> dict:
        from prograph import _core
        from prograph.models import ProjectDescription

        raw = _core.describe_project(app.state.db_path, project_id)
        if raw is None:
            raise HTTPException(status_code=404, detail=f"project_id {project_id} not in latest snapshot")
        return ProjectDescription.from_core(raw).model_dump(mode="json")
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/web_app.py
git commit -m "prograph: M6 GET /api/projects/by-{name,id}"
```

---

## Task 5: `GET /api/contracts/by-{id,slug}`

**Files:**
- Modify: `prograph/web_app.py`

- [ ] **Step 1: Implement**

```python
    @app.get("/api/contracts/by-id/{contract_id}")
    async def contract_by_id(contract_id: int) -> dict:
        from prograph import _core
        from prograph.models import ContractDescription

        raw = _core.describe_contract(app.state.db_path, contract_id)
        if raw is None:
            raise HTTPException(status_code=404, detail=f"contract_id {contract_id} not in latest snapshot")
        return ContractDescription.from_core(raw).model_dump(mode="json")

    @app.get("/api/contracts/by-slug/{slug}")
    async def contract_by_slug(slug: str) -> dict:
        from prograph import _core
        from prograph.models import ContractDescription, MonorepoOverview

        raw_ov = _core.monorepo_overview(app.state.db_path)
        if raw_ov is None:
            raise HTTPException(status_code=404, detail="no snapshot")
        ov = MonorepoOverview.from_core(raw_ov)
        matched = [c for c in ov.contracts if c.slug == slug]
        if not matched:
            raise HTTPException(status_code=404, detail=f"contract slug not found: {slug}")

        # Resolve contract slug → DB id via small direct sqlite3 lookup.
        import sqlite3
        conn = sqlite3.connect(app.state.db_path)
        try:
            target = matched[0]
            row = conn.execute(
                """
                SELECT id FROM contracts
                WHERE COALESCE(declared_id, '') = COALESCE(?, '')
                  AND last_seen = (SELECT MAX(id) FROM snapshots)
                LIMIT 1
                """,
                (target.declared_id,),
            ).fetchone()
        finally:
            conn.close()
        if not row:
            raise HTTPException(status_code=404, detail=f"contract row not found for slug {slug}")

        raw = _core.describe_contract(app.state.db_path, row[0])
        if raw is None:
            raise HTTPException(status_code=404, detail=f"contract_id {row[0]} not in latest snapshot")
        return ContractDescription.from_core(raw).model_dump(mode="json")
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/web_app.py
git commit -m "prograph: M6 GET /api/contracts/by-{id,slug}"
```

---

## Task 6: `GET /api/edges/{edge_id}` with evidence

**Files:**
- Modify: `prograph/web_app.py`

- [ ] **Step 1: Implement**

```python
    @app.get("/api/edges/{edge_id}")
    async def edge_by_id(edge_id: int) -> dict:
        from prograph import _core
        from prograph.models import EdgeEvidenceRow, EdgeRow

        all_edges = _core.find_edges_filtered(app.state.db_path, None, None, None, None)
        match = next((e for e in all_edges if e.id == edge_id), None)
        if match is None:
            raise HTTPException(status_code=404, detail=f"edge_id {edge_id} not in latest snapshot")

        edge_dict = EdgeRow.from_core(match).model_dump(mode="json")
        ev_rows = _core.edge_evidence_for(app.state.db_path, edge_id)
        edge_dict["evidence"] = [EdgeEvidenceRow.from_core(r).model_dump(mode="json") for r in ev_rows]
        return edge_dict
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/web_app.py
git commit -m "prograph: M6 GET /api/edges/{id} with evidence"
```

---

## Task 7: `GET /api/changelog`

**Files:**
- Modify: `prograph/web_app.py`

- [ ] **Step 1: Implement**

```python
    @app.get("/api/changelog")
    async def changelog(
        since: int | None = None,
        entity_kind: str | None = None,
        limit: int = 50,
    ) -> list[dict]:
        from prograph import _core
        from prograph.models import ChangeEvent

        events = _core.changelog_paginated(app.state.db_path, since, entity_kind, limit)
        return [ChangeEvent.from_core(e).model_dump(mode="json") for e in events]
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/web_app.py
git commit -m "prograph: M6 GET /api/changelog"
```

---

## Task 8: `GET /api/search`

**Files:**
- Modify: `prograph/web_app.py`

- [ ] **Step 1: Implement**

```python
    @app.get("/api/search")
    async def search(q: str, kinds: str | None = None, limit: int = 20) -> list[dict]:
        """Full-text search. `kinds` is a comma-separated list."""
        from prograph import _core
        from prograph.models import SearchHit

        kind_list = [k.strip() for k in kinds.split(",")] if kinds else None
        hits = _core.search_fts(app.state.db_path, q, kind_list, limit)
        return [SearchHit.from_core(h).model_dump(mode="json") for h in hits]
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/web_app.py
git commit -m "prograph: M6 GET /api/search (FTS)"
```

---

## Task 9: `GET /api/snapshots[/{id}]`

**Files:**
- Modify: `prograph/web_app.py`

- [ ] **Step 1: Implement both**

```python
    @app.get("/api/snapshots")
    async def list_snapshots(limit: int = 50) -> list[dict]:
        from prograph import _core
        from prograph.models import SnapshotInfo

        # Quick direct read for snapshot ids. _core has no list helper.
        import sqlite3
        conn = sqlite3.connect(app.state.db_path)
        try:
            rows = conn.execute(
                "SELECT id FROM snapshots ORDER BY id DESC LIMIT ?", (limit,)
            ).fetchall()
        finally:
            conn.close()

        out: list[dict] = []
        for (sid,) in rows:
            raw = _core.snapshot_by_id(app.state.db_path, sid)
            if raw is not None:
                out.append(SnapshotInfo.from_core(raw).model_dump(mode="json"))
        return out

    @app.get("/api/snapshots/{snapshot_id}")
    async def snapshot_by_id_endpoint(snapshot_id: int) -> dict:
        from prograph import _core
        from prograph.models import SnapshotInfo

        raw = _core.snapshot_by_id(app.state.db_path, snapshot_id)
        if raw is None:
            raise HTTPException(status_code=404, detail=f"snapshot {snapshot_id} not found")
        return SnapshotInfo.from_core(raw).model_dump(mode="json")
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/web_app.py
git commit -m "prograph: M6 GET /api/snapshots (list + by-id)"
```

---

## Task 10: REST API integration tests

**Files:**
- Create: `tests/integration/test_cli_serve.py`

Test all endpoints against `monorepo_mcp` via FastAPI's `TestClient` (in-process, no subprocess).

- [ ] **Step 1: Write the tests**

`tests/integration/test_cli_serve.py`:
```python
"""REST API integration tests via FastAPI TestClient."""

import shutil
from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from typer.testing import CliRunner

from prograph.cli import app as cli_app
from prograph.web_app import build_app

cli_runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_mcp"


@pytest.fixture
def indexed_fixture(tmp_path: Path) -> Path:
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    cli_runner.invoke(cli_app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(cli_app, ["index", "--monorepo", str(dst)])
    return dst


@pytest.fixture
def client(indexed_fixture: Path) -> TestClient:
    return TestClient(build_app(indexed_fixture))


def test_health(client: TestClient):
    r = client.get("/api/health")
    assert r.status_code == 200
    assert r.json()["status"] == "ok"


def test_root_serves_html(client: TestClient):
    r = client.get("/")
    assert r.status_code == 200
    text_lc = r.text.lower()
    assert "<h1" in text_lc or "<!doctype html>" in text_lc


def test_graph_returns_nodes_and_edges(client: TestClient):
    r = client.get("/api/graph")
    assert r.status_code == 200
    payload = r.json()
    assert payload["n_projects"] == 6
    assert payload["n_edges"] >= 5
    node_kinds = {n["node_kind"] for n in payload["nodes"]}
    assert "project" in node_kinds
    assert "contract" in node_kinds


def test_project_by_name(client: TestClient):
    r = client.get("/api/projects/by-name/py_server")
    assert r.status_code == 200
    assert r.json()["name"] == "py_server"


def test_project_by_name_404(client: TestClient):
    r = client.get("/api/projects/by-name/nonexistent")
    assert r.status_code == 404


def test_project_by_id(client: TestClient):
    by_name = client.get("/api/projects/by-name/py_server").json()
    pid = by_name["project_id"]
    r = client.get(f"/api/projects/by-id/{pid}")
    assert r.status_code == 200
    assert r.json()["name"] == "py_server"


def test_edge_with_evidence(client: TestClient):
    graph = client.get("/api/graph").json()
    mcp_edges = [e for e in graph["edges"] if e["kind"] == "mcp_call"]
    assert mcp_edges
    edge_id = mcp_edges[0]["edge_id"]
    r = client.get(f"/api/edges/{edge_id}")
    assert r.status_code == 200
    payload = r.json()
    assert payload["kind"] == "mcp_call"
    assert isinstance(payload["evidence"], list)
    assert len(payload["evidence"]) >= 1


def test_changelog_returns_list(client: TestClient):
    r = client.get("/api/changelog?limit=5")
    assert r.status_code == 200
    assert isinstance(r.json(), list)


def test_search_finds_project(client: TestClient):
    r = client.get("/api/search?q=py_server")
    assert r.status_code == 200
    payload = r.json()
    assert any(h["name"] == "py_server" for h in payload)


def test_snapshots_list_and_by_id(client: TestClient):
    r = client.get("/api/snapshots")
    assert r.status_code == 200
    snaps = r.json()
    assert len(snaps) >= 1
    sid = snaps[0]["id"]
    r2 = client.get(f"/api/snapshots/{sid}")
    assert r2.status_code == 200
    assert r2.json()["id"] == sid


def test_contract_endpoints(client: TestClient):
    graph = client.get("/api/graph").json()
    contract_nodes = [n for n in graph["nodes"] if n["node_kind"] == "contract"]
    assert contract_nodes
    slug = contract_nodes[0]["id"].removeprefix("c:")
    r = client.get(f"/api/contracts/by-slug/{slug}")
    assert r.status_code == 200
    assert "owners" in r.json()
```

- [ ] **Step 2: Run**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run pytest tests/integration/test_cli_serve.py -v
```
Expected: 11 passed.

Full suite:
```sh
uv run pytest -v
```
Expected: 111+ tests.

- [ ] **Step 3: Commit**

```sh
git add prograph/tests/integration/test_cli_serve.py
git commit -m "prograph: M6 REST API integration tests — 11 tests covering all 8 endpoints"
```

---

## Task 11: Static frontend — HTML + CSS + DOM helper

**Files:**
- Create: `prograph/web_static/index.html`
- Create: `prograph/web_static/styles.css`
- Create: `prograph/web_static/dom.js`

The base HTML + CSS + a small XSS-safe DOM construction helper used by both `app.js` (Task 13) and `graph.js` (Task 12).

- [ ] **Step 1: Write `index.html`**

`prograph/web_static/index.html`:
```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>prograph</title>
    <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/@picocss/pico@2/css/pico.min.css">
    <link rel="stylesheet" href="/static/styles.css">
    <script src="https://cdn.jsdelivr.net/npm/cytoscape@3.30/dist/cytoscape.min.js"></script>
    <script src="https://cdn.jsdelivr.net/npm/layout-base/layout-base.js"></script>
    <script src="https://cdn.jsdelivr.net/npm/cose-base/cose-base.js"></script>
    <script src="https://cdn.jsdelivr.net/npm/cytoscape-cose-bilkent@4.1/cytoscape-cose-bilkent.js"></script>
</head>
<body>
    <header id="topbar">
        <h1>prograph</h1>
        <div id="snapshot-info"></div>
        <div id="search-container">
            <input id="search-box" type="text" placeholder="Search projects + contracts…">
            <div id="search-results"></div>
        </div>
    </header>
    <main>
        <div id="graph"></div>
        <aside id="sidepanel">
            <p class="placeholder">Click a node or edge to see details.</p>
        </aside>
    </main>
    <footer id="activity">
        <h3>Recent activity</h3>
        <ol id="activity-list"></ol>
    </footer>
    <script type="module" src="/static/app.js"></script>
</body>
</html>
```

- [ ] **Step 2: Write `styles.css`**

`prograph/web_static/styles.css`:
```css
* { box-sizing: border-box; }

body {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    display: flex;
    flex-direction: column;
    height: 100vh;
}

#topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid var(--pico-muted-border-color, #ddd);
    gap: 1rem;
}

#topbar h1 {
    margin: 0;
    font-size: 1.25rem;
}

#snapshot-info {
    font-size: 0.85rem;
    color: var(--pico-muted-color, #666);
    flex: 1;
    text-align: center;
}

#search-container {
    position: relative;
    min-width: 18rem;
}

#search-box {
    width: 100%;
    margin: 0;
    padding: 0.4rem 0.6rem;
}

#search-results {
    position: absolute;
    top: 100%;
    right: 0;
    width: 100%;
    background: var(--pico-background-color, white);
    border: 1px solid var(--pico-muted-border-color, #ddd);
    border-radius: 4px;
    max-height: 16rem;
    overflow-y: auto;
    display: none;
    z-index: 10;
}

#search-results.visible {
    display: block;
}

#search-results .hit {
    padding: 0.4rem 0.6rem;
    cursor: pointer;
    border-bottom: 1px solid var(--pico-muted-border-color, #eee);
}

#search-results .hit:hover {
    background: var(--pico-secondary-background, #f4f4f4);
}

#search-results .hit-kind {
    font-size: 0.75rem;
    color: var(--pico-muted-color, #888);
    margin-left: 0.5rem;
}

main {
    flex: 1;
    display: flex;
    overflow: hidden;
}

#graph {
    flex: 1;
    background: var(--pico-background-color, #fafafa);
}

#sidepanel {
    width: 24rem;
    padding: 1rem;
    border-left: 1px solid var(--pico-muted-border-color, #ddd);
    overflow-y: auto;
}

#sidepanel .placeholder {
    color: var(--pico-muted-color, #888);
    text-align: center;
    margin-top: 4rem;
}

#sidepanel h2 {
    margin-top: 0;
    font-size: 1.1rem;
}

#sidepanel dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.25rem 0.5rem;
    font-size: 0.9rem;
}

#sidepanel dt {
    color: var(--pico-muted-color, #666);
    font-weight: 500;
}

#sidepanel dd {
    margin: 0;
}

#sidepanel ul {
    padding-left: 1.25rem;
    font-size: 0.9rem;
}

#sidepanel li {
    margin-bottom: 0.25rem;
}

#activity {
    border-top: 1px solid var(--pico-muted-border-color, #ddd);
    padding: 0.5rem 1rem;
    max-height: 8rem;
    overflow-y: auto;
}

#activity h3 {
    margin: 0 0 0.25rem 0;
    font-size: 0.85rem;
    color: var(--pico-muted-color, #666);
}

#activity ol {
    margin: 0;
    padding-left: 1.25rem;
    font-size: 0.8rem;
}
```

- [ ] **Step 3: Write `dom.js`**

`prograph/web_static/dom.js` — the XSS-safe DOM construction helper:
```javascript
/* eslint-env browser */

/**
 * Create a DOM element safely.
 *
 *   el('div', {class: 'card', dataset: {id: '7'}}, [
 *       el('strong', {}, ['Title']),
 *       ' — body text',
 *   ])
 *
 * All string children are inserted via createTextNode — they CANNOT inject HTML.
 * To insert another element, pass it as a child directly.
 *
 * NEVER pass user-controlled strings as the tag name or attribute name; only
 * attribute values are safe under this contract.
 */
export function el(tag, attrs, children) {
    const node = document.createElement(tag);
    if (attrs) {
        for (const key of Object.keys(attrs)) {
            const val = attrs[key];
            if (val === null || val === undefined) continue;
            if (key === 'class') {
                node.className = String(val);
            } else if (key === 'dataset') {
                for (const dk of Object.keys(val)) {
                    node.dataset[dk] = String(val[dk]);
                }
            } else if (key === 'onclick' && typeof val === 'function') {
                node.addEventListener('click', val);
            } else {
                node.setAttribute(key, String(val));
            }
        }
    }
    if (children) {
        for (const child of children) {
            if (child === null || child === undefined) continue;
            if (typeof child === 'string' || typeof child === 'number') {
                node.appendChild(document.createTextNode(String(child)));
            } else if (child instanceof Node) {
                node.appendChild(child);
            }
        }
    }
    return node;
}

/** Replace all children of `parent` with the given nodes. */
export function setChildren(parent, children) {
    parent.replaceChildren();
    for (const child of children) {
        if (child === null || child === undefined) continue;
        if (typeof child === 'string' || typeof child === 'number') {
            parent.appendChild(document.createTextNode(String(child)));
        } else if (child instanceof Node) {
            parent.appendChild(child);
        }
    }
}

/** Replace all children with a single message node. */
export function setMessage(parent, message, cls) {
    setChildren(parent, [el('p', cls ? {class: cls} : {}, [message])]);
}
```

- [ ] **Step 4: Commit**

```sh
git add prograph/prograph/web_static/index.html prograph/prograph/web_static/styles.css prograph/prograph/web_static/dom.js
git commit -m "prograph: M6 static frontend scaffold — HTML + CSS + XSS-safe DOM helper"
```

---

## Task 12: Frontend `graph.js` — cytoscape setup + rendering

**Files:**
- Create: `prograph/web_static/graph.js`

The graph module fetches `/api/graph` and renders projects (rounded rectangles) + contracts (diamonds) with edges colored by kind. Exposes `buildCytoscape(container)` and `renderGraph(cy, data)`. Click handlers dispatch CustomEvents for `app.js` to consume.

- [ ] **Step 1: Write `graph.js`**

`prograph/web_static/graph.js`:
```javascript
/* eslint-env browser */

const KIND_COLORS = {
    package_dep: '#888',
    mcp_call: '#0fa3b1',
    contract_link: '#d8862c',
};

const KIND_LINESTYLES = {
    package_dep: 'solid',
    mcp_call: 'solid',
    contract_link: 'dashed',
};

const PROJECT_KIND_COLORS = {
    python: '#3776ab',
    rust: '#dea584',
    js: '#f0db4f',
    docs: '#888',
    mixed: '#9b59b6',
};

function attachCoseBilkent() {
    // cose-bilkent registers itself as a global on script load. Just ensure it's wired.
    if (window.cytoscape && window.cytoscapeCoseBilkent) {
        window.cytoscape.use(window.cytoscapeCoseBilkent);
    }
}

export function buildCytoscape(container) {
    attachCoseBilkent();
    // eslint-disable-next-line no-undef
    const cy = cytoscape({
        container,
        elements: [],
        style: [
            {
                selector: 'node[node_kind = "project"]',
                style: {
                    'shape': 'round-rectangle',
                    'label': 'data(label)',
                    'background-color': (n) => PROJECT_KIND_COLORS[n.data('kind')] || '#888',
                    'color': '#fff',
                    'text-valign': 'center',
                    'text-halign': 'center',
                    'width': 'label',
                    'height': 40,
                    'padding': '12px',
                    'font-size': 12,
                    'font-weight': 600,
                    'border-width': 1,
                    'border-color': '#333',
                },
            },
            {
                selector: 'node[node_kind = "contract"]',
                style: {
                    'shape': 'diamond',
                    'label': 'data(label)',
                    'background-color': '#fff',
                    'color': '#333',
                    'text-valign': 'bottom',
                    'text-halign': 'center',
                    'text-margin-y': 8,
                    'width': 40,
                    'height': 40,
                    'font-size': 11,
                    'font-style': 'italic',
                    'border-width': 2,
                    'border-color': '#d8862c',
                },
            },
            {
                selector: 'edge',
                style: {
                    'curve-style': 'bezier',
                    'target-arrow-shape': 'triangle',
                    'line-color': (e) => KIND_COLORS[e.data('kind')] || '#888',
                    'target-arrow-color': (e) => KIND_COLORS[e.data('kind')] || '#888',
                    'line-style': (e) => KIND_LINESTYLES[e.data('kind')] || 'solid',
                    'width': 2,
                    'arrow-scale': 1.2,
                },
            },
            {
                selector: ':selected',
                style: {
                    'border-color': '#222',
                    'border-width': 3,
                    'line-color': '#222',
                    'target-arrow-color': '#222',
                },
            },
        ],
        layout: { name: 'cose-bilkent', animate: false, nodeRepulsion: 4500 },
        wheelSensitivity: 0.2,
    });

    cy.on('tap', 'node', (evt) => {
        const node = evt.target;
        const detail = {
            type: 'node',
            id: node.id(),
            node_kind: node.data('node_kind'),
            name: node.data('name'),
        };
        window.dispatchEvent(new CustomEvent('prograph:select', { detail }));
    });

    cy.on('tap', 'edge', (evt) => {
        const edge = evt.target;
        const detail = {
            type: 'edge',
            edge_id: edge.data('edge_id'),
            kind: edge.data('kind'),
        };
        window.dispatchEvent(new CustomEvent('prograph:select', { detail }));
    });

    cy.on('tap', (evt) => {
        if (evt.target === cy) {
            window.dispatchEvent(new CustomEvent('prograph:deselect'));
        }
    });

    return cy;
}

export function renderGraph(cy, data) {
    cy.elements().remove();
    cy.add(
        data.nodes.map((n) => ({
            data: {
                id: n.id,
                label: n.label,
                name: n.name,
                kind: n.kind,
                node_kind: n.node_kind,
            },
        }))
    );
    cy.add(
        data.edges.map((e) => ({
            data: {
                id: e.id,
                source: e.source,
                target: e.target,
                kind: e.kind,
                edge_id: e.edge_id,
            },
        }))
    );
    cy.layout({ name: 'cose-bilkent', animate: false, nodeRepulsion: 4500 }).run();
}
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/web_static/graph.js
git commit -m "prograph: M6 graph.js — cytoscape setup, kind-colored edges, click event dispatch"
```

---

## Task 13: Frontend `app.js` — side panel + search + activity (XSS-safe DOM)

**Files:**
- Create: `prograph/web_static/app.js`

The main UI module. All DOM construction goes through `dom.js`'s `el()` helper — no `innerHTML` anywhere, all user-facing strings flow through `textContent` via the helper.

- [ ] **Step 1: Write `app.js`**

`prograph/web_static/app.js`:
```javascript
/* eslint-env browser */
import { buildCytoscape, renderGraph } from './graph.js';
import { el, setChildren, setMessage } from './dom.js';

const cy = buildCytoscape(document.getElementById('graph'));

async function fetchJson(url) {
    const r = await fetch(url);
    if (!r.ok) throw new Error(`${url} → ${r.status}`);
    return r.json();
}

async function init() {
    const data = await fetchJson('/api/graph');
    renderGraph(cy, data);

    const info = document.getElementById('snapshot-info');
    info.textContent = `snapshot #${data.snapshot_id} · ${data.n_projects} projects · ${data.n_contracts} contracts · ${data.n_edges} edges`;

    refreshActivity();
}

async function refreshActivity() {
    try {
        const events = await fetchJson('/api/changelog?limit=10');
        const list = document.getElementById('activity-list');
        setChildren(list, events.map(renderActivityRow));
    } catch (e) {
        console.warn('activity fetch failed', e);
    }
}

function renderActivityRow(ev) {
    return el('li', {}, [
        el('span', { class: 'ts' }, [ev.ts]),
        ' · ',
        `${ev.entity_kind} ${ev.entity_id}: `,
        el('strong', {}, [ev.change]),
    ]);
}

// ─── Side panel ───────────────────────────────────────────────────────────────

const sidepanel = document.getElementById('sidepanel');

window.addEventListener('prograph:select', async (evt) => {
    const detail = evt.detail;
    setMessage(sidepanel, 'Loading…');
    try {
        if (detail.type === 'node' && detail.node_kind === 'project') {
            const payload = await fetchJson(`/api/projects/by-name/${encodeURIComponent(detail.name)}`);
            setChildren(sidepanel, renderProject(payload));
        } else if (detail.type === 'node' && detail.node_kind === 'contract') {
            const slug = detail.id.replace(/^c:/, '');
            const payload = await fetchJson(`/api/contracts/by-slug/${encodeURIComponent(slug)}`);
            setChildren(sidepanel, renderContract(payload));
        } else if (detail.type === 'edge') {
            const payload = await fetchJson(`/api/edges/${detail.edge_id}`);
            setChildren(sidepanel, renderEdge(payload));
        }
    } catch (e) {
        setMessage(sidepanel, `Error loading: ${e.message}`);
    }
});

window.addEventListener('prograph:deselect', () => {
    setMessage(sidepanel, 'Click a node or edge to see details.', 'placeholder');
});

// Render functions return arrays of DOM nodes; setChildren swaps them into the side panel.

function renderProject(p) {
    const nodes = [
        el('h2', {}, [p.name]),
        renderDl([
            ['kind', p.kind],
            ['root', el('code', {}, [p.root_path])],
            ['snapshot', `#${p.snapshot_id}`],
        ]),
    ];
    if (p.mcp_decls && p.mcp_decls.length) {
        nodes.push(el('h3', {}, ['MCP tools exposed']));
        nodes.push(el('ul', {}, p.mcp_decls.map((d) => (
            el('li', {}, [
                el('code', {}, [d.tool_name]),
                ' — ',
                el('code', {}, [`${d.rel_path}:${d.line}`]),
            ])
        ))));
    }
    if (p.outbound && p.outbound.length) {
        nodes.push(el('h3', {}, ['Outbound']));
        nodes.push(el('ul', {}, p.outbound.map((e) => (
            el('li', {}, [
                '→ ',
                el('strong', {}, [e.target_name]),
                ' ',
                el('em', {}, [e.kind]),
            ])
        ))));
    }
    if (p.inbound && p.inbound.length) {
        nodes.push(el('h3', {}, ['Inbound']));
        nodes.push(el('ul', {}, p.inbound.map((e) => (
            el('li', {}, [
                '← ',
                el('strong', {}, [e.source_name]),
                ' ',
                el('em', {}, [e.kind]),
            ])
        ))));
    }
    if (p.recent_changes && p.recent_changes.length) {
        nodes.push(el('h3', {}, ['Recent changes']));
        nodes.push(el('ul', {}, p.recent_changes.map((c) => (
            el('li', {}, [`snapshot #${c.snapshot_id}: ${c.change}`])
        ))));
    }
    return nodes;
}

function renderContract(c) {
    const nodes = [
        el('h2', {}, [`Contract: ${c.declared_id || c.slug}`]),
        renderDl([
            ['kind', c.kind],
            ['content hash', el('code', {}, [`${c.content_hash.slice(0, 16)}…`])],
        ]),
        el('h3', {}, ['Owners']),
    ];
    nodes.push(el('ul', {}, (c.owners || []).map((o) => (
        el('li', {}, [
            el('strong', {}, [o.project_name]),
            ' — ',
            el('code', {}, [o.rel_path]),
        ])
    ))));
    return nodes;
}

function renderEdge(e) {
    const nodes = [
        el('h2', {}, [`Edge: ${e.kind}`]),
        renderDl([
            ['from', el('span', {}, [
                el('strong', {}, [e.from_name]),
                ` (${e.from_kind})`,
            ])],
            ['to', el('span', {}, [
                el('strong', {}, [e.to_name]),
                ` (${e.to_kind})`,
            ])],
            ['attrs', el('code', {}, [JSON.stringify(e.attrs)])],
        ]),
    ];
    if (e.evidence && e.evidence.length) {
        nodes.push(el('h3', {}, ['Evidence']));
        nodes.push(el('ul', {}, e.evidence.map((ev) => (
            el('li', {}, [el('code', {}, [`${ev.rel_path}:${ev.line}`])])
        ))));
    } else {
        nodes.push(el('p', {}, [el('em', {}, ['No evidence persisted for this edge kind in M7.'])]));
    }
    return nodes;
}

function renderDl(pairs) {
    const dl = el('dl', {}, []);
    for (const [label, value] of pairs) {
        dl.appendChild(el('dt', {}, [label]));
        if (typeof value === 'string') {
            dl.appendChild(el('dd', {}, [value]));
        } else {
            dl.appendChild(el('dd', {}, [value]));
        }
    }
    return dl;
}

// ─── Search ──────────────────────────────────────────────────────────────────

const searchBox = document.getElementById('search-box');
const searchResults = document.getElementById('search-results');
let searchDebounce;

searchBox.addEventListener('input', () => {
    clearTimeout(searchDebounce);
    const q = searchBox.value.trim();
    if (!q) {
        searchResults.classList.remove('visible');
        setChildren(searchResults, []);
        return;
    }
    searchDebounce = setTimeout(() => doSearch(q), 250);
});

async function doSearch(q) {
    try {
        const hits = await fetchJson(`/api/search?q=${encodeURIComponent(q)}&limit=10`);
        if (!hits.length) {
            setChildren(searchResults, [el('div', { class: 'hit' }, [el('em', {}, ['No matches'])])]);
        } else {
            const items = hits.map((h) => {
                const div = el('div', {
                    class: 'hit',
                    dataset: {
                        entityKind: h.entity_kind,
                        entityId: String(h.entity_id),
                        name: h.name,
                    },
                }, [
                    el('strong', {}, [h.name]),
                    ' ',
                    el('span', { class: 'hit-kind' }, [h.entity_kind]),
                ]);
                div.addEventListener('click', () => onSearchHitClick(div));
                return div;
            });
            setChildren(searchResults, items);
        }
        searchResults.classList.add('visible');
    } catch (e) {
        setChildren(searchResults, [el('div', { class: 'hit' }, [`Error: ${e.message}`])]);
        searchResults.classList.add('visible');
    }
}

function onSearchHitClick(div) {
    const kind = div.dataset.entityKind;
    const name = div.dataset.name;
    searchResults.classList.remove('visible');
    searchBox.value = '';
    const slug = slugify(name);
    const cyId = kind === 'project' ? `p:${slug}` : `c:${slug}`;
    const node = cy.getElementById(cyId);
    if (node.length) {
        cy.elements().unselect();
        node.select();
        cy.animate({ center: { eles: node }, zoom: 1.2 }, { duration: 400 });
        window.dispatchEvent(new CustomEvent('prograph:select', {
            detail: { type: 'node', node_kind: kind, name, id: node.id() },
        }));
    }
}

function slugify(s) {
    return String(s).split('').map((c) => /[A-Za-z0-9_-]/.test(c) ? c : '-').join('');
}

document.addEventListener('click', (e) => {
    if (!searchBox.contains(e.target) && !searchResults.contains(e.target)) {
        searchResults.classList.remove('visible');
    }
});

init().catch((e) => {
    console.error('init failed', e);
    setMessage(document.getElementById('graph'), `Failed to load graph: ${e.message}`);
});
```

- [ ] **Step 2: Commit**

```sh
git add prograph/prograph/web_static/app.js
git commit -m "prograph: M6 app.js — side panel + search + activity (XSS-safe via dom.js el())"
```

---

## Task 14: `prograph serve` CLI command + static asset tests

**Files:**
- Modify: `prograph/cli.py`
- Modify: `tests/integration/test_cli_serve.py`

- [ ] **Step 1: Add the command**

In `prograph/cli.py`, append:
```python
@app.command()
def serve(
    monorepo: Path = typer.Option(  # noqa: B008
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
    host: str = typer.Option(
        "127.0.0.1",
        "--host",
        help="Bind address. Use 0.0.0.0 to expose on all interfaces (warning printed).",
    ),
    port: int = typer.Option(7700, "--port", help="Bind port."),
) -> None:
    """Start the local web UI + REST API at http://<host>:<port>."""
    import uvicorn

    from prograph.web_app import build_app

    root = _resolve_monorepo(monorepo)
    paths = PrographPaths(monorepo_root=root)
    if not paths.is_initialized():
        err_console.print(
            f"[red]error:[/red] not initialized at {paths.prograph_dir}. "
            "Run `prograph init` first."
        )
        raise typer.Exit(code=1)
    if not paths.db_path.exists():
        err_console.print(
            f"[red]error:[/red] no snapshot at {paths.db_path}. "
            "Run `prograph index` first."
        )
        raise typer.Exit(code=1)

    if host == "0.0.0.0":
        err_console.print(
            "[yellow]warning:[/yellow] binding to 0.0.0.0 exposes the API on all "
            "network interfaces with NO authentication. Use only on trusted networks."
        )

    console.print(f"[green]prograph serve[/green] at http://{host}:{port} (monorepo: {root})")

    app_instance = build_app(root)
    uvicorn.run(app_instance, host=host, port=port, log_level="info")
```

- [ ] **Step 2: Add static asset wiring tests**

Append to `tests/integration/test_cli_serve.py`:
```python
def test_static_html_serves(client: TestClient):
    r = client.get("/")
    assert r.status_code == 200
    text = r.text
    for expected_id in ('graph', 'sidepanel', 'search-box', 'activity-list'):
        assert f'id="{expected_id}"' in text


def test_static_js_files_load(client: TestClient):
    for path in ["/static/app.js", "/static/graph.js", "/static/dom.js", "/static/styles.css"]:
        r = client.get(path)
        assert r.status_code == 200, f"{path} returned {r.status_code}"
```

- [ ] **Step 3: Run**

```sh
cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run pytest tests/integration/test_cli_serve.py -v
```
Expected: 13 passed (11 + 2 new).

- [ ] **Step 4: Manual smoke**

```sh
cd /tmp && rm -rf m6smoke && mkdir m6smoke && cd m6smoke
mkdir alpha beta
printf '[project]\nname = "alpha"\n' > alpha/pyproject.toml
printf '[project]\nname = "beta"\ndependencies = ["alpha"]\n' > beta/pyproject.toml

cd /Users/Andrei_Shtanakov/labs/all_ai_orchestrators/prograph
uv run prograph init --monorepo /tmp/m6smoke
uv run prograph index --monorepo /tmp/m6smoke

uv run prograph serve --monorepo /tmp/m6smoke --port 7777 &
SERVE_PID=$!
sleep 2
curl -s http://127.0.0.1:7777/api/health
echo ""
curl -s http://127.0.0.1:7777/api/graph | head -c 200
echo ""
kill $SERVE_PID
rm -rf /tmp/m6smoke
```

Expected: `/api/health` returns ok JSON; `/api/graph` returns nodes/edges JSON.

- [ ] **Step 5: Commit**

```sh
git add prograph/prograph/cli.py prograph/tests/integration/test_cli_serve.py
git commit -m "prograph: M6 'prograph serve' CLI + static asset tests"
```

---

## Task 15: Unit tests for static asset structure

**Files:**
- Create: `tests/unit/test_web_static.py`

Cheap regression test that catches broken HTML / JS module exports without needing a browser.

- [ ] **Step 1: Write the unit test**

`tests/unit/test_web_static.py`:
```python
"""Sanity-check the bundled static assets without booting a browser."""

from pathlib import Path

STATIC_DIR = Path(__file__).parents[2] / "prograph" / "web_static"


def test_static_dir_exists():
    assert STATIC_DIR.is_dir(), f"missing static dir at {STATIC_DIR}"


def test_index_html_references_expected_ids():
    html = (STATIC_DIR / "index.html").read_text()
    for expected_id in ("graph", "sidepanel", "search-box", "search-results", "activity-list", "snapshot-info"):
        assert f'id="{expected_id}"' in html, f"index.html missing id={expected_id}"


def test_index_html_loads_app_module():
    html = (STATIC_DIR / "index.html").read_text()
    assert "/static/app.js" in html
    assert "/static/styles.css" in html


def test_graph_js_exports_expected_functions():
    js = (STATIC_DIR / "graph.js").read_text()
    assert "export function buildCytoscape" in js
    assert "export function renderGraph" in js


def test_app_js_imports_graph_and_dom():
    js = (STATIC_DIR / "app.js").read_text()
    assert "from './graph.js'" in js
    assert "from './dom.js'" in js


def test_app_js_dispatches_select_event():
    js = (STATIC_DIR / "app.js").read_text()
    assert "addEventListener('prograph:select'" in js


def test_app_js_does_not_use_innerHTML():
    """XSS hardening: all DOM construction must go through dom.js's el() helper."""
    js = (STATIC_DIR / "app.js").read_text()
    assert ".innerHTML" not in js, (
        "app.js must not use innerHTML — use dom.js helpers instead. "
        "See M6 plan §architecture for the XSS-safe DOM construction contract."
    )


def test_graph_js_does_not_use_innerHTML():
    js = (STATIC_DIR / "graph.js").read_text()
    assert ".innerHTML" not in js


def test_dom_helper_exports_el():
    js = (STATIC_DIR / "dom.js").read_text()
    assert "export function el" in js
    assert "export function setChildren" in js
```

- [ ] **Step 2: Run + commit**

```sh
uv run pytest tests/unit/test_web_static.py -v
```
Expected: 9 passed.

Full suite:
```sh
uv run pytest -v
```
Expected: 122+ tests.

```sh
git add prograph/tests/unit/test_web_static.py
git commit -m "prograph: M6 static asset structure tests (9 unit, incl. innerHTML enforcement)"
```

---

## Task 16: README + CLAUDE.md + smoke + close

**Files:**
- Modify: `prograph/README.md`
- Modify: `prograph/CLAUDE.md`
- Modify: `tests/integration/test_smoke_real.py`
- Modify: this plan file

- [ ] **Step 1: Extend the real-monorepo smoke**

Append to `tests/integration/test_smoke_real.py`'s existing test (after M7's `build_server` smoke):
```python
    # M6: confirm `prograph serve` can boot a TestClient against the real monorepo.
    from fastapi.testclient import TestClient
    from prograph.web_app import build_app
    app_instance = build_app(real)
    with TestClient(app_instance) as web_client:
        r = web_client.get("/api/health")
        assert r.status_code == 200, r.text
        r2 = web_client.get("/api/graph")
        assert r2.status_code == 200, r2.text
        payload = r2.json()
        assert payload["n_projects"] >= 3
```

- [ ] **Step 2: Update README**

Replace the Status line:
```markdown
**Status:** M6 — Browser UI. `prograph serve` starts a local web server at `http://127.0.0.1:7700` with an interactive cytoscape.js graph view of the monorepo. Eight REST endpoints mirror M7's MCP tools: `/api/graph`, `/api/projects/by-{name,id}`, `/api/contracts/by-{id,slug}`, `/api/edges/{id}`, `/api/changelog`, `/api/search`, `/api/snapshots[/{id}]`. AI agents still use `prograph mcp` (M7); humans use `prograph serve`. All DOM construction in the frontend goes through an XSS-safe `el()` helper — no `innerHTML` anywhere.
```

Add a "Browser UI" section under Usage:
````markdown
## Browser UI

```sh
prograph serve [--port 7700] [--host 127.0.0.1]
```

Opens a local web UI:
- **Graph view** (cytoscape.js): projects as rectangles colored by language, contracts as diamonds. Edges colored by kind (gray=package_dep, teal=mcp_call, amber=contract_link).
- **Side panel**: click a node or edge to see full details — manifest, MCP tools, contract owners, evidence, recent changes.
- **Search box**: FTS query against project + contract names.
- **Activity feed**: last 10 change_log entries.

Static assets (cytoscape.js, cose-bilkent, Pico CSS) load from a CDN. Internet required at page load. Offline bundling is a later milestone.

### REST endpoints

| Endpoint | Purpose |
|---|---|
| `GET /api/health` | Liveness probe. |
| `GET /api/graph` | Full graph (nodes + edges) for the latest snapshot. |
| `GET /api/projects/by-name/{name}` | Project description by name. |
| `GET /api/projects/by-id/{id}` | Project description by id. |
| `GET /api/contracts/by-id/{id}` | Contract description by id. |
| `GET /api/contracts/by-slug/{slug}` | Contract description by slug. |
| `GET /api/edges/{edge_id}` | Edge + evidence + history. |
| `GET /api/changelog?since=&entity_kind=&limit=` | Paginated changelog. |
| `GET /api/search?q=&kinds=&limit=` | FTS search. |
| `GET /api/snapshots[?limit=]` | List of snapshots. |
| `GET /api/snapshots/{id}` | Snapshot metadata. |

No auth in M6 — bind to 127.0.0.1 (default). `--host 0.0.0.0` prints a warning.

### M6 limitations (intentional — addressed in later milestones)

- **Diff view** (`/api/graph?since=<snap>`) deferred — only the latest snapshot is rendered. M8 adds.
- **WebSocket live updates** deferred. Reload the page to pick up new snapshots.
- **No authentication** — bind to 127.0.0.1 only. M8+ may add basic auth for shared servers.
- **CDN-loaded JS** — internet required at first page load. M8 bundles offline.
- **Desktop-only layout** — mobile/responsive design is M9+.
- **No browser-level E2E tests** — REST is tested via FastAPI TestClient; the static UI is structurally validated but not exercised in a real browser.
````

- [ ] **Step 3: Update CLAUDE.md**

Update the components list to include browser UI:
```markdown
- **`prograph` (Python package):**
  - `cli.py` — `init`, `index`, `status`, `export-md`, `mcp`, **`serve`** (M6), `--version`
  - `web_app.py` — FastAPI app + 8 REST endpoints (M6)
  - `web_static/` — Static frontend with XSS-safe DOM helpers (index.html, app.js, graph.js, dom.js, styles.css; cytoscape.js via CDN) (M6)
  - `mcp_server.py` — MCP stdio server with 8 tools (M7)
  - `export/` — Markdown rendering (M5)
  - `config.py`, `models.py`, `paths.py`
```

Add to "Common commands":
```sh
uv run prograph serve [--monorepo PATH] [--host 127.0.0.1] [--port 7700]  # browser UI + REST
```

Replace "What is NOT in M7" with:
```markdown
## What is NOT in M6

- Diff view (`/api/graph?since=<snap>`) — surfaces structural changes between two snapshots; deferred to M8.
- WebSocket `/ws/changes` for live updates — page reload picks up new snapshots in M6.
- Authentication / TLS — local-dev only.
- Offline asset bundling — CDN works in M6; vendored bundle is M8.
- Browser-level E2E tests (Playwright) — M8 polish.
- Mobile / responsive design — desktop-only.

(See `docs/superpowers/plans/` for individual milestone plans.)
```

Add a sub-section about the XSS-safe DOM contract:
```markdown
### Frontend DOM safety

All static frontend code uses `prograph/web_static/dom.js`'s `el(tag, attrs, children)` helper to construct DOM. **No `innerHTML` assignments anywhere.** The unit test `tests/unit/test_web_static.py::test_app_js_does_not_use_innerHTML` enforces this. If you add UI code, route it through `el()` — pass user-controlled values as string children (auto-escaped via `createTextNode`) or as attribute values.
```

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
uv run pytest -m realmonorepo -v
```

Expected: every command exits 0. Cargo unchanged from M7. Pytest ≥122; realmonorepo 1.

- [ ] **Step 5: Check the DoD boxes**

Mark every `- [ ]` in "Definition of Done (M6)" as `- [x]` with achieved counts.

- [ ] **Step 6: Final commit**

```sh
git add prograph/README.md prograph/CLAUDE.md \
        prograph/tests/integration/test_smoke_real.py \
        prograph/docs/superpowers/plans/2026-05-26-prograph-m6-browser-ui.md
git commit -m "prograph: M6 close — docs updated, smoke exercises FastAPI TestClient, DoD checked"
```

---

## Definition of Done (M6)

- [x] `cargo test --all-targets` passes (129 tests; unchanged from M7).
- [x] `uv run pytest -v` passes (122 tests; 1 deselected).
- [x] `uv run pytest -m realmonorepo -v` passes; the real monorepo's `prograph serve` boots via FastAPI TestClient and `/api/graph` returns ≥3 projects.
- [x] All 8 REST endpoints (plus `/api/health` and the static `/` and `/static/*`) return 200 for valid requests against `monorepo_mcp` (13 integration tests).
- [x] `prograph serve` boots uvicorn against `web_app:build_app(monorepo_root)` on `127.0.0.1:7700` by default and refuses to start when `.prograph/graph.db` is missing.
- [x] `--host 0.0.0.0` emits a no-auth warning before binding.
- [x] `index.html` references the expected element ids (`graph`, `sidepanel`, `search-box`, `search-results`, `activity-list`, `snapshot-info`).
- [x] `graph.js` exports `buildCytoscape` + `renderGraph` and dispatches `prograph:select` / `prograph:deselect` CustomEvents.
- [x] `app.js` imports from `./graph.js` AND `./dom.js`, registers a `prograph:select` listener, and wires the search box with a 250ms debounce.
- [x] `dom.js` exports `el` + `setChildren` (XSS-safe via `createTextNode` for all string content).
- [x] **No `innerHTML` assignment** appears in `app.js` or `graph.js` (unit test enforces).
- [x] Cytoscape edges are styled by kind (package_dep gray, mcp_call teal, contract_link amber dashed).
- [x] CI workflow continues to pass with no changes required.
- [x] All commits follow the `prograph: M6 ...` prefix convention.

## What is NOT done in M6 (handled in subsequent milestones)

- **M8** — Diff view (`/api/graph?since=<snap>`); WebSocket live updates; offline asset bundling; Playwright/E2E browser tests; `edge_evidence` backfill for `package_dep` + `contract_link`; module-level facts; performance baselines; workspace auto-discovery.
- **M9+** — Authentication / TLS; mobile / responsive design; pluggable themes; configurable graph layouts.
