"""prograph MCP stdio server — exposes the snapshot graph to AI agents."""

from __future__ import annotations

import asyncio
import json
import sys
from pathlib import Path

import jsonschema
from mcp.server import Server, ServerRequestContext
from mcp.server.stdio import stdio_server
from mcp.types import (
    CallToolRequestParams,
    CallToolResult,
    ListToolsResult,
    PaginatedRequestParams,
    TextContent,
    Tool,
)

from prograph import _core
from prograph.models import (
    ChangeEvent,
    EdgeEvidenceRow,
    EdgeRow,
    MonorepoOverview,
    ProjectDescription,
    SearchHit,
    SnapshotInfo,
)
from prograph.paths import PrographPaths


def build_server(monorepo_root: Path) -> Server:
    """Construct an MCP server bound to the given monorepo's .prograph/graph.db."""
    paths = PrographPaths(monorepo_root=monorepo_root)
    db_path = str(paths.db_path)
    validators = {
        tool.name: jsonschema.Draft202012Validator(tool.input_schema)
        for tool in _tool_definitions()
    }

    async def _list_tools(
        ctx: ServerRequestContext, params: PaginatedRequestParams | None
    ) -> ListToolsResult:
        return ListToolsResult(tools=_tool_definitions())

    async def _call(ctx: ServerRequestContext, params: CallToolRequestParams) -> CallToolResult:
        args = params.arguments or {}
        validator = validators.get(params.name)
        if validator is not None:
            try:
                validator.validate(args)
            except jsonschema.ValidationError as exc:
                return CallToolResult(
                    content=[
                        TextContent(type="text", text=f"Input validation error: {exc.message}")
                    ],
                    is_error=True,
                )
        try:
            result = await _dispatch(params.name, args, db_path)
        except Exception as exc:
            err = {"error": str(exc), "tool": params.name}
            return CallToolResult(
                content=[TextContent(type="text", text=json.dumps(err))],
                is_error=False,
            )
        return CallToolResult(
            content=[TextContent(type="text", text=json.dumps(result, indent=2))],
            is_error=False,
        )

    return Server("prograph", on_list_tools=_list_tools, on_call_tool=_call)


