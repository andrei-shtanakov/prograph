"""Tests for `prograph init`."""

from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()


def test_init_creates_prograph_skeleton(tmp_path: Path):
    result = runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.stdout

    paths = PrographPaths(monorepo_root=tmp_path)
    assert paths.prograph_dir.is_dir()
    assert paths.config_path.is_file()
    assert paths.gitignore_path.is_file()
    assert paths.projects_md_dir.is_dir()
    assert paths.contracts_md_dir.is_dir()

    config = paths.config_path.read_text()
    assert "[monorepo]" in config
    assert "include" in config or "exclude" in config

    gi = paths.gitignore_path.read_text()
    assert "graph.db" in gi
    assert "index.lock" in gi


def test_init_is_idempotent(tmp_path: Path):
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    paths = PrographPaths(monorepo_root=tmp_path)
    config_before = paths.config_path.read_text()

    # Mutate user content; init should preserve it.
    paths.config_path.write_text(config_before + "\n# user edit\n")

    result = runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0
    assert "# user edit" in paths.config_path.read_text()


def test_init_uses_cwd_when_no_monorepo_flag(tmp_path: Path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    result = runner.invoke(app, ["init"])
    assert result.exit_code == 0
    assert (tmp_path / ".prograph" / "config.toml").is_file()


def test_init_creates_tracked_toml_template(tmp_path: Path) -> None:
    result = runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0
    tracked = tmp_path / ".prograph" / "tracked.toml"
    assert tracked.is_file()
    assert "projects = []" in tracked.read_text()


def test_init_does_not_overwrite_tracked_toml(tmp_path: Path) -> None:
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    tracked = tmp_path / ".prograph" / "tracked.toml"
    tracked.write_text('projects = ["mine"]\n')
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    assert tracked.read_text() == 'projects = ["mine"]\n'
