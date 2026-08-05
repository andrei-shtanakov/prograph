"""M11: find_drifts MCP tool via stdio."""

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
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_drift"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "md"
    shutil.copytree(FIXTURE, dst)
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def _server_params(monorepo: Path) -> StdioServerParameters:
    return StdioServerParameters(
        command=sys.executable,
        args=["-m", "prograph.mcp_server", str(monorepo)],
    )


def _text(content_list) -> str:
    return getattr(content_list[0], "text", "")


async def test_find_drifts_no_filter(indexed: Path):
    async with Client(stdio_client(_server_params(indexed))) as client:
        result = await client.call_tool("find_drifts", arguments={})
        payload = json.loads(_text(result.content))
        assert isinstance(payload, list)
        assert any(d["kind"] == "missing" for d in payload)
        assert any(d["kind"] == "extra" for d in payload)


async def test_find_drifts_by_project(indexed: Path):
    async with Client(stdio_client(_server_params(indexed))) as client:
        result = await client.call_tool("find_drifts", arguments={"project_name": "cleaner"})
        payload = json.loads(_text(result.content))
        assert payload == []


async def test_find_drifts_by_kind(indexed: Path):
    async with Client(stdio_client(_server_params(indexed))) as client:
        result = await client.call_tool("find_drifts", arguments={"kind": "missing"})
        payload = json.loads(_text(result.content))
        assert all(d["kind"] == "missing" for d in payload)


async def test_find_drifts_invalid_kind(indexed: Path):
    """`kind` is enum-validated against the tool's input_schema in build_server (app-side,
    v2's lowlevel Server dropped the SDK-side validation v1 had). Result is is_error=True
    with a plain-text message, NOT our JSON {"error": ...}."""
    async with Client(stdio_client(_server_params(indexed))) as client:
        result = await client.call_tool("find_drifts", arguments={"kind": "bogus"})
        assert result.is_error is True
        assert "bogus" in _text(result.content)


async def test_find_drifts_kind_stale_declaration_accepted(indexed: Path):
    """Schema accepts kind="stale_declaration"; fixture has none, so [] is valid."""
    async with Client(stdio_client(_server_params(indexed))) as client:
        result = await client.call_tool("find_drifts", arguments={"kind": "stale_declaration"})
        payload = json.loads(_text(result.content))
        assert isinstance(payload, list)
