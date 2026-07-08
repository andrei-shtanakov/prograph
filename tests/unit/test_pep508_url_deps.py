"""M8: PEP 508 URL form deps parse correctly via the Python parser."""

import sqlite3
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
        "[project]\n"
        'name = "consumer"\n'
        'version = "0.1.0"\n'
        'dependencies = ["mylib @ git+https://github.com/x/mylib.git"]\n'
    )
    (dst / "mylib").mkdir()
    (dst / "mylib" / "pyproject.toml").write_text('[project]\nname = "mylib"\nversion = "0.1.0"\n')
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def test_url_dep_resolves_to_in_monorepo_publisher(url_dep_fixture: Path):
    db = url_dep_fixture / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    try:
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
    finally:
        conn.close()
    assert ("consumer", "mylib", "mylib") in rows, rows
