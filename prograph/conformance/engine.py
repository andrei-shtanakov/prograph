"""Conformance engine: three-valued verdicts over the observed edge store (spec D2-D5)."""

from __future__ import annotations

import datetime as dt
import json
from collections.abc import Mapping
from dataclasses import dataclass
from fnmatch import fnmatchcase

from prograph import _core
from prograph.conformance.manifest import (
    FILE_PREFIX,
    Component,
    Constraint,
    IntendedManifest,
    Interface,
    parse_rule,
)

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


@dataclass(frozen=True)
class Finding:
    """One report entry (spec D5). `element` is None for undeclared-edge."""

    finding_class: str
    element: str | None
    detail: str
    suppressed_by: str | None = None


@dataclass(frozen=True)
class ElementResult:
    """Verdict for one intended element (spec D4)."""

    id: str
    element_type: str
    detector: str
    verdict: str
    reason: str | None
    waived_by: str | None = None


@dataclass(frozen=True)
class ExceptionStatus:
    id: str
    target: str
    expires: str
    status: str  # "active" | "expired"


@dataclass(frozen=True)
class ConformanceReport:
    system: str
    elements: tuple[ElementResult, ...]
    findings: tuple[Finding, ...]
    exceptions: tuple[ExceptionStatus, ...]


@dataclass(frozen=True)
class _FileEndpoint:
    path: str


@dataclass(frozen=True)
class _RuleSide:
    kind: str  # "component" | "project" | "file"
    value: str  # glob pattern or file path (kind != "component")
    component: Component | None = None


def _resolve_side(raw: str, comp_by_id: dict[str, Component]) -> _RuleSide:
    if raw.startswith(FILE_PREFIX):
        return _RuleSide(kind="file", value=raw.removeprefix(FILE_PREFIX))
    comp = comp_by_id.get(raw)
    if comp is not None:
        return _RuleSide(kind="component", value=comp.project, component=comp)
    return _RuleSide(kind="project", value=raw)


def _side_matches(side: _RuleSide, edge: ObservedEdge, from_side: bool) -> bool:
    if side.kind == "file":
        return edge.kind == "declared" and _path_matches(side.value, edge.path)
    name = edge.from_name if from_side else edge.to_name
    if side.kind == "component":
        return name == side.value
    return fnmatchcase(name, side.value)


def _is_glob_pattern(name: str) -> bool:
    """Spec D6: a project endpoint with glob metacharacters is a pattern, not a name."""
    return any(ch in name for ch in "*?[")


def _path_matches(file_path: str, edge_path: str | None) -> bool:
    """Plan rule 1: exact or segment-suffix match, './' normalized off."""
    if edge_path is None:
        return False
    p = file_path.removeprefix("./")
    return edge_path == p or edge_path.endswith("/" + p)


def _component_gap(comp: Component, observed: ObservedGraph) -> tuple[str, Finding] | None:
    """Orphan checks (spec D4/D5). Returns (unknown-reason, finding) or None."""
    if comp.project not in observed.projects:
        return (
            REASON_OUTSIDE,
            Finding(
                "orphan-component",
                comp.id,
                f"project {comp.project!r} is not in the indexed workspace",
            ),
        )
    known = observed.project_paths.get(comp.project) or frozenset()
    if comp.scope and known:
        prefix = comp.scope.rstrip("/") + "/"
        if not any(p == comp.scope or p.startswith(prefix) for p in known):
            return (
                REASON_ORPHAN,
                Finding(
                    "orphan-component",
                    comp.id,
                    f"scope {comp.scope!r} matches nothing indexed in {comp.project!r}",
                ),
            )
    return None


def _match_interface(
    detector: str,
    producer: Component | _FileEndpoint,
    consumer: Component | _FileEndpoint,
    observed: ObservedGraph,
) -> ObservedEdge | None:
    """Return the first observed edge satisfying the interface, else None."""
    if detector == "contract":
        assert isinstance(producer, Component) and isinstance(consumer, Component)
        producer_nodes = {
            e.to_name
            for e in observed.edges
            if e.kind == "contract_link" and e.from_name == producer.project
        }
        for e in observed.edges:
            if (
                e.kind == "contract_link"
                and e.from_name == consumer.project
                and e.to_name in producer_nodes
            ):
                return e
        return None

    kind = DETECTOR_TO_KIND[detector]
    if detector in ("import", "mcp"):
        assert isinstance(producer, Component) and isinstance(consumer, Component)
        for e in observed.edges:
            if (
                e.kind == kind
                and e.from_name == consumer.project
                and e.to_kind == "project"
                and e.to_name == producer.project
            ):
                return e
        return None

    # declared
    if isinstance(producer, _FileEndpoint):
        assert isinstance(consumer, Component)
        for e in observed.edges:
            if (
                e.kind == "declared"
                and e.mode == "read"
                and e.from_name == consumer.project
                and _path_matches(producer.path, e.path)
            ):
                return e
        return None
    if isinstance(consumer, _FileEndpoint):
        for e in observed.edges:
            if (
                e.kind == "declared"
                and e.mode == "write"
                and e.from_name == producer.project
                and _path_matches(consumer.path, e.path)
            ):
                return e
        return None
    # component -> component: consumer reads producer's files, or producer writes into
    # consumer's; honor the producer's scope as a workspace-relative path prefix when set.
    scope_prefix = f"{producer.project}/{producer.scope.rstrip('/')}/" if producer.scope else None

    def _scope_ok(e: ObservedEdge) -> bool:
        if scope_prefix is None or e.path is None:
            return True
        return e.path.startswith(scope_prefix) or e.path == scope_prefix.rstrip("/")

    for e in observed.edges:
        if e.kind != "declared" or not _scope_ok(e):
            continue
        reads = (
            e.mode == "read" and e.from_name == consumer.project and e.to_name == producer.project
        )
        writes = (
            e.mode == "write" and e.from_name == producer.project and e.to_name == consumer.project
        )
        if reads or writes:
            return e
    return None


