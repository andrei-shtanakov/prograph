"""M10: Python cross-project symbol refs persist + are queryable."""

import shutil
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph import _core
from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_symbol_refs"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "msr"
    shutil.copytree(FIXTURE, dst)
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def test_python_inbound_refs_for_py_sdk(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    refs = _core.refs_to_symbol(db, "py_sdk", None)
    assert refs, "expected ≥1 inbound ref to py_sdk"
    names = {r.to_symbol_name for r in refs}
    assert "Client" in names
    assert "AdminClient" in names
    assert "helper" in names


def test_python_inbound_refs_filter_by_symbol(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    client_refs = _core.refs_to_symbol(db, "py_sdk", "Client")
    assert len(client_refs) == 1
    assert client_refs[0].from_project_name == "py_consumer"
    assert client_refs[0].to_module_path == "client"


def test_python_outbound_refs_for_consumer(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    refs = _core.refs_from_project(db, "py_consumer")
    targets = {(r.to_project_name, r.to_symbol_name) for r in refs}
    assert ("py_sdk", "Client") in targets
    assert ("py_sdk", "AdminClient") in targets
    assert ("py_sdk", "helper") in targets
