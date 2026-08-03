"""Conformance engine: three-valued verdicts over the observed edge store (spec D2-D5)."""

from __future__ import annotations

import json
from collections.abc import Mapping
from dataclasses import dataclass

from prograph import _core

# Spec D4: closed verdict set.
VERDICT_CONFORMANT = "conformant"
VERDICT_VIOLATION = "violation"
VERDICT_UNKNOWN = "unknown"

# Spec D4: machine-readable unknown reasons.
REASON_MANUAL = "manual-evidence"
REASON_UNSUPPORTED = "unsupported-resolution"
REASON_OUTSIDE = "outside-workspace"
REASON_ORPHAN = "orphan-component"

# Spec D5: finding taxonomy v1 — exact identifiers, also the --fail-on vocabulary (D7).
FINDING_CLASSES = (
    "missing-required-edge",
    "forbidden-edge",
    "undeclared-edge",
    "orphan-component",
    "expired-waiver",
    "manual-obligation",
)

# Spec D3: detector vocabulary maps onto existing EdgeKind strings.
DETECTOR_TO_KIND = {
    "import": "package_dep",
    "mcp": "mcp_call",
    "contract": "contract_link",
    "declared": "declared",
}


@dataclass(frozen=True)
class ObservedEdge:
    """One edge from the descriptive plane, flattened for matching."""

    kind: str
    from_name: str
    to_kind: str
    to_name: str
    path: str | None
    mode: str | None


@dataclass(frozen=True)
class ObservedGraph:
    """Everything the engine needs from a snapshot — data only, no DB handle."""

    projects: frozenset[str]
    edges: tuple[ObservedEdge, ...]
    project_paths: Mapping[str, frozenset[str]]


def load_observed(db_path: str) -> ObservedGraph | None:
    """Flatten the latest snapshot into an ObservedGraph. None when no snapshot."""
    overview = _core.monorepo_overview(db_path)
    if overview is None:
        return None
    names = [p.name for p in overview.projects]

    edges: list[ObservedEdge] = []
    for row in _core.find_edges_filtered(db_path):
        path: str | None = None
        mode: str | None = None
        if row.kind == "declared":
            try:
                attrs = json.loads(row.attrs_json)
            except json.JSONDecodeError:
                attrs = {}
            raw_path = attrs.get("path")
            raw_mode = attrs.get("mode")
            path = raw_path if isinstance(raw_path, str) else None
            mode = raw_mode if isinstance(raw_mode, str) else None
        edges.append(
            ObservedEdge(
                kind=row.kind,
                from_name=row.from_name,
                to_kind=row.to_kind,
                to_name=row.to_name,
                path=path,
                mode=mode,
            )
        )

    project_paths: dict[str, frozenset[str]] = {}
    for name in names:
        pid = _core.project_by_name(db_path, name)
        desc = _core.describe_project(db_path, pid) if pid is not None else None
        if desc is None:
            project_paths[name] = frozenset()
            continue
        paths = {m.rel_path for m in desc.modules}
        paths.update(c.rel_path for c in desc.contract_files)
        project_paths[name] = frozenset(paths)

    return ObservedGraph(projects=frozenset(names), edges=tuple(edges), project_paths=project_paths)
