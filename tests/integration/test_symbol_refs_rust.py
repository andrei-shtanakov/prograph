"""M10: Rust cross-project symbol refs persist + are queryable."""

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


def test_rust_inbound_refs_for_sdk(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    refs = _core.refs_to_symbol(db, "rust_sdk", None)
    names = {r.to_symbol_name for r in refs}
    assert "Client" in names
    assert "build" in names


def test_rust_inbound_module_path_stripped(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    refs = _core.refs_to_symbol(db, "rust_sdk", "Client")
    assert len(refs) == 1
    # `use rust_sdk::client::Client` → to_module_path="client", symbol="Client"
    assert refs[0].to_module_path == "client"