async def _dispatch(name: str, args: dict, db_path: str) -> object:
    """Route MCP tool name → corresponding _core query, return JSON-friendly dict.

    Each branch is a 1-5 line wrapper. Tools are listed in priority order — most
    used at top.
    """
    if name == "monorepo_overview":
        raw = _core.monorepo_overview(db_path)
        if raw is None:
            return {"error": "no snapshot yet — run `prograph index` first"}
        return MonorepoOverview.from_core(raw).model_dump(mode="json")

    if name == "list_projects":
        kind_filter = args.get("kind")
        raw = _core.monorepo_overview(db_path)
        if raw is None:
            return []
        ov = MonorepoOverview.from_core(raw)
        projects = ov.projects
        if kind_filter:
            projects = [p for p in projects if p.kind == kind_filter]
        return [p.model_dump(mode="json") for p in projects]

    if name == "describe_project":
        project_name = args.get("name")
        if not project_name or not isinstance(project_name, str):
            return {"error": "missing required arg 'name'"}
        pid = _core.project_by_name(db_path, project_name)
        if pid is None:
            return {"error": f"project not found: {project_name}"}
        raw = _core.describe_project(db_path, pid)
        if raw is None:
            return {"error": f"project_id {pid} not in latest snapshot"}
        return ProjectDescription.from_core(raw).model_dump(mode="json")

    if name == "find_edges":
        rows = _core.find_edges_filtered(
            db_path,
            args.get("from"),
            args.get("to"),
            args.get("kind"),
            args.get("since"),
        )
        return [EdgeRow.from_core(r).model_dump(mode="json") for r in rows]

    if name == "edge_evidence":
        edge_id = args.get("edge_id")
        if edge_id is None or not isinstance(edge_id, int):
            return {"error": "missing required int arg 'edge_id'"}
        rows = _core.edge_evidence_for(db_path, edge_id)
        return [EdgeEvidenceRow.from_core(r).model_dump(mode="json") for r in rows]

    if name == "changelog":
        events = _core.changelog_paginated(
            db_path,
            args.get("since"),
            args.get("entity_kind"),
            args.get("limit", 50),
        )
        return [ChangeEvent.from_core(e).model_dump(mode="json") for e in events]

    if name == "search":
        query = args.get("q")
        if not query or not isinstance(query, str):
            return {"error": "missing required string arg 'q'"}
        kinds = args.get("kinds")
        if kinds is not None and not isinstance(kinds, list):
            return {"error": "'kinds' must be a list of strings"}
        limit = args.get("limit", 20)
        hits = _core.search_fts(db_path, query, kinds, limit)
        return [SearchHit.from_core(h).model_dump(mode="json") for h in hits]

    if name == "snapshot_info":
        snap_id = args.get("id")
        if snap_id is None:
            raw = _core.latest_snapshot_info(db_path)
        else:
            raw = _core.snapshot_by_id(db_path, snap_id)
        if raw is None:
            return {"error": "no snapshot found"}
        return SnapshotInfo.from_core(raw).model_dump(mode="json")

    if name == "find_symbol_references":
        project_name = args.get("project_name")
        if not project_name or not isinstance(project_name, str):
            return {"error": "missing required string arg 'project_name'"}
        symbol_name = args.get("symbol_name")
        if symbol_name is not None and not isinstance(symbol_name, str):
            return {"error": "'symbol_name' must be a string when present"}
        direction = args.get("direction", "inbound")

        from prograph.models import SymbolRefRow

        if direction == "inbound":
            rows = _core.refs_to_symbol(db_path, project_name, symbol_name)
        elif direction == "outbound":
            if symbol_name is not None:
                return {"error": "symbol_name filter not supported for direction=outbound"}
            rows = _core.refs_from_project(db_path, project_name)
        else:
            return {"error": f"invalid direction: {direction} (use 'inbound' or 'outbound')"}

        return [SymbolRefRow.from_core(r).model_dump(mode="json") for r in rows]

    if name == "find_drifts":
        project_name = args.get("project_name")
        kind = args.get("kind")
        if project_name is not None and not isinstance(project_name, str):
            return {"error": "'project_name' must be a string when present"}
        if kind is not None:
            if not isinstance(kind, str):
                return {"error": "'kind' must be a string when present"}
            if kind not in ("missing", "extra", "stale_todo", "stale_declaration"):
                return {"error": f"invalid kind: {kind}"}

        from prograph.models import DriftFinding

        if project_name:
            rows = _core.drifts_for_project(db_path, project_name)
            if kind:
                rows = [r for r in rows if r.kind == kind]
        else:
            rows = _core.find_drifts_filtered(db_path, kind)

        return [DriftFinding.from_core(r).model_dump(mode="json") for r in rows]

    raise ValueError(f"unknown tool: {name}")


