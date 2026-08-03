"""Conformance engine: verdicts + findings over a synthetic observed graph."""

import datetime as dt

from prograph.conformance.engine import ObservedEdge, ObservedGraph

TODAY = dt.date(2026, 8, 3)


def graph(
    *edges: ObservedEdge,
    projects: tuple[str, ...] = ("alpha", "beta", "gamma"),
    project_paths: dict[str, frozenset[str]] | None = None,
) -> ObservedGraph:
    return ObservedGraph(
        projects=frozenset(projects),
        edges=edges,
        project_paths=project_paths or {},
    )


def dep(src: str, dst: str) -> ObservedEdge:
    return ObservedEdge(
        kind="package_dep", from_name=src, to_kind="project", to_name=dst, path=None, mode=None
    )


def mcp(src: str, dst: str) -> ObservedEdge:
    return ObservedEdge(
        kind="mcp_call", from_name=src, to_kind="project", to_name=dst, path=None, mode=None
    )


def contract(src: str, node: str) -> ObservedEdge:
    return ObservedEdge(
        kind="contract_link",
        from_name=src,
        to_kind="contract",
        to_name=node,
        path=None,
        mode=None,
    )


def declared(src: str, dst: str, path: str, mode: str = "read") -> ObservedEdge:
    return ObservedEdge(
        kind="declared", from_name=src, to_kind="project", to_name=dst, path=path, mode=mode
    )


def test_observed_edge_is_hashable() -> None:
    assert len({dep("a", "b"), dep("a", "b"), dep("a", "c")}) == 2
