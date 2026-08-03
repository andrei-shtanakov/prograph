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


from prograph.conformance.engine import evaluate  # noqa: E402  (single import point)
from prograph.conformance.manifest import IntendedManifest  # noqa: E402


def manifest(**overrides: object) -> IntendedManifest:
    base: dict[str, object] = {
        "schema": "intended-graph/v1",
        "system": "t",
        "components": [
            {
                "id": "alpha.api",
                "project": "alpha",
                "kind": "service",
                "owner": "architects",
                "responsibility": "api",
            },
            {
                "id": "alpha.worker",
                "project": "alpha",
                "kind": "module",
                "owner": "architects",
                "responsibility": "worker",
            },
            {
                "id": "beta.lib",
                "project": "beta",
                "kind": "module",
                "owner": "architects",
                "responsibility": "lib",
            },
            {
                "id": "gamma.reader",
                "project": "gamma",
                "kind": "cli",
                "owner": "architects",
                "responsibility": "reader",
            },
        ],
    }
    base.update(overrides)
    return IntendedManifest.model_validate(base)


def _by_id(report, element_id):  # type: ignore[no-untyped-def]  # test helper
    return next(e for e in report.elements if e.id == element_id)


def iface(id_: str, producer: str, consumer: str, detector: str) -> dict[str, str]:
    return {"id": id_, "producer": producer, "consumer": consumer, "detector": detector}


def test_import_interface_conformant() -> None:
    m = manifest(interfaces=[iface("I-01", "beta.lib", "alpha.api", "import")])
    report = evaluate(m, graph(dep("alpha", "beta")), TODAY)
    el = _by_id(report, "I-01")
    assert (el.verdict, el.reason) == ("conformant", None)
    assert report.findings == ()


def test_import_interface_missing_edge() -> None:
    m = manifest(interfaces=[iface("I-01", "beta.lib", "alpha.api", "import")])
    report = evaluate(m, graph(), TODAY)
    el = _by_id(report, "I-01")
    assert (el.verdict, el.reason) == ("unknown", None)
    assert [f.finding_class for f in report.findings] == ["missing-required-edge"]
    assert report.findings[0].element == "I-01"


def test_mcp_interface_conformant() -> None:
    m = manifest(interfaces=[iface("I-01", "alpha.api", "gamma.reader", "mcp")])
    report = evaluate(m, graph(mcp("gamma", "alpha")), TODAY)
    assert _by_id(report, "I-01").verdict == "conformant"


def test_contract_interface_needs_shared_node() -> None:
    m = manifest(interfaces=[iface("I-01", "beta.lib", "gamma.reader", "contract")])
    both = graph(contract("beta", "feed-v1"), contract("gamma", "feed-v1"))
    only_one = graph(contract("beta", "feed-v1"), contract("gamma", "other-v1"))
    assert _by_id(evaluate(m, both, TODAY), "I-01").verdict == "conformant"
    assert _by_id(evaluate(m, only_one, TODAY), "I-01").verdict == "unknown"


def test_declared_interface_file_producer() -> None:
    # gamma.reader consumes a file published by beta → gamma declares the read.
    m = manifest(interfaces=[iface("I-02", "file:beta/data/feed.txt", "gamma.reader", "declared")])
    ok = graph(declared("gamma", "beta", "beta/data/feed.txt", "read"))
    wrong_mode = graph(declared("gamma", "beta", "beta/data/feed.txt", "write"))
    assert _by_id(evaluate(m, ok, TODAY), "I-02").verdict == "conformant"
    assert _by_id(evaluate(m, wrong_mode, TODAY), "I-02").verdict == "unknown"


def test_declared_file_path_segment_suffix_matches() -> None:
    # Manifest writes a repo-generic path; the edge carries the workspace-relative one.
    m = manifest(interfaces=[iface("I-02", "file:.steward/gv.jsonl", "gamma.reader", "declared")])
    ok = graph(declared("gamma", "beta", "beta/.steward/gv.jsonl", "read"))
    assert _by_id(evaluate(m, ok, TODAY), "I-02").verdict == "conformant"


