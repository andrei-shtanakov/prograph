"""M12 declared edges — end-to-end: manifest declaration → edge → evidence → drift → API."""

from pathlib import Path

from fastapi.testclient import TestClient
from typer.testing import CliRunner

from prograph.cli import app
from prograph.web_app import build_app

runner = CliRunner()


def _setup(root: Path) -> None:
    (root / "proctor" / "data").mkdir(parents=True)
    (root / "proctor" / "pyproject.toml").write_text('[project]\nname="proctor"\nversion="1"\n')
    (root / "proctor" / "data" / "state.db").write_text("x")
    (root / "dispatcher").mkdir()
    (root / "dispatcher" / "pyproject.toml").write_text(
        '[project]\nname="dispatcher"\nversion="1"\n'
        "[tool.prograph]\n"
        'reads=["proctor/data/state.db", "proctor/data/vanished.db"]\n'
        'writes=["proctor/inbox/"]\n'
    )
    (root / "proctor" / "inbox").mkdir()
    runner.invoke(app, ["init", "--monorepo", str(root)])
    result = runner.invoke(app, ["index", "--monorepo", str(root), "--json"])
    assert result.exit_code == 0, result.output


def test_declared_edges_in_graph_api(tmp_path: Path) -> None:
    _setup(tmp_path)
    client = TestClient(build_app(tmp_path))
    with client:
        graph = client.get("/api/graph").json()
        declared = [e for e in graph["edges"] if e["kind"] == "declared"]
        assert len(declared) == 3  # 2 reads + 1 write, all resolving to proctor
        edge_detail = client.get(f"/api/edges/{declared[0]['edge_id']}").json()
        assert edge_detail["attrs"]["mode"] in ("read", "write")
        assert edge_detail["attrs"]["path"].startswith("proctor/")
        assert edge_detail["evidence"], "declared edge must carry manifest evidence"
        assert edge_detail["evidence"][0]["rel_path"] == "pyproject.toml"


def test_stale_declaration_in_drifts_api(tmp_path: Path) -> None:
    _setup(tmp_path)
    client = TestClient(build_app(tmp_path))
    with client:
        drifts = client.get(
            "/api/drifts", params={"project": "dispatcher", "kind": "stale_declaration"}
        ).json()
        names = {d["entity_name"] for d in drifts}
        assert "proctor/data/vanished.db" in names


def test_md_card_shows_declared_suffix(tmp_path: Path) -> None:
    _setup(tmp_path)
    result = runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.output
    card = (tmp_path / ".prograph" / "projects" / "dispatcher.md").read_text()
    assert "· read `proctor/data/state.db`" in card
