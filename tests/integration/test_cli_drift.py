"""M11 CLI: prograph drift."""

import json
import shutil
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()
# Use any existing fixture that produces drift after index. The monorepo_drift
# fixture lands in Task 11; for now, exercise the existing monorepo_mcp which
# has projects without intent docs (zero drift expected — tests the no-drift path).
FIXTURE_MCP = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_mcp"


@pytest.fixture
def indexed_mcp(tmp_path: Path) -> Path:
    dst = tmp_path / "mcp"
    shutil.copytree(FIXTURE_MCP, dst, ignore=shutil.ignore_patterns("golden"))
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def test_drift_command_runs(indexed_mcp: Path):
    res = runner.invoke(app, ["drift", "--monorepo", str(indexed_mcp)])
    assert res.exit_code == 0, res.stdout
    # monorepo_mcp has no intent docs → drift command prints "No drift findings."
    # (Once Task 11's monorepo_drift fixture lands, additional tests cover the
    # populated case.)


def test_drift_command_json_empty(indexed_mcp: Path):
    res = runner.invoke(app, ["drift", "--monorepo", str(indexed_mcp), "--json"])
    assert res.exit_code == 0, res.stdout
    payload = json.loads(res.stdout)
    assert isinstance(payload, list)


def test_drift_command_filter_by_kind(indexed_mcp: Path):
    res = runner.invoke(app, ["drift", "--monorepo", str(indexed_mcp), "--kind", "missing"])
    assert res.exit_code == 0


def test_drift_command_no_db(tmp_path: Path):
    res = runner.invoke(app, ["drift", "--monorepo", str(tmp_path)])
    assert res.exit_code == 1
    # Combined stdout+stderr — CliRunner default mixes them; check both.
    combined = (res.stdout or "") + (res.stderr or "")
    assert "graph.db" in combined or "init" in combined
