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
            raise RuntimeError(f"no snapshot at {paths.db_path}. Run `prograph index` first.")
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

    @app.get("/api/graph")
    async def graph(since: int | None = None) -> dict:
        from prograph import _core
        from prograph.export.slug import slugify
        from prograph.models import DiffEdgeRow, MonorepoOverview

        raw = _core.monorepo_overview(app.state.db_path)
        if raw is None:
            return {"nodes": [], "edges": [], "snapshot_id": None, "since": since}
        ov = MonorepoOverview.from_core(raw)

        nodes: list[dict] = []
        for p in ov.projects:
            nodes.append(
                {
                    "id": f"p:{p.slug}",
                    "node_kind": "project",
                    "name": p.name,
                    "kind": p.kind,
                    "label": p.name,
                }
            )
        for c in ov.contracts:
            nodes.append(
                {
                    "id": f"c:{c.slug}",
                    "node_kind": "contract",
                    "name": c.declared_id or c.slug,
                    "kind": c.kind,
                    "label": c.declared_id or c.slug,
                    "n_owners": c.n_owners,
                }
            )

        edges_out: list[dict] = []
        if since is None:
            # All alive edges, uniformly tagged "unchanged" for downstream UI.
            raw_edges = _core.find_edges_filtered(app.state.db_path, None, None, None, None)
            for e in raw_edges:
                from_id = f"{'p' if e.from_kind == 'project' else 'c'}:{slugify(e.from_name)}"
                to_id = f"{'p' if e.to_kind == 'project' else 'c'}:{slugify(e.to_name)}"
                edges_out.append(
                    {
                        "id": f"e:{e.id}",
                        "source": from_id,
                        "target": to_id,
                        "kind": e.kind,
                        "edge_id": e.id,
                        "status": "unchanged",
                    }
                )
        else:
            raw_diff = _core.find_edges_with_status_since(app.state.db_path, since)
            for raw_edge in raw_diff:
                d = DiffEdgeRow.from_core(raw_edge)
                from_id = f"{'p' if d.from_kind == 'project' else 'c'}:{slugify(d.from_name)}"
                to_id = f"{'p' if d.to_kind == 'project' else 'c'}:{slugify(d.to_name)}"
                edges_out.append(
                    {
                        "id": f"e:{d.id}",
                        "source": from_id,
                        "target": to_id,
                        "kind": d.kind,
                        "edge_id": d.id,
                        "status": d.status,
                    }
                )

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
            raise HTTPException(
                status_code=404,
                detail=f"project_id {project_id} not in latest snapshot",
            )
        return ProjectDescription.from_core(raw).model_dump(mode="json")

    @app.get("/api/contracts/by-id/{contract_id}")
    async def contract_by_id(contract_id: int) -> dict:
        from prograph import _core
        from prograph.models import ContractDescription

        raw = _core.describe_contract(app.state.db_path, contract_id)
        if raw is None:
            raise HTTPException(
                status_code=404,
                detail=f"contract_id {contract_id} not in latest snapshot",
            )
        return ContractDescription.from_core(raw).model_dump(mode="json")

    @app.get("/api/contracts/by-slug/{slug}")
    async def contract_by_slug(slug: str) -> dict:
        import sqlite3

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
        target = matched[0]
        conn = sqlite3.connect(app.state.db_path)
        try:
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
            raise HTTPException(
                status_code=404,
                detail=f"contract row not found for slug {slug}",
            )

        raw = _core.describe_contract(app.state.db_path, row[0])
        if raw is None:
            raise HTTPException(
                status_code=404,
                detail=f"contract_id {row[0]} not in latest snapshot",
            )
        return ContractDescription.from_core(raw).model_dump(mode="json")

    @app.get("/api/edges/{edge_id}")
    async def edge_by_id(edge_id: int) -> dict:
        from prograph import _core
        from prograph.models import EdgeEvidenceRow, EdgeRow

        all_edges = _core.find_edges_filtered(app.state.db_path, None, None, None, None)
        match = next((e for e in all_edges if e.id == edge_id), None)
        if match is None:
            raise HTTPException(
                status_code=404,
                detail=f"edge_id {edge_id} not in latest snapshot",
            )

        edge_dict = EdgeRow.from_core(match).model_dump(mode="json")
        ev_rows = _core.edge_evidence_for(app.state.db_path, edge_id)
        edge_dict["evidence"] = [
            EdgeEvidenceRow.from_core(r).model_dump(mode="json") for r in ev_rows
        ]
        return edge_dict

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

    @app.get("/api/search")
    async def search(q: str, kinds: str | None = None, limit: int = 20) -> list[dict]:
        """Full-text search. `kinds` is a comma-separated list."""
        from prograph import _core
        from prograph.models import SearchHit

        kind_list = [k.strip() for k in kinds.split(",")] if kinds else None
        hits = _core.search_fts(app.state.db_path, q, kind_list, limit)
        return [SearchHit.from_core(h).model_dump(mode="json") for h in hits]

    @app.get("/api/snapshots")
    async def list_snapshots(limit: int = 50) -> list[dict]:
        import sqlite3

        from prograph import _core
        from prograph.models import SnapshotInfo

        # Quick direct read for snapshot ids. _core has no list helper.
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
                raise HTTPException(
                    status_code=400,
                    detail="symbol filter not supported for direction=outbound",
                )
            rows = _core.refs_from_project(app.state.db_path, project)
        else:
            raise HTTPException(status_code=400, detail=f"invalid direction: {direction}")

        return [SymbolRefRow.from_core(r).model_dump(mode="json") for r in rows]

    @app.get("/api/drifts")
    async def drifts(
        project: str | None = None,
        kind: str | None = None,
    ) -> list[dict]:
        from prograph import _core
        from prograph.models import DriftFinding

        if project:
            rows = _core.drifts_for_project(app.state.db_path, project)
            if kind:
                rows = [r for r in rows if r.kind == kind]
        else:
            rows = _core.find_drifts_filtered(app.state.db_path, kind)
        return [DriftFinding.from_core(r).model_dump(mode="json") for r in rows]

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
