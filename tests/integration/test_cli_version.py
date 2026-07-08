"""CLI smoke: `prograph --version` works via the typer runner."""

from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()


def test_version_flag_prints_versions_and_exits_zero():
    result = runner.invoke(app, ["--version"])
    assert result.exit_code == 0
    assert "prograph 0.1.0" in result.stdout
    assert "core 0.1.0" in result.stdout


def test_no_args_shows_help():
    result = runner.invoke(app, [])
    # typer's no_args_is_help exits with code 0 and prints help on stderr/stdout
    assert "Cross-project structure mapper" in result.stdout