def test_declared_interface_file_consumer_requires_write() -> None:
    m = manifest(interfaces=[iface("I-01", "gamma.reader", "file:beta/out.txt", "declared")])
    ok = graph(declared("gamma", "beta", "beta/out.txt", "write"))
    assert _by_id(evaluate(m, ok, TODAY), "I-01").verdict == "conformant"


def test_same_project_pair_is_unsupported_resolution() -> None:
    m = manifest(interfaces=[iface("I-04", "alpha.api", "alpha.worker", "import")])
    report = evaluate(m, graph(dep("alpha", "alpha")), TODAY)
    el = _by_id(report, "I-04")
    assert (el.verdict, el.reason) == ("unknown", "unsupported-resolution")
    assert report.findings == ()


def test_manual_evidence_interface_is_manual_obligation() -> None:
    m = manifest(interfaces=[iface("I-09", "alpha.api", "gamma.reader", "manual-evidence")])
    report = evaluate(m, graph(), TODAY)
    el = _by_id(report, "I-09")
    assert (el.verdict, el.reason) == ("unknown", "manual-evidence")
    assert [f.finding_class for f in report.findings] == ["manual-obligation"]


def test_project_outside_workspace() -> None:
    m = manifest(
        components=[
            {
                "id": "delta.ghost",
                "project": "delta",
                "kind": "service",
                "owner": "architects",
                "responsibility": "ghost",
            },
            {
                "id": "beta.lib",
                "project": "beta",
                "kind": "module",
                "owner": "architects",
                "responsibility": "lib",
            },
        ],
        interfaces=[iface("I-06", "beta.lib", "delta.ghost", "import")],
    )
    report = evaluate(m, graph(), TODAY)
    el = _by_id(report, "I-06")
    assert (el.verdict, el.reason) == ("unknown", "outside-workspace")
    assert [f.finding_class for f in report.findings] == ["orphan-component"]


def test_scope_matching_nothing_is_orphan() -> None:
    m = manifest(
        components=[
            {
                "id": "beta.ghost",
                "project": "beta",
                "kind": "module",
                "owner": "architects",
                "responsibility": "ghost",
                "scope": "no/such/dir",
            },
            {
                "id": "gamma.reader",
                "project": "gamma",
                "kind": "cli",
                "owner": "architects",
                "responsibility": "reader",
            },
        ],
        interfaces=[iface("I-07", "beta.ghost", "gamma.reader", "import")],
    )
    paths = {"beta": frozenset({"beta/__init__.py"})}
    report = evaluate(m, graph(project_paths=paths), TODAY)
    el = _by_id(report, "I-07")
    assert (el.verdict, el.reason) == ("unknown", "orphan-component")
    assert [f.finding_class for f in report.findings] == ["orphan-component"]


def test_scope_check_skipped_when_no_paths_known() -> None:
    # No module/contract facts for beta → cannot falsify the scope → not an orphan.
    m = manifest(
        components=[
            {
                "id": "beta.ghost",
                "project": "beta",
                "kind": "module",
                "owner": "architects",
                "responsibility": "ghost",
                "scope": "no/such/dir",
            },
            {
                "id": "gamma.reader",
                "project": "gamma",
                "kind": "cli",
                "owner": "architects",
                "responsibility": "reader",
            },
        ],
        interfaces=[iface("I-07", "beta.ghost", "gamma.reader", "import")],
    )
    report = evaluate(m, graph(dep("gamma", "beta")), TODAY)
    assert _by_id(report, "I-07").verdict == "conformant"


def constraint(id_: str, rule: str, detector: str) -> dict[str, str]:
    return {"id": id_, "rule": rule, "detector": detector}


