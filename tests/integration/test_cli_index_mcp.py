"""End-to-end integration test against monorepo_mcp fixture (M4)."""

import json
import shutil
import sqlite3
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_mcp"


@pytest.fixture
def fresh_mcp_fixture(tmp_path: Path) -> Path:
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst)
    return dst


def _run(args: list[str]) -> dict:
    result = runner.invoke(app, [*args, "--json"])
    assert result.exit_code == 0, result.stdout + result.stderr
    return json.loads(result.stdout)


def _edges(db: Path) -> list[tuple[str, str, str, str]]:
    """Return (from_name, to_kind, to_name_or_id, edge_kind) for the latest snapshot."""
    conn = sqlite3.connect(db)
    try:
        rows = conn.execute(
            """
            SELECT
                (SELECT name FROM projects WHERE id = e.from_id) as from_name,
                e.to_kind,
                CASE
                    WHEN e.to_kind = 'project' THEN (SELECT name FROM projects WHERE id = e.to_id)
                    WHEN e.to_kind = 'contract' THEN (
                        SELECT COALESCE(declared_id, content_hash) FROM contracts WHERE id = e.to_id
                    )
                END as to_name,
                e.kind
            FROM edges e
            WHERE e.last_seen = (SELECT MAX(id) FROM snapshots)
            ORDER BY e.kind, from_name, to_name
            """
        ).fetchall()
    finally:
        conn.close()
    return rows


def test_mcp_index_detects_mcp_calls_and_contract_links(fresh_mcp_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_mcp_fixture)])
    summary = _run(["index", "--monorepo", str(fresh_mcp_fixture)])

    assert summary["n_projects"] == 6, summary

    rows = _edges(fresh_mcp_fixture / ".prograph" / "graph.db")
    mcp_rows = [r for r in rows if r[3] == "mcp_call"]
    contract_rows = [r for r in rows if r[3] == "contract_link"]

    assert len(mcp_rows) == 3, f"expected 3 mcp_call edges, got {mcp_rows}"
    assert len(contract_rows) == 2, f"expected 2 contract_link edges, got {contract_rows}"


def test_mcp_edge_attrs_carry_tool_name(fresh_mcp_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_mcp_fixture)])
    _run(["index", "--monorepo", str(fresh_mcp_fixture)])

    conn = sqlite3.connect(fresh_mcp_fixture / ".prograph" / "graph.db")
    rows = conn.execute(
        """
        SELECT json_extract(e.attrs_json, '$.tool')
        FROM edges e
        WHERE e.kind = 'mcp_call' AND e.last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()
    tools = {r[0] for r in rows}
    assert tools == {"decide", "evaluate"}, tools


def test_mcp_contract_node_created_with_declared_id(fresh_mcp_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_mcp_fixture)])
    _run(["index", "--monorepo", str(fresh_mcp_fixture)])

    conn = sqlite3.connect(fresh_mcp_fixture / ".prograph" / "graph.db")
    rows = conn.execute(
        """
        SELECT declared_id, kind FROM contracts
        WHERE last_seen = (SELECT MAX(id) FROM snapshots)
        """
    ).fetchall()
    conn.close()
    assert ("obs-v1", "json_schema") in rows


def test_mcp_idempotent_reindex(fresh_mcp_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_mcp_fixture)])
    _run(["index", "--monorepo", str(fresh_mcp_fixture)])
    summary = _run(["index", "--monorepo", str(fresh_mcp_fixture)])
    assert summary["n_changes"] == 0, "second index on unchanged state should produce zero changes"
