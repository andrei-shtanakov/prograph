"""Opt-in smoke: run init+status against the real all_ai_orchestrators/ dir.

This test is marked `realmonorepo` and excluded from the default pytest run.
Invoke explicitly with `uv run pytest -m realmonorepo`.

Skipped automatically if the parent monorepo is not present (e.g., in CI sandbox).
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app


def _find_real_monorepo(start: Path) -> Path | None:
    """Locate the all_ai_orchestrators/ root from the test file's location.

    Supports both layouts:
      - prograph project directly under all_ai_orchestrators/ (parents[3])
      - prograph project inside a git worktree at
        all_ai_orchestrators/.claude/worktrees/<branch>/prograph/ (parents[6])
    """
    resolved_parents = start.resolve().parents
    for n in (3, 6):
        if len(resolved_parents) <= n:
            continue
        candidate = resolved_parents[n]
        if (candidate / "Maestro").is_dir() or (candidate / "arbiter").is_dir():
            return candidate
    return None


REAL_MONOREPO = _find_real_monorepo(Path(__file__))

runner = CliRunner()


@pytest.mark.realmonorepo
@pytest.mark.skipif(
    REAL_MONOREPO is None,
    reason="real monorepo not present at expected path",
)
def test_init_status_and_index_run_on_real_monorepo(tmp_path: Path):
    real = REAL_MONOREPO
    assert real is not None  # for type checker

    init = runner.invoke(app, ["init", "--monorepo", str(real)])
    assert init.exit_code == 0, init.stdout

    status = runner.invoke(app, ["status", "--monorepo", str(real), "--json"])
    assert status.exit_code == 0, status.stdout
    payload = json.loads(status.stdout)
    names = {p["name"] for p in payload["projects"]}
    assert {"Maestro", "arbiter", "atp-platform"} & names, (
        f"expected to discover at least one known project, got: {sorted(names)}"
    )

    # Now run the indexer.
    idx = runner.invoke(app, ["index", "--monorepo", str(real), "--json"])
    assert idx.exit_code == 0, idx.stdout
    summary = json.loads(idx.stdout)
    assert summary["n_projects"] >= 3
    # M3 now reads [dependency-groups] (PEP 735) and [project.optional-dependencies],
    # so cross-deps like arbiter→spec-runner and atp-platform→spec-runner (declared in
    # their respective [dependency-groups].dev) should produce ≥1 edge.
    # The Maestro→atp-platform-sdk edge still won't fire unless the user adds
    # `[tool.prograph].aliases = ["atp-platform-sdk"]` to atp-platform/pyproject.toml.
    assert summary["n_edges"] >= 1, (
        f"expected ≥1 edge after M3 dependency-groups parsing, got summary: {summary}. "
        f"If this fails: investigate which [dependency-groups] blocks the real monorepo has."
    )

    # M4: soft-check MCP / contract edge breakdown. Detection patterns are heuristic;
    # we log if neither shows up rather than fail CI on a real-monorepo-specific gap.
    import sqlite3

    db_path = real / ".prograph" / "graph.db"
    if db_path.exists():
        conn = sqlite3.connect(db_path)
        try:
            kind_counts = dict(
                conn.execute(
                    "SELECT kind, COUNT(*) FROM edges "
                    "WHERE last_seen = (SELECT MAX(id) FROM snapshots) GROUP BY kind"
                ).fetchall()
            )
        finally:
            conn.close()
        has_mcp_or_contract = (
            kind_counts.get("mcp_call", 0) > 0 or kind_counts.get("contract_link", 0) > 0
        )
        if not has_mcp_or_contract:
            import warnings as _w

            _w.warn(
                f"M4 smoke: real monorepo has only package_dep edges. kind_counts={kind_counts}",
                stacklevel=2,
            )

    # M5: also run export-md and verify expected MD files exist.
    md = runner.invoke(app, ["export-md", "--monorepo", str(real)])
    assert md.exit_code == 0, md.stdout

    if not db_path.exists():
        return

    projects_md_dir = real / ".prograph" / "projects"
    contracts_md_dir = real / ".prograph" / "contracts"
    index_md = real / ".prograph" / "index.md"

    assert index_md.is_file(), "index.md must be written"
    assert any(projects_md_dir.glob("*.md")), "expected at least one project MD"

    # Spot-check: one of the known projects should have an MD card.
    known = {"Maestro", "arbiter", "atp-platform"}
    found = {p.stem for p in projects_md_dir.glob("*.md")}
    assert known & found, f"expected one of {known} in produced MDs, got {found}"

    # Spot-check: if any contract was detected (M4: 2 contract_link edges), there
    # should be at least one contract MD.
    conn = sqlite3.connect(db_path)
    n_contracts = conn.execute(
        "SELECT COUNT(*) FROM contracts WHERE last_seen = (SELECT MAX(id) FROM snapshots)"
    ).fetchone()[0]
    conn.close()
    if n_contracts > 0:
        assert any(contracts_md_dir.glob("*.md")), (
            f"expected >=1 contract MD given n_contracts={n_contracts}"
        )

    # M7: confirm `prograph mcp` server can boot against the real monorepo.
    # We don't run a full MCP session here (too slow + flaky in CI) — just confirm
    # the build_server call succeeds.
    from prograph.mcp_server import build_server

    server = build_server(real)
    assert server is not None

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

        # M8: diff view returns 200 even when there's only one snapshot
        # (the diff is just empty).
        r3 = web_client.get("/api/graph?since=1")
        assert r3.status_code == 200, r3.text
        diff_payload = r3.json()
        assert diff_payload["since"] == 1

    # M8: assert at least one package_dep edge now carries evidence (the M7
    # caveat is closed). Skip the assertion if the real monorepo happens to
    # have zero cross-project deps — but currently it has several.
    conn = sqlite3.connect(db_path)
    try:
        n_pkg_evidence = conn.execute(
            """
            SELECT COUNT(*) FROM edge_evidence ev
            JOIN edges e ON e.id = ev.edge_id
            WHERE e.kind = 'package_dep'
              AND ev.last_seen = (SELECT MAX(id) FROM snapshots)
            """
        ).fetchone()[0]
        n_pkg_edges = conn.execute(
            """
            SELECT COUNT(*) FROM edges
            WHERE kind = 'package_dep'
              AND last_seen = (SELECT MAX(id) FROM snapshots)
            """
        ).fetchone()[0]
    finally:
        conn.close()
    if n_pkg_edges > 0:
        assert n_pkg_evidence >= 1, (
            f"M8 should populate evidence for package_dep edges; "
            f"got {n_pkg_evidence} evidence rows for {n_pkg_edges} edges"
        )

    # M9: at least some modules + public symbols should be persisted for the
    # real monorepo (multiple Python + Rust projects with non-trivial source).
    conn = sqlite3.connect(db_path)
    try:
        n_modules = conn.execute(
            "SELECT COUNT(*) FROM modules WHERE last_seen = (SELECT MAX(id) FROM snapshots)"
        ).fetchone()[0]
        n_symbols = conn.execute(
            "SELECT COUNT(*) FROM public_symbols WHERE last_seen = (SELECT MAX(id) FROM snapshots)"
        ).fetchone()[0]
    finally:
        conn.close()
    assert n_modules >= 5, f"M9: expected ≥5 modules across the real monorepo, got {n_modules}"
    assert n_symbols >= 5, f"M9: expected ≥5 public symbols, got {n_symbols}"

    # M10: cross-project symbol refs. Soft assertion — log instead of fail if
    # zero (depends on whether the actual source files import each other).
    conn = sqlite3.connect(db_path)
    try:
        n_refs = conn.execute(
            "SELECT COUNT(*) FROM cross_project_symbol_refs "
            "WHERE last_seen = (SELECT MAX(id) FROM snapshots)"
        ).fetchone()[0]
        pairs = conn.execute(
            """
            SELECT DISTINCT p1.name, p2.name
            FROM cross_project_symbol_refs ref
            JOIN projects p1 ON p1.id = ref.from_project_id
            JOIN projects p2 ON p2.id = ref.to_project_id
            WHERE ref.last_seen = (SELECT MAX(id) FROM snapshots)
            """
        ).fetchall()
    finally:
        conn.close()

    if n_refs == 0:
        import warnings as _w

        _w.warn(
            f"M10 smoke: real monorepo has 0 cross_project_symbol_refs. "
            f"Either no in-monorepo imports exist, or the resolver missed them. "
            f"Project pairs seen: {pairs}",
            stacklevel=2,
        )
    else:
        distinct_pairs = {(a, b) for (a, b) in pairs if a != b}
        assert distinct_pairs, f"M10: expected refs between distinct projects, got: {pairs}"

    # M11: drift count is informational. Soft assertion that logs the counts.
    import sqlite3 as _sql
    import warnings as _w

    conn = _sql.connect(db_path)
    try:
        n_drifts = conn.execute(
            "SELECT COUNT(*) FROM drift_findings WHERE last_seen = (SELECT MAX(id) FROM snapshots)"
        ).fetchone()[0]
        kinds_seen = {
            row[0]
            for row in conn.execute(
                "SELECT DISTINCT kind FROM drift_findings "
                "WHERE last_seen = (SELECT MAX(id) FROM snapshots)"
            )
        }
    finally:
        conn.close()
    _w.warn(
        f"M11 smoke: real monorepo has {n_drifts} drift findings; kinds={kinds_seen}",
        stacklevel=2,
    )