def test_forbidden_project_pair_violation() -> None:
    m = manifest(constraints=[constraint("C-1", "forbidden: gamma -> alpha", "import")])
    report = evaluate(m, graph(dep("gamma", "alpha")), TODAY)
    el = _by_id(report, "C-1")
    assert (el.verdict, el.reason) == ("violation", None)
    classes = sorted(f.finding_class for f in report.findings)
    assert classes == ["forbidden-edge", "undeclared-edge"]
    assert report.findings[0].element == "C-1"


def test_forbidden_project_pair_conformant_when_absent() -> None:
    m = manifest(constraints=[constraint("C-1", "forbidden: gamma -> alpha", "import")])
    report = evaluate(m, graph(dep("alpha", "beta")), TODAY)
    assert _by_id(report, "C-1").verdict == "conformant"


def test_forbidden_glob_endpoint() -> None:
    m = manifest(constraints=[constraint("C-2", "forbidden: gamma -> *", "import")])
    report = evaluate(m, graph(dep("gamma", "beta")), TODAY)
    assert _by_id(report, "C-2").verdict == "violation"


def test_forbidden_kind_filter_respected() -> None:
    # The dep edge exists but the constraint watches mcp edges only.
    m = manifest(constraints=[constraint("C-3", "forbidden: gamma -> alpha", "mcp")])
    report = evaluate(m, graph(dep("gamma", "alpha")), TODAY)
    assert _by_id(report, "C-3").verdict == "conformant"


def test_component_endpoint_sole_in_project_is_attributable() -> None:
    # gamma.reader is gamma's only modelled component → project granularity is exact.
    m = manifest(constraints=[constraint("C-4", "forbidden: gamma.reader -> beta", "import")])
    report = evaluate(m, graph(dep("gamma", "beta")), TODAY)
    assert _by_id(report, "C-4").verdict == "violation"


def test_component_endpoint_ambiguous_is_unsupported() -> None:
    # alpha hosts alpha.api AND alpha.worker → attribution needs module-level (v1.1).
    m = manifest(constraints=[constraint("C-5", "forbidden: alpha.worker -> beta", "import")])
    report = evaluate(m, graph(dep("alpha", "beta")), TODAY)
    el = _by_id(report, "C-5")
    assert (el.verdict, el.reason) == ("unknown", "unsupported-resolution")
    # Edge is undeclared: alpha->beta between modelled projects but not covered by interface.
    assert [f.finding_class for f in report.findings] == ["undeclared-edge"]


def test_file_endpoint_in_rule_matches_declared_path() -> None:
    m = manifest(
        constraints=[
            constraint("C-6", "forbidden: gamma.reader -> file:beta/secret.db", "declared")
        ]
    )
    hit = graph(declared("gamma", "beta", "beta/secret.db", "read"))
    miss = graph(declared("gamma", "beta", "beta/other.db", "read"))
    assert _by_id(evaluate(m, hit, TODAY), "C-6").verdict == "violation"
    assert _by_id(evaluate(m, miss, TODAY), "C-6").verdict == "conformant"


def test_manual_evidence_constraint_is_permanent_unknown() -> None:
    m = manifest(
        constraints=[constraint("C-7", "forbidden: только чтение, без мутаций", "manual-evidence")]
    )
    report = evaluate(m, graph(), TODAY)
    el = _by_id(report, "C-7")
    assert (el.verdict, el.reason) == ("unknown", "manual-evidence")
    assert [f.finding_class for f in report.findings] == ["manual-obligation"]


def test_constraint_component_outside_workspace() -> None:
    m = manifest(
        components=[
            {
                "id": "delta.ghost",
                "project": "delta",
                "kind": "service",
                "owner": "architects",
                "responsibility": "ghost",
            },
        ],
        constraints=[constraint("C-8", "forbidden: delta.ghost -> beta", "import")],
    )
    report = evaluate(m, graph(), TODAY)
    el = _by_id(report, "C-8")
    assert (el.verdict, el.reason) == ("unknown", "outside-workspace")
    assert [f.finding_class for f in report.findings] == ["orphan-component"]


