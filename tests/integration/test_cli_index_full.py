"""End-to-end integration test against monorepo_full fixture."""

import json
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()

FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_full"


@pytest.fixture
def fresh_full_fixture(tmp_path: Path) -> Path:
    """Copy monorepo_full into tmp_path so tests don't pollute the repo's fixture dir."""
    import shutil

    dst = tmp_path / "monorepo_full"
    shutil.copytree(FIXTURE, dst)
    return dst


def _run(args: list[str]) -> dict:
    result = runner.invoke(app, [*args, "--json"])
    assert result.exit_code == 0, result.stdout + result.stderr
    return json.loads(result.stdout)


def test_full_index_detects_all_three_cross_deps(fresh_full_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_full_fixture)])
    summary = _run(["index", "--monorepo", str(fresh_full_fixture)])
    assert summary["n_projects"] == 5  # 4 python + 1 docs
    # 3 cross-edges: orchestrator→eval_sdk, orchestrator→policy, runner→orchestrator
    assert summary["n_edges"] == 3
    assert summary["n_changes"] >= 8  # 5 project-added + 3 edge-added


def test_full_reindex_idempotent(fresh_full_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_full_fixture)])
    _run(["index", "--monorepo", str(fresh_full_fixture)])
    summary = _run(["index", "--monorepo", str(fresh_full_fixture)])
    assert summary["n_changes"] == 0


def test_full_version_bump_produces_attrs_changed(fresh_full_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_full_fixture)])
    _run(["index", "--monorepo", str(fresh_full_fixture)])

    # Bump orchestrator's eval-sdk requirement from >=1.0 to >=2.0
    orch_toml = fresh_full_fixture / "orchestrator" / "pyproject.toml"
    text = orch_toml.read_text().replace('"eval-sdk>=1.0"', '"eval-sdk>=2.0"')
    orch_toml.write_text(text)

    summary = _run(["index", "--monorepo", str(fresh_full_fixture)])
    assert summary["n_changes"] >= 1  # the orchestrator→eval-sdk edge attrs_changed


def test_full_status_after_index_includes_snapshot(fresh_full_fixture: Path):
    runner.invoke(app, ["init", "--monorepo", str(fresh_full_fixture)])
    _run(["index", "--monorepo", str(fresh_full_fixture)])

    status = _run(["status", "--monorepo", str(fresh_full_fixture)])
    snap = status["snapshot"]
    assert snap is not None
    assert snap["n_projects"] == 5
    assert snap["n_edges"] == 3
