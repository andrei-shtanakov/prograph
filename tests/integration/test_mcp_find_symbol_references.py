"""M10: find_symbol_references MCP tool via stdio."""

import json
import shutil
import sys
from pathlib import Path

import pytest
from mcp import ClientSession
from mcp.client.stdio import StdioServerParameters, stdio_client
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_symbol_refs"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "msr"
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


async def test_find_symbol_references_inbound(indexed: Path):
    async with stdio_client(_server_params(indexed)) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool(
                "find_symbol_references",
                arguments={"project_name": "py_sdk", "symbol_name": "Client"},
            )
            payload = json.loads(_text(result.content))
            assert isinstance(payload, list)
            assert len(payload) == 1
            assert payload[0]["from_project_name"] == "py_consumer"
            assert payload[0]["to_symbol_name"] == "Client"


async def test_find_symbol_references_outbound(indexed: Path):
    async with stdio_client(_server_params(indexed)) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool(
                "find_symbol_references",
                arguments={"project_name": "py_consumer", "direction": "outbound"},
            )
            payload = json.loads(_text(result.content))
            target_pairs = {(r["to_project_name"], r["to_symbol_name"]) for r in payload}
            assert ("py_sdk", "Client") in target_pairs
            assert ("py_sdk", "AdminClient") in target_pairs


async def test_find_symbol_references_missing_project_arg(indexed: Path):
    """MCP framework validates inputSchema.required BEFORE dispatch — the result
    is isError=True with a plain-text "Input validation error" message, NOT a
    JSON {"error": ...} dict from our handler."""
    async with stdio_client(_server_params(indexed)) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool("find_symbol_references", arguments={})
            assert result.isError is True
            assert "project_name" in _text(result.content).lower()


async def test_find_symbol_references_invalid_direction(indexed: Path):
    """Like the missing-arg case: `direction` is enum-validated in inputSchema,
    so an unknown value is rejected by MCP before dispatch reaches the handler.
    Result is isError=True with plain-text message naming the bad value."""
    async with stdio_client(_server_params(indexed)) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()
            result = await session.call_tool(
                "find_symbol_references",
                arguments={"project_name": "py_sdk", "direction": "sideways"},
            )
            assert result.isError is True
            assert "sideways" in _text(result.content)