from prograph.conformance.engine import exit_code  # noqa: E402


def test_undeclared_edge_between_modelled_projects() -> None:
    # gamma -> alpha observed; both projects modelled; no interface covers it.
    m = manifest(interfaces=[iface("I-01", "beta.lib", "alpha.api", "import")])
    report = evaluate(m, graph(dep("alpha", "beta"), dep("gamma", "alpha")), TODAY)
    undeclared = [f for f in report.findings if f.finding_class == "undeclared-edge"]
    assert len(undeclared) == 1
    assert undeclared[0].element is None
    assert "gamma" in undeclared[0].detail and "alpha" in undeclared[0].detail


def test_undeclared_edge_ignores_unmodelled_projects() -> None:
    m = manifest(
        components=[
            {
                "id": "alpha.api",
                "project": "alpha",
                "kind": "service",
                "owner": "architects",
                "responsibility": "api",
            },
        ]
    )
    # beta is not modelled → its edges never fire the finding.
    report = evaluate(m, graph(dep("alpha", "beta"), dep("beta", "alpha")), TODAY)
    assert report.findings == ()


def test_undeclared_edge_skips_contract_nodes_and_self_edges() -> None:
    m = manifest()
    report = evaluate(m, graph(contract("alpha", "feed-v1"), dep("alpha", "alpha")), TODAY)
    assert [f for f in report.findings if f.finding_class == "undeclared-edge"] == []


def test_active_exception_suppresses_and_waives() -> None:
    m = manifest(
        interfaces=[iface("I-05", "alpha.api", "gamma.reader", "mcp")],
        exceptions=[
            {
                "id": "EX-01",
                "target": "I-05",
                "reason": "next milestone",
                "owner": "architects",
                "expires": "2999-01-01",
            }
        ],
    )
    report = evaluate(m, graph(), TODAY)
    finding = next(f for f in report.findings if f.finding_class == "missing-required-edge")
    assert finding.suppressed_by == "EX-01"
    assert _by_id(report, "I-05").waived_by == "EX-01"
    assert report.exceptions[0].status == "active"
    assert exit_code(report, frozenset({"missing-required-edge"}), frozenset()) == 0


def test_expired_exception_is_a_violation_and_stops_suppressing() -> None:
    m = manifest(
        constraints=[constraint("C-1", "forbidden: gamma -> alpha", "import")],
        exceptions=[
            {
                "id": "EX-02",
                "target": "C-1",
                "reason": "grandfathered",
                "owner": "architects",
                "expires": "2020-01-01",
            }
        ],
    )
    report = evaluate(m, graph(dep("gamma", "alpha")), TODAY)
    classes = sorted(f.finding_class for f in report.findings)
    assert classes == ["expired-waiver", "forbidden-edge", "undeclared-edge"]
    assert _by_id(report, "C-1").waived_by is None
    assert report.exceptions[0].status == "expired"
    assert exit_code(report, frozenset(), frozenset()) == 1


def test_exit_code_default_policy() -> None:
    # unknowns and report-only findings do not fail a default run (spec D7).
    m = manifest(interfaces=[iface("I-05", "alpha.api", "gamma.reader", "mcp")])
    report = evaluate(m, graph(dep("gamma", "alpha")), TODAY)  # + undeclared gamma->alpha
    assert exit_code(report, frozenset(), frozenset()) == 0
    assert exit_code(report, frozenset({"undeclared-edge"}), frozenset()) == 1
    assert exit_code(report, frozenset(), frozenset({"unknown"})) == 1


def test_violation_always_fails() -> None:
    m = manifest(constraints=[constraint("C-1", "forbidden: gamma -> alpha", "import")])
    report = evaluate(m, graph(dep("gamma", "alpha")), TODAY)
    assert exit_code(report, frozenset(), frozenset()) == 1
