"""Tests for prograph.export.render — focus on shape + determinism."""

from prograph.export.render import (
    render_contract,
    render_index,
    render_project,
)
from prograph.models import (
    ContractDescription,
    ContractOwner,
    ContractSummary,
    DriftFinding,
    McpToolDeclRow,
    MonorepoOverview,
    OutboundEdge,
    ProjectDescription,
    ProjectSummary,
)


def _empty_desc(**overrides) -> ProjectDescription:
    defaults = {
        "project_id": 1,
        "name": "x",
        "slug": "x",
        "kind": "python",
        "root_path": "./x",
        "attrs": {"declared_name": "x", "version": "0.1.0"},
        "snapshot_id": 1,
        "snapshot_ts": "2026-05-26T00:00:00Z",
        "mcp_decls": [],
        "contract_files": [],
        "outbound": [],
        "inbound": [],
        "recent_changes": [],
    }
    defaults.update(overrides)
    return ProjectDescription(**defaults)


def test_render_project_minimal():
    desc = _empty_desc()
    md = render_project(desc)
    assert "# x" in md
    assert "_None._" in md
    assert md.endswith("\n")
    assert not md.endswith("\n\n")


def test_render_project_frontmatter_alphabetical():
    desc = _empty_desc()
    md = render_project(desc)
    fm_lines = md.split("---")[1].strip().split("\n")
    keys = [line.split(":")[0] for line in fm_lines]
    assert keys == sorted(keys), f"frontmatter keys must be alphabetical, got: {keys}"


def test_render_project_includes_intro_when_provided():
    desc = _empty_desc()
    md = render_project(desc, intro="A short intro.")
    assert "> A short intro." in md


def test_render_project_omits_intro_when_none():
    desc = _empty_desc()
    md = render_project(desc, intro=None)
    assert "> " not in md


def test_render_outbound_edge_package_dep_with_version():
    desc = _empty_desc(
        outbound=[
            OutboundEdge(
                kind="package_dep",
                target_kind="project",
                target_name="beta",
                target_slug="beta",
                attrs={"dep_name": "beta-sdk", "version_req": ">=2.0"},
            )
        ]
    )
    md = render_project(desc)
    assert "→ [[beta]] · `package_dep` · `beta-sdk` `>=2.0`" in md


def test_render_outbound_edge_mcp_call_includes_tool():
    desc = _empty_desc(
        outbound=[
            OutboundEdge(
                kind="mcp_call",
                target_kind="project",
                target_name="server",
                target_slug="server",
                attrs={"tool": "decide"},
            )
        ]
    )
    md = render_project(desc)
    assert "→ [[server]] · `mcp_call` · tool `decide`" in md


def test_render_outbound_edge_contract_link_uses_double_arrow():
    desc = _empty_desc(
        outbound=[
            OutboundEdge(
                kind="contract_link",
                target_kind="contract",
                target_name="obs-v1",
                target_slug="obs-v1",
                attrs={"contract_kind": "json_schema"},
            )
        ]
    )
    md = render_project(desc)
    assert "↔ [[obs-v1]] · `contract_link` · `json_schema`" in md


def test_declared_edge_suffix_shows_mode_and_path():
    desc = _empty_desc(
        outbound=[
            OutboundEdge(
                kind="declared",
                target_kind="project",
                target_name="proctor",
                target_slug="proctor",
                attrs={"mode": "read", "path": "proctor/data/state.db"},
            )
        ]
    )
    md = render_project(desc)
    assert "→ [[proctor]] · `declared` · read `proctor/data/state.db`" in md


def test_render_project_drift_section_shows_stale_declaration_group():
    desc = _empty_desc(
        drifts=[
            DriftFinding(
                project_name="x",
                kind="stale_declaration",
                entity_kind="declared_edge",
                entity_name="proctor/data/state.db",
                source_path="proctor/AGENTS.md",
                source_line=12,
                confidence="high",
                detail=None,
            )
        ],
    )
    md = render_project(desc)
    assert "### Stale declarations (declared path no longer exists)" in md
    assert "`proctor/data/state.db` (declared_edge)" in md


def test_render_project_is_deterministic():
    desc = _empty_desc(
        mcp_decls=[McpToolDeclRow(tool_name="t1", rel_path="a.py", line=1)],
        outbound=[
            OutboundEdge(
                kind="package_dep",
                target_kind="project",
                target_name="b",
                target_slug="b",
                attrs={"dep_name": "b"},
            )
        ],
    )
    assert render_project(desc) == render_project(desc)


def test_render_contract_minimal():
    desc = ContractDescription(
        contract_id=7,
        declared_id="obs-v1",
        slug="obs-v1",
        kind="json_schema",
        content_hash="a" * 64,
        snapshot_id=1,
        snapshot_ts="2026-05-26T00:00:00Z",
        owners=[
            ContractOwner(project_name="alpha", project_slug="alpha", rel_path="schemas/obs.json"),
        ],
        recent_changes=[],
    )
    md = render_contract(desc)
    assert "# Contract: obs-v1" in md
    assert "[[alpha]]" in md
    assert "`schemas/obs.json`" in md


def test_render_index_minimal():
    overview = MonorepoOverview(
        monorepo_root="/tmp/mr",
        snapshot_id=1,
        snapshot_ts="2026-05-26T00:00:00Z",
        n_projects=2,
        n_contracts=1,
        n_edges=3,
        projects=[
            ProjectSummary(name="alpha", slug="alpha", kind="python"),
            ProjectSummary(name="beta", slug="beta", kind="rust"),
        ],
        contracts=[
            ContractSummary(slug="obs-v1", declared_id="obs-v1", kind="json_schema", n_owners=2),
        ],
        recent_changes=[],
    )
    md = render_index(overview)
    assert "# Monorepo: /tmp/mr" in md
    assert "- [[alpha]] — python" in md
    assert "- [[beta]] — rust" in md
    assert "- [[obs-v1]] — `json_schema` (2 owners)" in md
