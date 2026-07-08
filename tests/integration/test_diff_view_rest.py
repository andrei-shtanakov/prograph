"""M8: GET /api/graph?since=<snap> tags edges with added/removed/unchanged status."""

from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from typer.testing import CliRunner

from prograph.cli import app as cli_app
from prograph.web_app import build_app

cli_runner = CliRunner()


@pytest.fixture
def two_snapshots(tmp_path: Path) -> Path:
    """Set up a monorepo with two snapshots:

    Snapshot 1: alpha → beta (package_dep), beta exists.
    Snapshot 2: alpha → charlie (added), alpha → beta (removed), charlie added,
    beta removed by removing project altogether.
    """
    dst = tmp_path / "evolving"
    dst.mkdir()
    (dst / "alpha").mkdir()
    (dst / "alpha" / "pyproject.toml").write_text(
        '[project]\nname = "alpha"\nversion = "0.1.0"\ndependencies = ["beta"]\n'
    )
    (dst / "beta").mkdir()
    (dst / "beta" / "pyproject.toml").write_text('[project]\nname = "beta"\nversion = "0.1.0"\n')

    cli_runner.invoke(cli_app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(cli_app, ["index", "--monorepo", str(dst)])  # snapshot 1

    # Modify alpha: drop the beta dep, add a charlie dep. Add charlie project.
    (dst / "alpha" / "pyproject.toml").write_text(
        '[project]\nname = "alpha"\nversion = "0.1.0"\ndependencies = ["charlie"]\n'
    )
    (dst / "charlie").mkdir()
    (dst / "charlie" / "pyproject.toml").write_text(
        '[project]\nname = "charlie"\nversion = "0.1.0"\n'
    )

    cli_runner.invoke(cli_app, ["index", "--monorepo", str(dst)])  # snapshot 2
    return dst


def test_graph_without_since_returns_alive_edges(two_snapshots: Path):
    client = TestClient(build_app(two_snapshots))
    with client:
        r = client.get("/api/graph")
    assert r.status_code == 200
    payload = r.json()
    assert payload["since"] is None
    assert all(e["status"] == "unchanged" for e in payload["edges"])
    edge_kinds = {e["kind"] for e in payload["edges"]}
    assert "package_dep" in edge_kinds


def test_graph_with_since_tags_diff(two_snapshots: Path):
    client = TestClient(build_app(two_snapshots))
    with client:
        r = client.get("/api/graph?since=1")
    assert r.status_code == 200
    payload = r.json()
    assert payload["since"] == 1
    statuses = {(e["kind"], e["status"]) for e in payload["edges"]}

    # Expect: alpha → beta removed; alpha → charlie added.
    assert ("package_dep", "added") in statuses, statuses
    assert ("package_dep", "removed") in statuses, statuses