def _interface_result(
    iface: Interface,
    comp_by_id: dict[str, Component],
    observed: ObservedGraph,
) -> tuple[ElementResult, Finding | None, ObservedEdge | None]:
    def result(verdict: str, reason: str | None) -> ElementResult:
        return ElementResult(
            id=iface.id,
            element_type="interface",
            detector=iface.detector,
            verdict=verdict,
            reason=reason,
        )

    if iface.detector == "manual-evidence":
        finding = Finding(
            "manual-obligation",
            iface.id,
            "manual-evidence element: verify by review, restated in every report",
        )
        return result(VERDICT_UNKNOWN, REASON_MANUAL), finding, None

    endpoints: list[Component | _FileEndpoint] = []
    for raw in (iface.producer, iface.consumer):
        if raw.startswith(FILE_PREFIX):
            endpoints.append(_FileEndpoint(path=raw.removeprefix(FILE_PREFIX)))
        else:
            endpoints.append(comp_by_id[raw])
    producer, consumer = endpoints

    for endpoint in endpoints:
        if isinstance(endpoint, Component):
            gap = _component_gap(endpoint, observed)
            if gap is not None:
                reason, finding = gap
                # re-anchor the finding on the interface, not the component, so an
                # exception targeting the interface can suppress it (mirrors the
                # constraint path below).
                finding = Finding(finding.finding_class, iface.id, finding.detail)
                return result(VERDICT_UNKNOWN, reason), finding, None

    if (
        isinstance(producer, Component)
        and isinstance(consumer, Component)
        and producer.project == consumer.project
    ):
        return result(VERDICT_UNKNOWN, REASON_UNSUPPORTED), None, None

    matched = _match_interface(iface.detector, producer, consumer, observed)
    if matched is not None:
        return result(VERDICT_CONFORMANT, None), None, matched
    finding = Finding(
        "missing-required-edge",
        iface.id,
        f"detector {iface.detector!r} observed no edge for {iface.producer} -> {iface.consumer}",
    )
    return result(VERDICT_UNKNOWN, None), finding, None


def _constraint_result(
    con: Constraint,
    comp_by_id: dict[str, Component],
    project_component_count: dict[str, int],
    observed: ObservedGraph,
) -> tuple[ElementResult, Finding | None]:
    def result(verdict: str, reason: str | None) -> ElementResult:
        return ElementResult(
            id=con.id,
            element_type="constraint",
            detector=con.detector,
            verdict=verdict,
            reason=reason,
        )

    if con.detector == "manual-evidence":
        finding = Finding(
            "manual-obligation",
            con.id,
            "manual-evidence element: verify by review, restated in every report",
        )
        return result(VERDICT_UNKNOWN, REASON_MANUAL), finding

    rule = parse_rule(con.rule)  # load_manifest already guaranteed this parses
    src = _resolve_side(rule.src, comp_by_id)
    dst = _resolve_side(rule.dst, comp_by_id)

    for side in (src, dst):
        if side.component is not None:
            gap = _component_gap(side.component, observed)
            if gap is not None:
                reason, finding = gap
                # re-anchor the finding on the constraint, not the component
                finding = Finding(finding.finding_class, con.id, finding.detail)
                return result(VERDICT_UNKNOWN, reason), finding

    # A literal (non-glob) project-name endpoint that names neither an indexed
    # project nor any modelled component's project is a typo/rename, not a green
    # light — same honest-unknown treatment as a component outside the workspace.
    # Glob endpoints keep the existing "no match => conformant" semantics.
    for side in (src, dst):
        if (
            side.kind == "project"
            and not _is_glob_pattern(side.value)
            and side.value not in observed.projects
            and side.value not in project_component_count
        ):
            finding = Finding(
                "orphan-component",
                con.id,
                f"project {side.value!r} is not in the indexed workspace",
            )
            return result(VERDICT_UNKNOWN, REASON_OUTSIDE), finding

    # Plan rule 2: a component endpoint is attributable only when it is the sole
    # modelled component of its project; otherwise project-granularity evidence
    # cannot pin the edge on it — honest unknown until module-level v1.1.
    for side in (src, dst):
        if side.component is not None and project_component_count[side.component.project] > 1:
            return result(VERDICT_UNKNOWN, REASON_UNSUPPORTED), None

    kind = DETECTOR_TO_KIND[con.detector]
    for e in observed.edges:
        if e.kind != kind:
            continue
        if _side_matches(src, e, from_side=True) and _side_matches(dst, e, from_side=False):
            detail = f"observed {e.from_name} -[{e.kind}]-> {e.to_name} matches {con.rule!r}"
            finding = Finding("forbidden-edge", con.id, detail)
            return result(VERDICT_VIOLATION, None), finding
    return result(VERDICT_CONFORMANT, None), None


