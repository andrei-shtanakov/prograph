"""Render snapshot data to Markdown strings.

Three pure functions:
- render_project(desc) -> str
- render_contract(desc) -> str
- render_index(overview) -> str

All output is deterministic given identical input.
"""

from __future__ import annotations

import json

from prograph.models import (
    ContractDescription,
    MonorepoOverview,
    OutboundEdge,
    ProjectDescription,
)


def render_project(
    desc: ProjectDescription,
    intro: str | None = None,
    parent: str | None = None,
    readme_body: str | None = None,
) -> str:
    """Render one project's MD file.

    `parent` (workspace-root project name) goes into frontmatter so Obsidian
    Graph view can color/group members under their workspace root.
    `readme_body` is included as a `## Description` section after the intro —
    typically only set for workspace-root projects to give the parent node
    proper context.
    """
    fm_fields: dict[str, object] = {
        "indexed_at": desc.snapshot_ts,
        "kind": desc.kind,
        "name": desc.name,
        "prograph": "project",
        "root": desc.root_path,
        "snapshot": desc.snapshot_id,
    }
    if parent:
        fm_fields["parent"] = parent
    frontmatter = _frontmatter(fm_fields)

    lines: list[str] = ["<!-- prograph:generated -->", ""]
    lines.append(frontmatter)
    lines.append("")
    lines.append(f"# {desc.name}")
    lines.append("")
    if intro:
        lines.append(f"> {intro}")
        lines.append("")
    if readme_body:
        lines.append("## Description")
        lines.append("")
        lines.append(readme_body)
        lines.append("")

    # Manifest
    lines.append("## Manifest")
    lines.append("")
    decl_name = desc.attrs.get("declared_name") or desc.attrs.get("name") or desc.name
    version = desc.attrs.get("version")
    if version:
        lines.append(f"- declared package: `{decl_name}` version `{version}`")
    else:
        lines.append(f"- declared package: `{decl_name}`")
    lines.append("")

    # Public surface
    lines.append("## Public surface")
    lines.append("")
    lines.append("### MCP tools exposed")
    lines.append("")
    if desc.mcp_decls:
        for d in desc.mcp_decls:
            lines.append(f"- `{d.tool_name}` — `{d.rel_path}:{d.line}`")
    else:
        lines.append("_None._")
    lines.append("")

    lines.append("### Contracts declared")
    lines.append("")
    if desc.contract_files:
        seen_slugs: set[str] = set()
        for cf in desc.contract_files:
            if cf.contract_slug in seen_slugs:
                continue
            seen_slugs.add(cf.contract_slug)
            display = cf.contract_declared_id or cf.contract_slug
            lines.append(
                f"- [[{cf.contract_slug}]] ({cf.contract_kind}) — `{cf.rel_path}` — `{display}`"
            )
    else:
        lines.append("_None._")
    lines.append("")

    lines.append("### Public symbols")
    lines.append("")
    if desc.public_symbols:
        for s in desc.public_symbols:
            lines.append(f"- `{s.name}` ({s.kind}) — `{s.rel_path}:{s.line}`")
    else:
        lines.append("_None._")
    lines.append("")

    # Modules summary (sub-data of projects, rendered as its own top-level section).
    lines.append("## Modules")
    lines.append("")
    if desc.modules:
        lines.append(
            f"_{len(desc.modules)} files, {len(desc.public_symbols)} public symbols, "
            f"{len(desc.internal_imports)} internal imports._"
        )
        lines.append("")
        for m in desc.modules:
            lines.append(f"- `{m.rel_path}` ({m.language})")
    else:
        lines.append("_None._")
    lines.append("")

    # M10: cross-project symbol references — inbound = "who calls my X?".
    from collections import defaultdict

    lines.append("## Inbound references")
    lines.append("")
    if desc.inbound_refs:
        grouped_in: dict[str, list] = defaultdict(list)
        for r in desc.inbound_refs:
            grouped_in[r.from_project_name].append(r)
        for project, refs in sorted(grouped_in.items()):
            lines.append(f"- from [[{project}]]:")
            for r in refs:
                sym = r.to_symbol_name or "(module)"
                target = f"{r.to_module_path}::{sym}" if r.to_module_path else sym
                lines.append(f"  - `{r.from_module_rel_path}:{r.line}` → `{target}`")
    else:
        lines.append("_None._")
    lines.append("")

    lines.append("## Outbound references")
    lines.append("")
    if desc.outbound_refs:
        grouped_out: dict[str, list] = defaultdict(list)
        for r in desc.outbound_refs:
            grouped_out[r.to_project_name].append(r)
        for project, refs in sorted(grouped_out.items()):
            lines.append(f"- to [[{project}]]:")
            for r in refs:
                sym = r.to_symbol_name or "(module)"
                target = f"{r.to_module_path}::{sym}" if r.to_module_path else sym
                lines.append(f"  - `{r.from_module_rel_path}:{r.line}` → `{target}`")
    else:
        lines.append("_None._")
    lines.append("")

    # Outbound edges
    lines.append("## Outbound edges")
    lines.append("")
    if desc.outbound:
        for e in desc.outbound:
            lines.append(_render_outbound(e))
    else:
        lines.append("_None._")
    lines.append("")

    # Inbound edges
    lines.append("## Inbound edges")
    lines.append("")
    if desc.inbound:
        for e in desc.inbound:
            lines.append(
                f"- ← [[{e.source_slug}]] · `{e.kind}`{_inbound_attr_suffix(e.attrs, e.kind)}"
            )
    else:
        lines.append("_None._")
    lines.append("")

    # Recent changes
    lines.append("## Recent changes (last 5)")
    lines.append("")
    if desc.recent_changes:
        for c in desc.recent_changes:
            lines.append(f"- snapshot {c.snapshot_id} ({c.ts}): {c.summary} ({c.change})")
    else:
        lines.append("_None._")
    lines.append("")

    # M11: drift findings (declared-vs-actual discrepancies)
    lines.append("## Drift findings")
    lines.append("")
    if not desc.drifts:
        lines.append("_None._")
        lines.append("")
    else:
        by_kind: dict[str, list] = defaultdict(list)
        for d in desc.drifts:
            by_kind[d.kind].append(d)

        for kind_label, kind_key in [
            ("Missing (declared in intent, not implemented)", "missing"),
            ("Extra (implemented, not declared in intent)", "extra"),
            ("Stale TODOs (open TODOs that look done in recent change_log)", "stale_todo"),
        ]:
            entries = by_kind.get(kind_key, [])
            if not entries:
                continue
            lines.append(f"### {kind_label}")
            lines.append("")
            for d in entries:
                conf = "[low confidence]" if d.confidence == "low" else ""
                tail = f" {conf}" if conf else ""
                lines.append(
                    f"- `{d.entity_name}` ({d.entity_kind}) — "
                    f"`{d.source_path}:{d.source_line}`{tail}"
                )
                if d.detail:
                    lines.append(f"  - {d.detail}")
            lines.append("")

    return _finalize(lines)


