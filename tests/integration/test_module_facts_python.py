"""M9: Python module facts populate after `prograph index`."""

import shutil
import sqlite3
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_modules"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "monorepo_modules"
    shutil.copytree(FIXTURE, dst)
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def test_py_lib_has_public_symbols(indexed: Path):
    db = indexed / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    try:
        rows = conn.execute(
            """
            SELECT ps.name, ps.kind, m.rel_path
            FROM public_symbols ps
            JOIN modules m ON m.id = ps.module_id
            JOIN projects p ON p.id = m.project_id
            WHERE p.name = 'py_lib'
              AND ps.last_seen = (SELECT MAX(id) FROM snapshots)
            """
        ).fetchall()
    finally:
        conn.close()
    names = {r[0] for r in rows}
    assert "PublicAPI" in names
    assert "public_fn" in names
    assert "PUBLIC_CONST" in names
    assert "_private_helper" not in names, "private names must be filtered"


def test_py_lib_has_internal_imports(indexed: Path):
    db = indexed / ".prograph" / "graph.db"
    conn = sqlite3.connect(db)
    try:
        rows = conn.execute(
            """
            SELECT ii.target_path
            FROM internal_imports ii
            JOIN modules m ON m.id = ii.module_id
            JOIN projects p ON p.id = m.project_id
            WHERE p.name = 'py_lib'
              AND ii.last_seen = (SELECT MAX(id) FROM snapshots)
            """
        ).fetchall()
    finally:
        conn.close()
    targets = {r[0] for r in rows}
    assert "py_lib.helpers" in targets
    assert "py_lib.storage" in targets
