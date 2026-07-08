"""Tests for `prograph index`."""

from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()


def _setup(root: Path) -> None:
    (root / "consumer").mkdir()
    (root / "consumer" / "pyproject.toml").write_text(
        '[project]\nname="consumer"\nversion="1.0"\ndependencies=["sdk"]\n'
    )
    (root / "sdk").mkdir()
    (root / "sdk" / "pyproject.toml").write_text(
        '[project]\nname="sdk"\nversion="0.1"\ndependencies=[]\n'
    )


def test_index_requires_init(tmp_path: Path):
    _setup(tmp_path)
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1
    assert "not initialized" in (result.stdout + result.stderr).lower()


def test_index_writes_snapshot_with_one_edge(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.stdout
    assert "snapshot" in result.stdout.lower() or "index" in result.stdout.lower()

    paths = PrographPaths(monorepo_root=tmp_path)
    assert paths.db_path.is_file()

    # Use the Rust helper directly to verify state.
    from prograph._core import index_monorepo

    summary = index_monorepo(str(tmp_path), str(paths.db_path))
    assert summary.n_changes == 0, "second index on same state should produce no changes"


def test_index_fails_when_another_holds_lock(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    # Acquire the lock by hand.
    paths = PrographPaths(monorepo_root=tmp_path)
    paths.lock_path.parent.mkdir(parents=True, exist_ok=True)
    import fcntl

    f = open(paths.lock_path, "w")
    try:
        fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
        assert result.exit_code == 1
        assert "lock" in (result.stdout + result.stderr).lower()
    finally:
        fcntl.flock(f, fcntl.LOCK_UN)
        f.close()


def test_index_json_output(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--json"])
    assert result.exit_code == 0
    import json

    payload = json.loads(result.stdout)
    assert payload["n_projects"] == 2
    assert payload["n_edges"] == 1
    assert payload["snapshot_id"] >= 1
