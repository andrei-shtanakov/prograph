"""MCP stdio server integration tests."""

import json
import shutil
import sys
from pathlib import Path

import pytest
from mcp import Client, StdioServerParameters
from mcp.client.stdio import stdio_client
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_mcp"


@pytest.fixture
def indexed_mcp_fixture(tmp_path: Path) -> Path:
    """Copy monorepo_mcp into tmp_path and run init + index."""
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def _server_params(monorepo: Path) -> StdioServerParameters:
    return StdioServerParameters(
        command=sys.executable,
        args=["-m", "prograph.mcp_server", str(monorepo)],
    )


def _text(content_list) -> str:
    """Extract .text from the first content block, narrowing past the Content union."""
    return getattr(content_list[0], "text", "")


async def test_mcp_list_tools_returns_ten(indexed_mcp_fixture: Path):
    async with Client(stdio_client(_server_params(indexed_mcp_fixture))) as client:
        tools = await client.list_tools()
        names = {t.name for t in tools.tools}
        expected = {
            "monorepo_overview",
            "list_projects",
            "describe_project",
            "find_edges",
            "edge_evidence",
            "changelog",
            "search",
            "snapshot_info",
            "find_symbol_references",
            "find_drifts",
        }
        assert expected == names, f"expected {expected}, got {names}"


async def test_mcp_monorepo_overview_returns_projects(indexed_mcp_fixture: Path):
    async with Client(stdio_client(_server_params(indexed_mcp_fixture))) as client:
        result = await client.call_tool("monorepo_overview", arguments={})
        payload = json.loads(_text(result.content))
        assert payload["n_projects"] == 6
        project_names = {p["name"] for p in payload["projects"]}
        assert "py_server" in project_names


async def test_mcp_list_projects_filter_by_kind(indexed_mcp_fixture: Path):
    async with Client(stdio_client(_server_params(indexed_mcp_fixture))) as client:
        result = await client.call_tool("list_projects", arguments={"kind": "rust"})
        payload = json.loads(_text(result.content))
        assert len(payload) == 1
        assert payload[0]["name"] == "rust_server"


async def test_mcp_describe_project(indexed_mcp_fixture: Path):
    async with Client(stdio_client(_server_params(indexed_mcp_fixture))) as client:
        result = await client.call_tool("describe_project", arguments={"name": "py_client"})
        payload = json.loads(_text(result.content))
        assert payload["name"] == "py_client"
        assert len(payload["outbound"]) >= 1


async def test_mcp_find_edges_kind_filter(indexed_mcp_fixture: Path):
    async with Client(stdio_client(_server_params(indexed_mcp_fixture))) as client:
        result = await client.call_tool("find_edges", arguments={"kind": "mcp_call"})
        payload = json.loads(_text(result.content))
        assert len(payload) == 3
        assert all(e["kind"] == "mcp_call" for e in payload)


async def test_mcp_find_edges_kind_declared_accepted(indexed_mcp_fixture: Path):
    """Schema accepts kind="declared"; fixture has no declarations, so [] is valid."""
    async with Client(stdio_client(_server_params(indexed_mcp_fixture))) as client:
        result = await client.call_tool("find_edges", arguments={"kind": "declared"})
        payload = json.loads(_text(result.content))
        assert payload == []


async def test_mcp_edge_evidence_for_mcp_call(indexed_mcp_fixture: Path):
    async with Client(stdio_client(_server_params(indexed_mcp_fixture))) as client:
        edges = json.loads(
            _text((await client.call_tool("find_edges", arguments={"kind": "mcp_call"})).content)
        )
        mcp_edge_id = edges[0]["id"]
        result = await client.call_tool("edge_evidence", arguments={"edge_id": mcp_edge_id})
        evidence = json.loads(_text(result.content))
        assert len(evidence) >= 1
        assert "rel_path" in evidence[0]
        assert "line" in evidence[0]


async def test_mcp_changelog(indexed_mcp_fixture: Path):
    async with Client(stdio_client(_server_params(indexed_mcp_fixture))) as client:
        result = await client.call_tool("changelog", arguments={"limit": 5})
        payload = json.loads(_text(result.content))
        assert isinstance(payload, list)
        assert len(payload) <= 5


async def test_mcp_search_finds_project(indexed_mcp_fixture: Path):
    async with Client(stdio_client(_server_params(indexed_mcp_fixture))) as client:
        result = await client.call_tool("search", arguments={"q": "py_server"})
        payload = json.loads(_text(result.content))
        assert any(h["name"] == "py_server" for h in payload)


async def test_mcp_snapshot_info_latest(indexed_mcp_fixture: Path):
    async with Client(stdio_client(_server_params(indexed_mcp_fixture))) as client:
        result = await client.call_tool("snapshot_info", arguments={})
        payload = json.loads(_text(result.content))
        assert payload["id"] == 1
        assert payload["n_projects"] == 6


async def test_mcp_unknown_tool_returns_error(indexed_mcp_fixture: Path):
    async with Client(stdio_client(_server_params(indexed_mcp_fixture))) as client:
        result = await client.call_tool("nonexistent_tool", arguments={})
        payload = json.loads(_text(result.content))
        assert "error" in payload
        assert "unknown tool" in payload["error"]
