"""Tests for configurable Markdown export_root (`--out-dir` / config `[output]`)."""

from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()


def _setup(root: Path) -> None:
    (root / "alpha").mkdir()
    (root / "alpha" / "pyproject.toml").write_text("[project]\nname='alpha'\n")
    (root / "alpha" / "README.md").write_text("# alpha\n\nThe alpha project.\n")
    (root / "beta").mkdir()
    (root / "beta" / "pyproject.toml").write_text(
        "[project]\nname='beta'\ndependencies=['alpha']\n"
    )


def _set_config_export_root(config_path: Path, value: str) -> None:
    text = config_path.read_text()
    config_path.write_text(
        text.replace("auto_export = false", f'auto_export = false\nexport_root = "{value}"')
    )


def test_index_out_dir_writes_cards_to_staging(tmp_path: Path):
    _setup(tmp_path)
    stage = tmp_path / "stage"
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    result = runner.invoke(
        app,
        ["index", "--monorepo", str(tmp_path), "--export-md", "--out-dir", str(stage)],
    )
    assert result.exit_code == 0, result.stdout

    assert (stage / "projects" / "alpha.md").is_file()
    assert (stage / "contracts").is_dir()
    assert (stage / "index.md").is_file()

    # db and default .prograph md dirs are untouched.
    default = PrographPaths(monorepo_root=tmp_path)
    assert default.db_path.is_file()
    assert not (default.prograph_dir / "projects" / "alpha.md").is_file()


def test_index_config_export_root_writes_to_staging(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    default = PrographPaths(monorepo_root=tmp_path)
    _set_config_export_root(default.config_path, ".prograph/graph")

    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--export-md"])
    assert result.exit_code == 0, result.stdout

    assert (tmp_path / ".prograph" / "graph" / "projects" / "alpha.md").is_file()
    assert (tmp_path / ".prograph" / "graph" / "index.md").is_file()
    assert default.db_path.is_file()
    # Not in the plain .prograph/projects location.
    assert not (default.prograph_dir / "projects" / "alpha.md").is_file()


def test_out_dir_overrides_config(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    default = PrographPaths(monorepo_root=tmp_path)
    _set_config_export_root(default.config_path, ".prograph/from_config")

    cli_stage = tmp_path / "from_cli"
    result = runner.invoke(
        app,
        ["index", "--monorepo", str(tmp_path), "--export-md", "--out-dir", str(cli_stage)],
    )
    assert result.exit_code == 0, result.stdout

    assert (cli_stage / "projects" / "alpha.md").is_file()
    assert not (tmp_path / ".prograph" / "from_config" / "projects" / "alpha.md").is_file()


def test_export_md_standalone_respects_out_dir(tmp_path: Path):
    _setup(tmp_path)
    stage = tmp_path / "stage"
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    result = runner.invoke(app, ["export-md", "--monorepo", str(tmp_path), "--out-dir", str(stage)])
    assert result.exit_code == 0, result.stdout
    assert (stage / "projects" / "alpha.md").is_file()


def test_default_still_writes_to_prograph(tmp_path: Path):
    """Zero-regression: no --out-dir, no config → cards under .prograph/."""
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--export-md"])
    assert result.exit_code == 0, result.stdout

    default = PrographPaths(monorepo_root=tmp_path)
    assert (default.prograph_dir / "projects" / "alpha.md").is_file()
    assert (default.prograph_dir / "index.md").is_file()
