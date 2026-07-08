"""Type stubs for the prograph._core PyO3 extension module."""

from typing import ClassVar

def version() -> str: ...
def scan_monorepo(monorepo_root: str) -> list[ProjectCandidate]: ...
def index_monorepo(monorepo_root: str, db_path: str) -> IndexSummary: ...
def latest_snapshot_info(db_path: str) -> SnapshotInfo | None: ...
def describe_project(db_path: str, project_id: int) -> ProjectDescription | None: ...
def describe_contract(db_path: str, contract_id: int) -> ContractDescription | None: ...
def monorepo_overview(db_path: str) -> MonorepoOverview | None: ...
def project_by_name(db_path: str, name: str) -> int | None: ...
def snapshot_by_id(db_path: str, id: int) -> SnapshotInfo | None: ...
def find_edges_filtered(
    db_path: str,
    from_name: str | None = None,
    to_name: str | None = None,
    kind: str | None = None,
    since_snapshot: int | None = None,
) -> list[EdgeRow]: ...
def edge_evidence_for(db_path: str, edge_id: int) -> list[EdgeEvidenceRow]: ...
def find_edges_with_status_since(db_path: str, since_snapshot: int) -> list[DiffEdgeRow]: ...
def search_fts(
    db_path: str, query: str, kinds: list[str] | None = None, limit: int = 20
) -> list[SearchHit]: ...
def changelog_paginated(
    db_path: str,
    since_snapshot: int | None = None,
    entity_kind: str | None = None,
    limit: int = 50,
) -> list[ChangeEvent]: ...
def refs_to_symbol(
    db_path: str, project_name: str, symbol_name: str | None = None
) -> list[SymbolRefRow]: ...
def refs_from_project(db_path: str, project_name: str) -> list[SymbolRefRow]: ...
def drifts_for_project(db_path: str, project_name: str) -> list[DriftFindingRow]: ...
def find_drifts_filtered(db_path: str, kind: str | None = None) -> list[DriftFindingRow]: ...

class ProjectKind:
    Python: ClassVar[ProjectKind]
    Rust: ClassVar[ProjectKind]
    Js: ClassVar[ProjectKind]
    Docs: ClassVar[ProjectKind]
    Mixed: ClassVar[ProjectKind]

    def name(self) -> str: ...

class ProjectCandidate:
    name: str
    root_path: str
    kind: ProjectKind
    manifests: list[str]

    def __init__(
        self,
        name: str,
        root_path: str,
        kind: ProjectKind,
        manifests: list[str],
    ) -> None: ...

class NodeKind:
    Project: ClassVar[NodeKind]
    Contract: ClassVar[NodeKind]
    def name(self) -> str: ...

class EdgeKind:
    PackageDep: ClassVar[EdgeKind]
    McpCall: ClassVar[EdgeKind]
    ContractLink: ClassVar[EdgeKind]
    def name(self) -> str: ...

class ChangeKind:
    Added: ClassVar[ChangeKind]
    Removed: ClassVar[ChangeKind]
    AttrsChanged: ClassVar[ChangeKind]
    def name(self) -> str: ...

class EntityKind:
    Project: ClassVar[EntityKind]
    Edge: ClassVar[EntityKind]
    Contract: ClassVar[EntityKind]
    def name(self) -> str: ...

class Edge:
    id: int
    kind: EdgeKind
    from_kind: NodeKind
    from_id: int
    to_kind: NodeKind
    to_id: int
    attrs_json: str
    first_seen: int
    last_seen: int

class ChangeEvent:
    id: int
    snapshot_id: int
    ts: str
    entity_kind: EntityKind
    entity_id: int
    change: ChangeKind
    before_json: str | None
    after_json: str | None

class SnapshotInfo:
    id: int
    ts: str
    monorepo_root: str
    git_commit: str | None
    prograph_version: str
    n_projects: int
    n_edges: int
    n_changes: int

class IndexSummary:
    snapshot_id: int
    ts: str
    n_projects: int
    n_edges: int
    n_changes: int
    n_warnings: int
    duration_ms: int

class Contract:
    id: int
    declared_id: str | None
    content_hash: str
    kind: str
    first_seen: int
    last_seen: int

class OutboundEdge:
    kind: str
    target_kind: str
    target_name: str
    target_slug: str
    attrs_json: str

class InboundEdge:
    kind: str
    source_name: str
    source_slug: str
    attrs_json: str

class McpToolDeclRow:
    tool_name: str
    rel_path: str
    line: int

class ContractFileRow:
    contract_declared_id: str | None
    contract_slug: str
    contract_kind: str
    rel_path: str

class RecentChangeRow:
    snapshot_id: int
    ts: str
    change: str
    summary: str

class ProjectDescription:
    project_id: int
    name: str
    slug: str
    kind: str
    root_path: str
    attrs_json: str
    snapshot_id: int
    snapshot_ts: str
    mcp_decls: list[McpToolDeclRow]
    contract_files: list[ContractFileRow]
    outbound: list[OutboundEdge]
    inbound: list[InboundEdge]
    recent_changes: list[RecentChangeRow]
    modules: list[ModuleRow]
    public_symbols: list[PublicSymbolRow]
    internal_imports: list[InternalImportRow]
    inbound_refs: list[SymbolRefRow]
    outbound_refs: list[SymbolRefRow]
    drifts: list[DriftFindingRow]

class ContractOwner:
    project_name: str
    project_slug: str
    rel_path: str

class ContractDescription:
    contract_id: int
    declared_id: str | None
    slug: str
    kind: str
    content_hash: str
    snapshot_id: int
    snapshot_ts: str
    owners: list[ContractOwner]
    recent_changes: list[RecentChangeRow]

class ProjectSummary:
    name: str
    slug: str
    kind: str

class ContractSummary:
    slug: str
    declared_id: str | None
    kind: str
    n_owners: int

class MonorepoOverview:
    monorepo_root: str
    snapshot_id: int
    snapshot_ts: str
    n_projects: int
    n_contracts: int
    n_edges: int
    projects: list[ProjectSummary]
    contracts: list[ContractSummary]
    recent_changes: list[RecentChangeRow]

class EdgeRow:
    id: int
    kind: str
    from_kind: str
    from_id: int
    from_name: str
    to_kind: str
    to_id: int
    to_name: str
    attrs_json: str
    first_seen: int
    last_seen: int

class EdgeEvidenceRow:
    edge_id: int
    project_id: int
    project_name: str
    rel_path: str
    line: int
    snippet: str | None

class SearchHit:
    entity_kind: str
    entity_id: int
    name: str
    snippet: str
    rank: float

class DiffEdgeRow:
    id: int
    kind: str
    from_kind: str
    from_id: int
    from_name: str
    to_kind: str
    to_id: int
    to_name: str
    attrs_json: str
    first_seen: int
    last_seen: int
    status: str

class ModuleRow:
    id: int
    rel_path: str
    language: str

class PublicSymbolRow:
    module_id: int
    rel_path: str
    name: str
    kind: str
    line: int

class InternalImportRow:
    module_id: int
    rel_path: str
    target_path: str
    line: int

class SymbolRefRow:
    from_project_name: str
    from_module_rel_path: str
    line: int
    to_project_name: str
    to_module_path: str
    to_symbol_name: str | None

class DriftFindingRow:
    project_name: str
    kind: str
    entity_kind: str
    entity_name: str
    source_path: str
    source_line: int
    confidence: str
    detail: str | None
