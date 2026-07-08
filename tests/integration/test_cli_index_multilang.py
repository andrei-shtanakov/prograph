"""End-to-end integration test against monorepo_multilang fixture (M3)."""

import json
import shutil
import sqlite3
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


def _edges(monorepo: Path) -> list[tuple[str, str, str | None]]:
    """Return (consumer_name, publisher_name, dep_name) for every package_dep edge."""
    db = monorepo / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    try:
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
    finally:
        conn.close()
    return rows


def test_multilang_index_detects_all_cross_lang_edges(fresh_multilang_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_multilang_fixture)])
    summary = _run(["index", "--monorepo", str(fresh_multilang_fixture)])

    # 8 projects: 4 py + 2 rust + 2 js
    assert summary["n_projects"] == 8, summary
    # 5 cross-project edges:
    #   1. py_consumer  -> py_publisher  (via [project].dependencies)
    #   2. py_consumer  -> py_workspace  (via py-sdk alias)
    #   3. py_dev_consumer -> py_publisher (via [dependency-groups])
    #   4. rust_consumer -> rust_publisher
    #   5. js_consumer  -> js_publisher
    assert summary["n_edges"] == 5, summary


def test_multilang_python_alias_edge(fresh_multilang_fixture: Path):
    """py_consumer declares 'py-sdk'; py_workspace aliases 'py-sdk' to itself."""
    runner.invoke(app, ["init", "--monorepo", str(fresh_multilang_fixture)])
    _run(["index", "--monorepo", str(fresh_multilang_fixture)])

    rows = _edges(fresh_multilang_fixture)
    alias_edge = [r for r in rows if r[0] == "py_consumer" and r[1] == "py_workspace"]
    assert len(alias_edge) == 1
    assert alias_edge[0][2] == "py-sdk"


def test_multilang_rust_edge(fresh_multilang_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_multilang_fixture)])
    _run(["index", "--monorepo", str(fresh_multilang_fixture)])

    rows = _edges(fresh_multilang_fixture)
    assert ("rust_consumer", "rust_publisher", "rust-publisher") in rows


def test_multilang_js_edge(fresh_multilang_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_multilang_fixture)])
    _run(["index", "--monorepo", str(fresh_multilang_fixture)])

    rows = _edges(fresh_multilang_fixture)
    assert ("js_consumer", "js_publisher", "js-publisher") in rows


def test_multilang_dependency_groups_edge(fresh_multilang_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_multilang_fixture)])
    _run(["index", "--monorepo", str(fresh_multilang_fixture)])

    rows = _edges(fresh_multilang_fixture)
    assert ("py_dev_consumer", "py_publisher", "py-publisher") in rows