def evaluate(
    manifest: IntendedManifest, observed: ObservedGraph, today: dt.date
) -> ConformanceReport:
    """Evaluate every intended element against the observed graph (spec D4/D5)."""
    comp_by_id = {c.id: c for c in manifest.components}
    elements: list[ElementResult] = []
    findings: list[Finding] = []
    covered: set[ObservedEdge] = set()

    for iface in manifest.interfaces:
        element, finding, matched = _interface_result(iface, comp_by_id, observed)
        elements.append(element)
        if finding is not None:
            findings.append(finding)
        if matched is not None:
            covered.add(matched)

    project_component_count: dict[str, int] = {}
    for comp in manifest.components:
        project_component_count[comp.project] = project_component_count.get(comp.project, 0) + 1

    for con in manifest.constraints:
        element, finding = _constraint_result(con, comp_by_id, project_component_count, observed)
        elements.append(element)
        if finding is not None:
            findings.append(finding)

    # Spec D5 undeclared-edge, bounded by plan rule 4 (project→project kinds only).
    modelled_projects = {c.project for c in manifest.components}
    for e in observed.edges:
        if e.to_kind != "project" or e.from_name == e.to_name:
            continue
        if e.from_name not in modelled_projects or e.to_name not in modelled_projects:
            continue
        if e in covered:
            continue
        detail = f"observed {e.from_name} -[{e.kind}]-> {e.to_name} appears in no interface"
        if e.path is not None:
            detail += f" (path {e.path!r})"
        findings.append(Finding("undeclared-edge", None, detail))

    # Exceptions: active ones suppress findings + waive their element; expired ones are
    # violations on the exception itself (spec D5) and suppress nothing.
    exception_statuses: list[ExceptionStatus] = []
    active_by_target: dict[str, str] = {}
    for entry in manifest.exceptions:
        expired = entry.expires < today
        exception_statuses.append(
            ExceptionStatus(
                id=entry.id,
                target=entry.target,
                expires=entry.expires.isoformat(),
                status="expired" if expired else "active",
            )
        )
        if expired:
            findings.append(
                Finding(
                    "expired-waiver",
                    entry.id,
                    f"exception on {entry.target} expired {entry.expires.isoformat()}",
                )
            )
        else:
            active_by_target[entry.target] = entry.id

    if active_by_target:
        findings = [
            Finding(
                f.finding_class,
                f.element,
                f.detail,
                suppressed_by=active_by_target.get(f.element or ""),
            )
            for f in findings
        ]
        elements = [
            ElementResult(
                id=el.id,
                element_type=el.element_type,
                detector=el.detector,
                verdict=el.verdict,
                reason=el.reason,
                waived_by=active_by_target.get(el.id),
            )
            for el in elements
        ]

    findings.sort(key=lambda f: (f.finding_class, f.element or "", f.detail))
    return ConformanceReport(
        system=manifest.system,
        elements=tuple(elements),
        findings=tuple(findings),
        exceptions=tuple(exception_statuses),
    )


def exit_code(
    report: ConformanceReport,
    fail_on: frozenset[str],
    fail_on_verdict: frozenset[str],
) -> int:
    """Spec D7 exit policy for exit 0 vs 1 (exit 2 is the CLI's tool-error path)."""
    violation = any(
        el.verdict == VERDICT_VIOLATION and el.waived_by is None for el in report.elements
    )
    expired = any(f.finding_class == "expired-waiver" for f in report.findings)
    escalated_finding = any(
        f.suppressed_by is None and f.finding_class in fail_on for f in report.findings
    )
    escalated_verdict = any(
        el.waived_by is None and el.verdict in fail_on_verdict for el in report.elements
    )
    return 1 if (violation or expired or escalated_finding or escalated_verdict) else 0
