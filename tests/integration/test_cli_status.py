"""Tests for `prograph status`."""

import json
from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()


def _setup_mini_monorepo(root: Path) -> None:
    (root / "alpha").mkdir()
    (root / "alpha" / "pyproject.toml").write_text("[project]\nname='alpha'\n")
    (root / "beta").mkdir()
    (root / "beta" / "Cargo.toml").write_text("[package]\nname='beta'\n")
    (root / "docs_only").mkdir()
    (root / "docs_only" / "README.md").write_text("# docs only")


def test_status_lists_classified_projects(tmp_path: Path):
    _setup_mini_monorepo(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.stdout
    out = result.stdout

    assert "alpha" in out
    assert "beta" in out
    assert "docs_only" in out
    assert "python" in out
    assert "rust" in out
    assert "docs" in out


def test_status_json_output_is_valid_json(tmp_path: Path):
    _setup_mini_monorepo(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path), "--json"])
    assert result.exit_code == 0
    payload = json.loads(result.stdout)
    assert payload["monorepo_root"].endswith(str(tmp_path.resolve()).split("/")[-1])
    assert len(payload["projects"]) == 3
    names = sorted(p["name"] for p in payload["projects"])
    assert names == ["alpha", "beta", "docs_only"]


def test_status_requires_init(tmp_path: Path):
    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1
    assert "not initialized" in result.stdout.lower() or "not initialized" in result.stderr.lower()


def test_status_shows_snapshot_info_after_index(tmp_path: Path):
    _setup_mini_monorepo(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0
    assert "snapshot" in result.stdout.lower()
    assert "projects" in result.stdout.lower()


def test_status_json_includes_snapshot_when_indexed(tmp_path: Path):
    _setup_mini_monorepo(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path), "--json"])
    payload = json.loads(result.stdout)
    assert "snapshot" in payload
    assert payload["snapshot"]["n_projects"] >= 1


def test_status_json_snapshot_null_when_not_indexed(tmp_path: Path):
    _setup_mini_monorepo(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path), "--json"])
    payload = json.loads(result.stdout)
    assert payload["snapshot"] is None
