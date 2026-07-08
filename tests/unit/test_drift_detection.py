"""M11: Pydantic model accepts all drift kinds."""

from prograph.models import DriftFinding


def test_drift_finding_pydantic_round_trip():
    d = DriftFinding(
        project_name="p",
        kind="missing",
        entity_kind="mcp_tool",
        entity_name="x",
        source_path="r.md",
        source_line=10,
        confidence="high",
        detail="x",
    )
    payload = d.model_dump(mode="json")
    back = DriftFinding(**payload)
    assert back == d


def test_drift_finding_kind_is_string_not_enum():
    # kind is modeled as `str` (not Enum) so server-side enum changes don't break
    # Pydantic deserialisation.
    d = DriftFinding(
        project_name="p",
        kind="anything",
        entity_kind="todo",
        entity_name="x",
        source_path="r.md",
        source_line=0,
        confidence="low",
        detail=None,
    )
    assert d.kind == "anything"
