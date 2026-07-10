"""Tests for the tracked-projects allowlist: `index` filtering + `--discover` audit."""

import json as _json
from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()


def _setup(root: Path) -> None:
    """Two plain projects + one uv workspace with a nested member."""
    (root / "tracked_proj").mkdir()
    (root / "tracked_proj" / "pyproject.toml").write_text(
        '[project]\nname="tracked_proj"\nversion="1.0"\ndependencies=[]\n'
    )
    (root / "loose_proj").mkdir()
    (root / "loose_proj" / "pyproject.toml").write_text(
        '[project]\nname="loose_proj"\nversion="1.0"\ndependencies=[]\n'
    )
    ws = root / "ws_root"
    (ws / "member_a").mkdir(parents=True)
    (ws / "pyproject.toml").write_text(
        '[project]\nname="ws_root"\nversion="1.0"\ndependencies=[]\n'
        '[tool.uv.workspace]\nmembers=["member_a"]\n'
    )
    (ws / "member_a" / "pyproject.toml").write_text(
        '[project]\nname="member_a"\nversion="1.0"\ndependencies=[]\n'
    )


def _init_with_allowlist(root: Path, toml_body: str) -> PrographPaths:
    runner.invoke(app, ["init", "--monorepo", str(root)])
    paths = PrographPaths(monorepo_root=root)
    paths.tracked_path.write_text(toml_body)
    return paths


def test_index_filters_to_allowlist_closure(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, 'projects = ["tracked_proj", "ws_root"]\n')
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--json"])
    assert result.exit_code == 0, result.output
    summary = _json.loads(result.stdout)
    # tracked_proj + ws_root + member_a (workspace member of a tracked root)
    assert summary["n_projects"] == 3


def test_index_without_tracked_toml_indexes_all(tmp_path: Path) -> None:
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    paths = PrographPaths(monorepo_root=tmp_path)
    if paths.tracked_path.exists():  # init writes an empty template — empty means all
        assert "projects = []" in paths.tracked_path.read_text()
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--json"])
    assert result.exit_code == 0, result.output
    assert _json.loads(result.stdout)["n_projects"] == 4  # all incl. member_a


def test_index_malformed_tracked_toml_exits_1(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, "projects = [broken\n")
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1
    assert "tracked.toml" in (result.stdout + result.stderr)


def test_index_discover_json_embeds_audit(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, 'projects = ["tracked_proj", "ghost"]\n')
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--json", "--discover"])
    assert result.exit_code == 0, result.output
    payload = _json.loads(result.stdout)  # stdout must be pure JSON
    audit = payload["discover"]
    untracked_names = {e["name"] for e in audit["untracked"]}
    assert untracked_names == {"loose_proj", "ws_root", "member_a"}
    assert all({"name", "root_path", "kind"} <= set(e) for e in audit["untracked"])
    assert audit["missing"] == ["ghost"]


def test_index_discover_text_goes_to_stderr(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, 'projects = ["tracked_proj", "ghost"]\n')
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--discover"])
    assert result.exit_code == 0, result.output
    assert "loose_proj" in result.stderr
    assert "ghost" in result.stderr
    assert "loose_proj" not in result.stdout


def test_status_json_annotates_tracked(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, 'projects = ["tracked_proj"]\n')
    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path), "--json"])
    assert result.exit_code == 0, result.output
    payload = _json.loads(result.stdout)
    by_name = {p["name"]: p["tracked"] for p in payload["projects"]}
    assert by_name["tracked_proj"] is True
    assert by_name["loose_proj"] is False


def test_status_without_allowlist_all_tracked(tmp_path: Path) -> None:
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path), "--json"])
    payload = _json.loads(result.stdout)
    assert all(p["tracked"] for p in payload["projects"])


def test_status_malformed_tracked_toml_exits_1(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, "projects = [broken\n")
    result = runner.invoke(app, ["status", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1


def test_serve_malformed_tracked_toml_exits_1(tmp_path: Path) -> None:
    _setup(tmp_path)
    _init_with_allowlist(tmp_path, "projects = [broken\n")
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])  # exits 1, no db — fine
    result = runner.invoke(app, ["serve", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1
    assert "tracked.toml" in (result.stdout + result.stderr)


def test_serve_logs_audit_before_start(tmp_path: Path, monkeypatch) -> None:
    _setup(tmp_path)
    paths = _init_with_allowlist(tmp_path, 'projects = ["tracked_proj", "ghost"]\n')
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0
    assert paths.db_path.is_file()

    import uvicorn

    monkeypatch.setattr(uvicorn, "run", lambda *a, **k: None)
    result = runner.invoke(app, ["serve", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.output
    assert "loose_proj" in result.stderr
    assert "ghost" in result.stderr