def render_contract(desc: ContractDescription) -> str:
    """Render one contract's MD file."""
    title = desc.declared_id or desc.slug

    frontmatter = _frontmatter(
        {
            "content_hash": desc.content_hash,
            "declared_id": desc.declared_id or "",
            "indexed_at": desc.snapshot_ts,
            "kind": desc.kind,
            "prograph": "contract",
            "snapshot": desc.snapshot_id,
        }
    )

    lines: list[str] = ["<!-- prograph:generated -->", ""]
    lines.append(frontmatter)
    lines.append("")
    lines.append(f"# Contract: {title}")
    lines.append("")
    lines.append(f"- kind: `{desc.kind}`")
    if desc.declared_id:
        lines.append(f"- declared id: `{desc.declared_id}`")
    lines.append(f"- content hash: `{desc.content_hash[:16]}…`")
    lines.append("")

    lines.append("## Owners")
    lines.append("")
    if desc.owners:
        for o in desc.owners:
            lines.append(f"- [[{o.project_slug}]] — `{o.rel_path}`")
    else:
        lines.append("_None._")
    lines.append("")

    lines.append("## Recent changes (last 5)")
    lines.append("")
    if desc.recent_changes:
        for c in desc.recent_changes:
            lines.append(f"- snapshot {c.snapshot_id} ({c.ts}): {c.summary} ({c.change})")
    else:
        lines.append("_None._")
    lines.append("")

    return _finalize(lines)


