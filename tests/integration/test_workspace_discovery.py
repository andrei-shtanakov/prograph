"""M8: discovery recurses into Cargo + Python workspaces."""

import json
import shutil
import sqlite3
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

cli_runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_workspace"


@pytest.fixture
def workspace_root(tmp_path: Path) -> Path:
    dst = tmp_path / "ws"
    shutil.copytree(FIXTURE, dst)
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    return dst


def test_discovery_finds_python_workspace_members(workspace_root: Path):
    result = cli_runner.invoke(app, ["status", "--monorepo", str(workspace_root), "--json"])
    assert result.exit_code == 0, result.stdout
    payload = json.loads(result.stdout)
    names = {p["name"] for p in payload["projects"]}
    assert "outer_python" in names
    assert "sub_a" in names
    assert "sub_b" in names


def test_discovery_finds_rust_workspace_members(workspace_root: Path):
    result = cli_runner.invoke(app, ["status", "--monorepo", str(workspace_root), "--json"])
    payload = json.loads(result.stdout)
    names = {p["name"] for p in payload["projects"]}
    assert "rust_workspace" in names
    assert "crate_x" in names
    assert "crate_y" in names


def test_index_finds_workspace_cross_deps(workspace_root: Path):
    result = cli_runner.invoke(app, ["index", "--monorepo", str(workspace_root), "--json"])
    assert result.exit_code == 0, result.stdout

    db = workspace_root / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    try:
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
    finally:
        conn.close()

    # consumer declares "outer-python-sub-a" which matches sub_a's declared name.
    found = any(
        c[0] == "consumer" and c[1] == "sub_a" and c[2] == "outer-python-sub-a" for c in edges
    )
    assert found, f"expected consumer → sub_a edge, got: {edges}"
