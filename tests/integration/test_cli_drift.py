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


def test_drift_kind_stale_declaration_accepted(tmp_path: Path) -> None:
    """--kind stale_declaration filters; a fresh declared-and-deleted path shows up."""
    (tmp_path / "owner").mkdir()
    (tmp_path / "owner" / "pyproject.toml").write_text('[project]\nname="owner"\nversion="1"\n')
    (tmp_path / "reader").mkdir()
    (tmp_path / "reader" / "pyproject.toml").write_text(
        '[project]\nname="reader"\nversion="1"\n[tool.prograph]\nreads=["owner/missing.db"]\n'
    )
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    result = runner.invoke(
        app, ["drift", "--monorepo", str(tmp_path), "--kind", "stale_declaration", "--json"]
    )
    assert result.exit_code == 0, result.output
    findings = json.loads(result.stdout)
    assert any(f["entity_name"] == "owner/missing.db" for f in findings)

    text = runner.invoke(app, ["drift", "--monorepo", str(tmp_path)])
    assert "stale_declaration" in text.stdout.lower() or "stale declaration" in text.stdout.lower()
