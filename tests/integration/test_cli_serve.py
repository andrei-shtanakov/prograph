"""REST API integration tests via FastAPI TestClient."""

import shutil
from pathlib import Path

import pytest
from fastapi.testclient import TestClient
from typer.testing import CliRunner

from prograph.cli import app as cli_app
from prograph.web_app import build_app

cli_runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_mcp"


@pytest.fixture
def indexed_fixture(tmp_path: Path) -> Path:
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    cli_runner.invoke(cli_app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(cli_app, ["index", "--monorepo", str(dst)])
    return dst


@pytest.fixture
def client(indexed_fixture: Path) -> TestClient:
    return TestClient(build_app(indexed_fixture))


def test_health(client: TestClient):
    r = client.get("/api/health")
    assert r.status_code == 200
    assert r.json()["status"] == "ok"


def test_root_serves_html(client: TestClient):
    r = client.get("/")
    assert r.status_code == 200
    text_lc = r.text.lower()
    assert "<h1" in text_lc or "<!doctype html>" in text_lc


def test_graph_returns_nodes_and_edges(client: TestClient):
    r = client.get("/api/graph")
    assert r.status_code == 200
    payload = r.json()
    assert payload["n_projects"] == 6
    assert payload["n_edges"] >= 5
    node_kinds = {n["node_kind"] for n in payload["nodes"]}
    assert "project" in node_kinds
    assert "contract" in node_kinds


def test_project_by_name(client: TestClient):
    r = client.get("/api/projects/by-name/py_server")
    assert r.status_code == 200
    assert r.json()["name"] == "py_server"


def test_project_by_name_404(client: TestClient):
    r = client.get("/api/projects/by-name/nonexistent")
    assert r.status_code == 404


def test_project_by_id(client: TestClient):
    by_name = client.get("/api/projects/by-name/py_server").json()
    pid = by_name["project_id"]
    r = client.get(f"/api/projects/by-id/{pid}")
    assert r.status_code == 200
    assert r.json()["name"] == "py_server"


def test_edge_with_evidence(client: TestClient):
    graph = client.get("/api/graph").json()
    mcp_edges = [e for e in graph["edges"] if e["kind"] == "mcp_call"]
    assert mcp_edges
    edge_id = mcp_edges[0]["edge_id"]
    r = client.get(f"/api/edges/{edge_id}")
    assert r.status_code == 200
    payload = r.json()
    assert payload["kind"] == "mcp_call"
    assert isinstance(payload["evidence"], list)
    assert len(payload["evidence"]) >= 1


def test_changelog_returns_list(client: TestClient):
    r = client.get("/api/changelog?limit=5")
    assert r.status_code == 200
    assert isinstance(r.json(), list)


def test_search_finds_project(client: TestClient):
    r = client.get("/api/search?q=py_server")
    assert r.status_code == 200
    payload = r.json()
    assert any(h["name"] == "py_server" for h in payload)


def test_snapshots_list_and_by_id(client: TestClient):
    r = client.get("/api/snapshots")
    assert r.status_code == 200
    snaps = r.json()
    assert len(snaps) >= 1
    sid = snaps[0]["id"]
    r2 = client.get(f"/api/snapshots/{sid}")
    assert r2.status_code == 200
    assert r2.json()["id"] == sid


def test_contract_endpoints(client: TestClient):
    graph = client.get("/api/graph").json()
    contract_nodes = [n for n in graph["nodes"] if n["node_kind"] == "contract"]
    assert contract_nodes
    slug = contract_nodes[0]["id"].removeprefix("c:")
    r = client.get(f"/api/contracts/by-slug/{slug}")
    assert r.status_code == 200
    assert "owners" in r.json()


def test_static_html_serves(client: TestClient):
    r = client.get("/")
    assert r.status_code == 200
    text = r.text
    for expected_id in ("graph", "sidepanel", "search-box", "activity-list"):
        assert f'id="{expected_id}"' in text


def test_static_js_files_load(client: TestClient):
    for path in [
        "/static/app.js",
        "/static/graph.js",
        "/static/dom.js",
        "/static/styles.css",
    ]:
        r = client.get(path)
        assert r.status_code == 200, f"{path} returned {r.status_code}"
