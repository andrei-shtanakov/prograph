"""Round-trip tests between Rust pyclasses and pydantic mirrors."""

from prograph import (
    ChangeKind,
    Contract,
    EdgeKind,
    EntityKind,
    NodeKind,
    ProjectCandidate,
    ProjectKind,
    _core,
)


def test_kind_round_trip_via_name():
    for variant in (
        _core.ProjectKind.Python,
        _core.ProjectKind.Rust,
        _core.ProjectKind.Js,
        _core.ProjectKind.Docs,
        _core.ProjectKind.Mixed,
    ):
        assert ProjectKind.from_core(variant).value == variant.name()


def test_candidate_round_trip():
    raw = _core.ProjectCandidate(
        name="Maestro",
        root_path="./Maestro",
        kind=_core.ProjectKind.Python,
        manifests=["pyproject.toml"],
    )
    candidate = ProjectCandidate.from_core(raw)
    assert candidate.name == "Maestro"
    assert candidate.root_path == "./Maestro"
    assert candidate.kind is ProjectKind.PYTHON
    assert candidate.manifests == ["pyproject.toml"]


def test_candidate_is_frozen():
    import pytest
    from pydantic import ValidationError

    candidate = ProjectCandidate(name="X", root_path="./X", kind=ProjectKind.RUST, manifests=[])
    with pytest.raises(ValidationError):
        candidate.name = "Y"  # type: ignore[misc]


def test_edge_kind_round_trip():
    assert EdgeKind.from_core(_core.EdgeKind.PackageDep) is EdgeKind.PACKAGE_DEP


def test_node_kind_round_trip():
    assert NodeKind.from_core(_core.NodeKind.Project) is NodeKind.PROJECT
    assert NodeKind.from_core(_core.NodeKind.Contract) is NodeKind.CONTRACT


def test_change_kind_round_trip():
    for variant in (
        _core.ChangeKind.Added,
        _core.ChangeKind.Removed,
        _core.ChangeKind.AttrsChanged,
    ):
        assert ChangeKind.from_core(variant).value == variant.name()


def test_entity_kind_round_trip():
    for variant in (_core.EntityKind.Project, _core.EntityKind.Edge):
        assert EntityKind.from_core(variant).value == variant.name()


def test_edge_kind_round_trip_extended():
    """M4: EdgeKind gained McpCall and ContractLink."""
    assert EdgeKind.from_core(_core.EdgeKind.PackageDep) is EdgeKind.PACKAGE_DEP
    assert EdgeKind.from_core(_core.EdgeKind.McpCall) is EdgeKind.MCP_CALL
    assert EdgeKind.from_core(_core.EdgeKind.ContractLink) is EdgeKind.CONTRACT_LINK


def test_contract_pydantic_mirror_round_trip():
    """M4: Contract pydantic mirror constructs and round-trips its fields."""
    c = Contract(
        id=42,
        declared_id="https://example.org/schemas/obs-v1",
        content_hash="a" * 64,
        kind="json_schema",
        first_seen=1,
        last_seen=3,
    )
    assert c.kind == "json_schema"
    assert c.declared_id == "https://example.org/schemas/obs-v1"
    assert c.first_seen == 1


def test_project_description_round_trip(tmp_path):
    """M5: ProjectDescription round-trips from _core via Store::describe_project."""
    import sqlite3
    from pathlib import Path

    from prograph._core import describe_project, index_monorepo
    from prograph.models import ProjectDescription

    root = Path(tmp_path)
    (root / ".prograph").mkdir()
    (root / "alpha").mkdir()
    (root / "alpha" / "pyproject.toml").write_text("[project]\nname='alpha'\n")
    (root / "beta").mkdir()
    (root / "beta" / "pyproject.toml").write_text(
        "[project]\nname='beta'\ndependencies=['alpha']\n"
    )

    db = root / ".prograph" / "graph.db"
    index_monorepo(str(root), str(db))

    conn = sqlite3.connect(db)
    pid = conn.execute(
        "SELECT id FROM projects WHERE name = 'beta' "
        "AND last_seen = (SELECT MAX(id) FROM snapshots)"
    ).fetchone()[0]
    conn.close()

    raw = describe_project(str(db), pid)
    assert raw is not None
    desc = ProjectDescription.from_core(raw)
    assert desc.name == "beta"
    assert any(e.target_name == "alpha" for e in desc.outbound)


def test_search_hit_pydantic_shape():
    """M7: SearchHit pydantic mirror constructs cleanly (no #[new] on _core type)."""
    from prograph.models import SearchHit

    h = SearchHit(
        entity_kind="project",
        entity_id=1,
        name="Maestro",
        snippet="DAG [orchestrator]",
        rank=-1.5,
    )
    assert h.entity_kind == "project"
    assert h.rank == -1.5