def _tool_definitions() -> list[Tool]:
    """Return the MCP tool definitions exposed by the server."""
    return [
        Tool(
            name="monorepo_overview",
            description=(
                "High-level summary of the monorepo: list of projects, list of contracts, "
                "edge counts, last 10 changelog entries. The 'hello world' tool for "
                "an AI agent entering a new monorepo."
            ),
            input_schema={"type": "object", "properties": {}, "required": []},
        ),
        Tool(
            name="list_projects",
            description=(
                "List discovered projects. Optionally filter by kind. "
                "Returns minimal summaries (name, slug, kind)."
            ),
            input_schema={
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["python", "rust", "js", "docs", "mixed"],
                        "description": "Optional kind filter.",
                    }
                },
                "required": [],
            },
        ),
        Tool(
            name="describe_project",
            description=(
                "Full description of one project: manifest, MCP tools exposed, contracts "
                "declared, outbound and inbound edges, last 5 recent changes. "
                "Use after `list_projects` to drill into a specific project."
            ),
            input_schema={
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Project name as shown by `list_projects`.",
                    }
                },
                "required": ["name"],
            },
        ),
        Tool(
            name="find_edges",
            description=(
                "Query the edge graph with optional filters. All four filters are AND'ed. "
                "Returns full edge rows with from/to names and attrs."
            ),
            input_schema={
                "type": "object",
                "properties": {
                    "from": {"type": "string", "description": "Source project name."},
                    "to": {
                        "type": "string",
                        "description": "Target project name OR contract declared_id.",
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["package_dep", "mcp_call", "contract_link", "declared"],
                    },
                    "since": {
                        "type": "integer",
                        "description": "Only edges first seen at or after this snapshot id.",
                    },
                },
                "required": [],
            },
        ),
        Tool(
            name="edge_evidence",
            description=(
                "Source-line locations that justify an edge. Returns evidence rows "
                "(file:line) for all edge kinds: mcp_call (every call site), "
                "package_dep (the consumer's manifest), contract_link (the consumer's "
                "contract file paths), declared (the declarer's manifest "
                "[tool.prograph] entry)."
            ),
            input_schema={
                "type": "object",
                "properties": {
                    "edge_id": {
                        "type": "integer",
                        "description": "Edge id from `find_edges`.",
                    }
                },
                "required": ["edge_id"],
            },
        ),
        Tool(
            name="changelog",
            description=(
                "Paginated change history. Filter by `since` (snapshot id), "
                "`entity_kind` (project/edge/contract), and `limit` (default 50)."
            ),
            input_schema={
                "type": "object",
                "properties": {
                    "since": {
                        "type": "integer",
                        "description": "Only events at this snapshot id or later.",
                    },
                    "entity_kind": {
                        "type": "string",
                        "enum": ["project", "edge", "contract"],
                    },
                    "limit": {
                        "type": "integer",
                        "default": 50,
                        "minimum": 1,
                        "maximum": 500,
                    },
                },
                "required": [],
            },
        ),
        Tool(
            name="search",
            description=(
                "Full-text search over project + contract names and attributes. "
                "Returns hits with FTS snippet (matched terms wrapped in []) and BM25 rank."
            ),
            input_schema={
                "type": "object",
                "properties": {
                    "q": {"type": "string", "description": "FTS query."},
                    "kinds": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["project", "contract"],
                        },
                    },
                    "limit": {
                        "type": "integer",
                        "default": 20,
                        "minimum": 1,
                        "maximum": 100,
                    },
                },
                "required": ["q"],
            },
        ),
        Tool(
            name="snapshot_info",
            description=(
                "Metadata for a snapshot: ts, monorepo_root, git_commit, prograph_version, "
                "counts of projects/edges/changes. Defaults to the latest snapshot when "
                "`id` is omitted."
            ),
            input_schema={
                "type": "object",
                "properties": {"id": {"type": "integer", "description": "Snapshot id."}},
                "required": [],
            },
        ),
        Tool(
            name="find_symbol_references",
            description=(
                "Find cross-project source-line references. 'inbound' (default): who imports "
                "the given project's symbols (optionally filtered by symbol_name). 'outbound': "
                "which other projects this project imports from. Returns SymbolRefRow records "
                "with from_module_rel_path + line + to_module_path + to_symbol_name."
            ),
            input_schema={
                "type": "object",
                "properties": {
                    "project_name": {
                        "type": "string",
                        "description": "Target project name.",
                    },
                    "symbol_name": {
                        "type": "string",
                        "description": "Optional symbol filter (inbound only).",
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["inbound", "outbound"],
                        "default": "inbound",
                    },
                },
                "required": ["project_name"],
            },
        ),
        Tool(
            name="find_drifts",
            description=(
                "Find drift findings — discrepancies between declared intent "
                "(README/TODO/specs) and detected reality. Filter by project_name "
                "and/or kind ('missing' / 'extra' / 'stale_todo' / "
                "'stale_declaration'). Returns DriftFinding records with "
                "entity_name, source_path:line, confidence."
            ),
            input_schema={
                "type": "object",
                "properties": {
                    "project_name": {"type": "string"},
                    "kind": {
                        "type": "string",
                        "enum": ["missing", "extra", "stale_todo", "stale_declaration"],
                    },
                },
            },
        ),
    ]


async def serve(monorepo_root: Path) -> None:
    """Run the MCP stdio server until the client disconnects."""
    server = build_server(monorepo_root)
    async with stdio_server() as (read_stream, write_stream):
        await server.run(read_stream, write_stream, server.create_initialization_options())


def main(monorepo_root: Path) -> None:
    """Entry point called from the `prograph mcp` CLI command."""
    asyncio.run(serve(monorepo_root))


if __name__ == "__main__":
    if len(sys.argv) > 1:
        main(Path(sys.argv[1]).resolve())
    else:
        main(Path.cwd())