def render_index(
    overview: MonorepoOverview,
    parent_map: dict[str, str] | None = None,
) -> str:
    """Render the monorepo-level index.md.

    `parent_map` maps a workspace-member slug → its parent (workspace-root) slug.
    When provided, members are listed nested under their parent in the Projects
    section. Roots come first; orphans (no parent, not a parent of anyone) are
    rendered flat alongside roots.
    """
    frontmatter = _frontmatter(
        {
            "indexed_at": overview.snapshot_ts,
            "n_contracts": overview.n_contracts,
            "n_edges": overview.n_edges,
            "n_projects": overview.n_projects,
            "prograph": "index",
            "snapshot": overview.snapshot_id,
        }
    )

    lines: list[str] = ["<!-- prograph:generated -->", ""]
    lines.append(frontmatter)
    lines.append("")
    lines.append(f"# Monorepo: {overview.monorepo_root}")
    lines.append("")

    lines.append("## Projects")
    lines.append("")
    if overview.projects:
        pmap = parent_map or {}
        # Top-level = anything that isn't a workspace member.
        top_level = [p for p in overview.projects if p.slug not in pmap]
        # Group members by parent slug.
        members_by_parent: dict[str, list] = {}
        for p in overview.projects:
            if p.slug in pmap:
                members_by_parent.setdefault(pmap[p.slug], []).append(p)
        for p in top_level:
            lines.append(f"- [[{p.slug}]] — {p.kind}")
            for member in members_by_parent.get(p.slug, []):
                lines.append(f"  - [[{member.slug}]] — {member.kind}")
    else:
        lines.append("_None._")
    lines.append("")

    lines.append("## Contracts")
    lines.append("")
    if overview.contracts:
        for c in overview.contracts:
            display = c.declared_id or c.slug
            owners_str = f"{c.n_owners} owner" + ("s" if c.n_owners != 1 else "")
            lines.append(f"- [[{c.slug}]] — `{c.kind}` ({owners_str}) — `{display}`")
    else:
        lines.append("_None._")
    lines.append("")

    lines.append("## Recent activity (last 10)")
    lines.append("")
    if overview.recent_changes:
        for c in overview.recent_changes:
            lines.append(f"- snapshot {c.snapshot_id} ({c.ts}): {c.summary} ({c.change})")
    else:
        lines.append("_None._")
    lines.append("")

    return _finalize(lines)


def _frontmatter(fields: dict[str, object]) -> str:
    """YAML frontmatter — keys sorted alphabetically for byte stability."""
    out = ["---"]
    for k in sorted(fields.keys()):
        v = fields[k]
        if isinstance(v, str):
            # If the value contains a colon, hash, newline or starts with a YAML-special
            # char, JSON-encode it to make parsing unambiguous.
            if any(ch in v for ch in ":#\n") or v.startswith(("'", '"', "[", "{", "-")):
                v = json.dumps(v)
        out.append(f"{k}: {v}")
    out.append("---")
    return "\n".join(out)


def _render_outbound(e: OutboundEdge) -> str:
    arrow = "↔" if e.kind == "contract_link" else "→"
    suffix = ""
    if e.kind == "package_dep":
        dep_name = e.attrs.get("dep_name")
        version_req = e.attrs.get("version_req")
        if dep_name:
            if version_req:
                suffix = f" · `{dep_name}` `{version_req}`"
            else:
                suffix = f" · `{dep_name}`"
    elif e.kind == "mcp_call":
        tool = e.attrs.get("tool")
        if tool:
            suffix = f" · tool `{tool}`"
    elif e.kind == "contract_link":
        ckind = e.attrs.get("contract_kind")
        if ckind:
            suffix = f" · `{ckind}`"

    return f"- {arrow} [[{e.target_slug}]] · `{e.kind}`{suffix}"


def _inbound_attr_suffix(attrs: dict[str, object], kind: str) -> str:
    if kind == "package_dep":
        dep_name = attrs.get("dep_name")
        if dep_name:
            return f" · `{dep_name}`"
    elif kind == "mcp_call":
        tool = attrs.get("tool")
        if tool:
            return f" · tool `{tool}`"
    return ""


def _finalize(lines: list[str]) -> str:
    """Join, strip trailing whitespace per line, ensure exactly one trailing newline."""
    cleaned = [line.rstrip() for line in lines]
    text = "\n".join(cleaned).rstrip("\n") + "\n"
    return text
