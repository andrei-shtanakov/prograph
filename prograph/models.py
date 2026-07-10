"""Pydantic models mirroring the Rust dataclasses exposed by prograph._core.

These models are the single source of truth for shapes flowing through CLI --json
output, MCP tool responses, and FastAPI endpoints. They round-trip with the Rust
side via from_core() / to_core() helpers.
"""

from __future__ import annotations

import json
from enum import StrEnum

from pydantic import BaseModel, ConfigDict

import prograph._core as _core


class ProjectKind(StrEnum):
    PYTHON = "python"
    RUST = "rust"
    JS = "js"
    DOCS = "docs"
    MIXED = "mixed"

    @classmethod
    def from_core(cls, value: _core.ProjectKind) -> ProjectKind:
        return cls(value.name())


class ProjectCandidate(BaseModel):
    """A project discovered in the monorepo, before any deep parsing."""

    model_config = ConfigDict(frozen=True)

    name: str
    root_path: str
    kind: ProjectKind
    manifests: list[str]

    @classmethod
    def from_core(cls, value: _core.ProjectCandidate) -> ProjectCandidate:
        return cls(
            name=value.name,
            root_path=value.root_path,
            kind=ProjectKind.from_core(value.kind),
            manifests=list(value.manifests),
        )


class NodeKind(StrEnum):
    PROJECT = "project"
    CONTRACT = "contract"

    @classmethod
    def from_core(cls, value: _core.NodeKind) -> NodeKind:
        return cls(value.name())


class EdgeKind(StrEnum):
    PACKAGE_DEP = "package_dep"
    MCP_CALL = "mcp_call"
    CONTRACT_LINK = "contract_link"

    @classmethod
    def from_core(cls, value: _core.EdgeKind) -> EdgeKind:
        return cls(value.name())


class ChangeKind(StrEnum):
    ADDED = "added"
    REMOVED = "removed"
    ATTRS_CHANGED = "attrs_changed"

    @classmethod
    def from_core(cls, value: _core.ChangeKind) -> ChangeKind:
        return cls(value.name())


class EntityKind(StrEnum):
    PROJECT = "project"
    EDGE = "edge"
    CONTRACT = "contract"

    @classmethod
    def from_core(cls, value: _core.EntityKind) -> EntityKind:
        return cls(value.name())


class Edge(BaseModel):
    """A persisted cross-project edge."""

    model_config = ConfigDict(frozen=True)

    id: int
    kind: EdgeKind
    from_kind: NodeKind
    from_id: int
    to_kind: NodeKind
    to_id: int
    attrs: dict[str, object]
    first_seen: int
    last_seen: int

    @classmethod
    def from_core(cls, value: _core.Edge) -> Edge:
        return cls(
            id=value.id,
            kind=EdgeKind.from_core(value.kind),
            from_kind=NodeKind.from_core(value.from_kind),
            from_id=value.from_id,
            to_kind=NodeKind.from_core(value.to_kind),
            to_id=value.to_id,
            attrs=json.loads(value.attrs_json),
            first_seen=value.first_seen,
            last_seen=value.last_seen,
        )


class ChangeEvent(BaseModel):
    """A row from the change_log table."""

    model_config = ConfigDict(frozen=True)

    id: int
    snapshot_id: int
    ts: str
    entity_kind: EntityKind
    entity_id: int
    change: ChangeKind
    before: dict[str, object] | None
    after: dict[str, object] | None

    @classmethod
    def from_core(cls, value: _core.ChangeEvent) -> ChangeEvent:
        return cls(
            id=value.id,
            snapshot_id=value.snapshot_id,
            ts=value.ts,
            entity_kind=EntityKind.from_core(value.entity_kind),
            entity_id=value.entity_id,
            change=ChangeKind.from_core(value.change),
            before=json.loads(value.before_json) if value.before_json else None,
            after=json.loads(value.after_json) if value.after_json else None,
        )


class SnapshotInfo(BaseModel):
    """Metadata about a single `prograph index` snapshot."""

    model_config = ConfigDict(frozen=True)

    id: int
    ts: str
    monorepo_root: str
    git_commit: str | None
    prograph_version: str
    n_projects: int
    n_edges: int
    n_changes: int

    @classmethod
    def from_core(cls, value: _core.SnapshotInfo) -> SnapshotInfo:
        return cls(
            id=value.id,
            ts=value.ts,
            monorepo_root=value.monorepo_root,
            git_commit=value.git_commit,
            prograph_version=value.prograph_version,
            n_projects=value.n_projects,
            n_edges=value.n_edges,
            n_changes=value.n_changes,
        )


class IndexSummary(BaseModel):
    """Summary returned by `prograph index` after running."""

    model_config = ConfigDict(frozen=True)

    snapshot_id: int
    ts: str
    n_projects: int
    n_edges: int
    n_changes: int
    n_warnings: int
    duration_ms: int

    @classmethod
    def from_core(cls, value: _core.IndexSummary) -> IndexSummary:
        return cls(
            snapshot_id=value.snapshot_id,
            ts=value.ts,
            n_projects=value.n_projects,
            n_edges=value.n_edges,
            n_changes=value.n_changes,
            n_warnings=value.n_warnings,
            duration_ms=value.duration_ms,
        )


class Contract(BaseModel):
    """A shared contract (JSON Schema / OpenAPI / .proto) referenced by >=2 projects."""

    model_config = ConfigDict(frozen=True)

    id: int
    declared_id: str | None
    content_hash: str
    kind: str
    first_seen: int
    last_seen: int

    @classmethod
    def from_core(cls, value: _core.Contract) -> Contract:
        return cls(
            id=value.id,
            declared_id=value.declared_id,
            content_hash=value.content_hash,
            kind=value.kind,
            first_seen=value.first_seen,
            last_seen=value.last_seen,
        )


class OutboundEdge(BaseModel):
    model_config = ConfigDict(frozen=True)

    kind: str
    target_kind: str
    target_name: str
    target_slug: str
    attrs: dict[str, object]

    @classmethod
    def from_core(cls, value: _core.OutboundEdge) -> OutboundEdge:
        return cls(
            kind=value.kind,
            target_kind=value.target_kind,
            target_name=value.target_name,
            target_slug=value.target_slug,
            attrs=json.loads(value.attrs_json) if value.attrs_json else {},
        )


class InboundEdge(BaseModel):
    model_config = ConfigDict(frozen=True)

    kind: str
    source_name: str
    source_slug: str
    attrs: dict[str, object]

    @classmethod
    def from_core(cls, value: _core.InboundEdge) -> InboundEdge:
        return cls(
            kind=value.kind,
            source_name=value.source_name,
            source_slug=value.source_slug,
            attrs=json.loads(value.attrs_json) if value.attrs_json else {},
        )


class McpToolDeclRow(BaseModel):
    model_config = ConfigDict(frozen=True)

    tool_name: str
    rel_path: str
    line: int

    @classmethod
    def from_core(cls, value: _core.McpToolDeclRow) -> McpToolDeclRow:
        return cls(tool_name=value.tool_name, rel_path=value.rel_path, line=value.line)


class ContractFileRow(BaseModel):
    model_config = ConfigDict(frozen=True)

    contract_declared_id: str | None
    contract_slug: str
    contract_kind: str
    rel_path: str

    @classmethod
    def from_core(cls, value: _core.ContractFileRow) -> ContractFileRow:
        return cls(
            contract_declared_id=value.contract_declared_id,
            contract_slug=value.contract_slug,
            contract_kind=value.contract_kind,
            rel_path=value.rel_path,
        )


class RecentChangeRow(BaseModel):
    model_config = ConfigDict(frozen=True)

    snapshot_id: int
    ts: str
    change: str
    summary: str

    @classmethod
    def from_core(cls, value: _core.RecentChangeRow) -> RecentChangeRow:
        return cls(
            snapshot_id=value.snapshot_id,
            ts=value.ts,
            change=value.change,
            summary=value.summary,
        )


class ModuleRow(BaseModel):
    """M9: one source file (module) inside a project."""

    model_config = ConfigDict(frozen=True)

    id: int
    rel_path: str
    language: str

    @classmethod
    def from_core(cls, value: _core.ModuleRow) -> ModuleRow:
        return cls(id=value.id, rel_path=value.rel_path, language=value.language)


class PublicSymbolRow(BaseModel):
    """M9: one public symbol exposed by a module."""

    model_config = ConfigDict(frozen=True)

    module_id: int
    rel_path: str
    name: str
    kind: str
    line: int

    @classmethod
    def from_core(cls, value: _core.PublicSymbolRow) -> PublicSymbolRow:
        return cls(
            module_id=value.module_id,
            rel_path=value.rel_path,
            name=value.name,
            kind=value.kind,
            line=value.line,
        )


class InternalImportRow(BaseModel):
    """M9: one internal import inside a module."""

    model_config = ConfigDict(frozen=True)

    module_id: int
    rel_path: str
    target_path: str
    line: int

    @classmethod
    def from_core(cls, value: _core.InternalImportRow) -> InternalImportRow:
        return cls(
            module_id=value.module_id,
            rel_path=value.rel_path,
            target_path=value.target_path,
            line=value.line,
        )


class SymbolRefRow(BaseModel):
    """M10: one cross-project symbol reference (resolved from an external import)."""

    model_config = ConfigDict(frozen=True)

    from_project_name: str
    from_module_rel_path: str
    line: int
    to_project_name: str
    to_module_path: str
    to_symbol_name: str | None

    @classmethod
    def from_core(cls, value: _core.SymbolRefRow) -> SymbolRefRow:
        return cls(
            from_project_name=value.from_project_name,
            from_module_rel_path=value.from_module_rel_path,
            line=value.line,
            to_project_name=value.to_project_name,
            to_module_path=value.to_module_path,
            to_symbol_name=value.to_symbol_name,
        )


class DriftFinding(BaseModel):
    """M11: one drift finding (declared-vs-actual discrepancy)."""

    model_config = ConfigDict(frozen=True)

    project_name: str
    kind: str  # "missing" | "extra" | "stale_todo" | "stale_declaration"
    entity_kind: str  # "public_symbol" | "mcp_tool" | "contract" | "todo" | "declared_path"
    entity_name: str
    source_path: str
    source_line: int
    confidence: str  # "high" | "low"
    detail: str | None

    @classmethod
    def from_core(cls, value: _core.DriftFindingRow) -> DriftFinding:
        return cls(
            project_name=value.project_name,
            kind=value.kind,
            entity_kind=value.entity_kind,
            entity_name=value.entity_name,
            source_path=value.source_path,
            source_line=value.source_line,
            confidence=value.confidence,
            detail=value.detail,
        )


class ProjectDescription(BaseModel):
    model_config = ConfigDict(frozen=True)

    project_id: int
    name: str
    slug: str
    kind: str
    root_path: str
    attrs: dict[str, object]
    snapshot_id: int
    snapshot_ts: str
    mcp_decls: list[McpToolDeclRow]
    contract_files: list[ContractFileRow]
    outbound: list[OutboundEdge]
    inbound: list[InboundEdge]
    recent_changes: list[RecentChangeRow]
    modules: list[ModuleRow] = []
    public_symbols: list[PublicSymbolRow] = []
    internal_imports: list[InternalImportRow] = []
    inbound_refs: list[SymbolRefRow] = []
    outbound_refs: list[SymbolRefRow] = []
    drifts: list[DriftFinding] = []

    @classmethod
    def from_core(cls, value: _core.ProjectDescription) -> ProjectDescription:
        return cls(
            project_id=value.project_id,
            name=value.name,
            slug=value.slug,
            kind=value.kind,
            root_path=value.root_path,
            attrs=json.loads(value.attrs_json) if value.attrs_json else {},
            snapshot_id=value.snapshot_id,
            snapshot_ts=value.snapshot_ts,
            mcp_decls=[McpToolDeclRow.from_core(d) for d in value.mcp_decls],
            contract_files=[ContractFileRow.from_core(c) for c in value.contract_files],
            outbound=[OutboundEdge.from_core(e) for e in value.outbound],
            inbound=[InboundEdge.from_core(e) for e in value.inbound],
            recent_changes=[RecentChangeRow.from_core(c) for c in value.recent_changes],
            modules=[ModuleRow.from_core(m) for m in value.modules],
            public_symbols=[PublicSymbolRow.from_core(s) for s in value.public_symbols],
            internal_imports=[InternalImportRow.from_core(i) for i in value.internal_imports],
            inbound_refs=[SymbolRefRow.from_core(r) for r in value.inbound_refs],
            outbound_refs=[SymbolRefRow.from_core(r) for r in value.outbound_refs],
            drifts=[DriftFinding.from_core(d) for d in value.drifts],
        )


class ContractOwner(BaseModel):
    model_config = ConfigDict(frozen=True)

    project_name: str
    project_slug: str
    rel_path: str

    @classmethod
    def from_core(cls, value: _core.ContractOwner) -> ContractOwner:
        return cls(
            project_name=value.project_name,
            project_slug=value.project_slug,
            rel_path=value.rel_path,
        )


class ContractDescription(BaseModel):
    model_config = ConfigDict(frozen=True)

    contract_id: int
    declared_id: str | None
    slug: str
    kind: str
    content_hash: str
    snapshot_id: int
    snapshot_ts: str
    owners: list[ContractOwner]
    recent_changes: list[RecentChangeRow]

    @classmethod
    def from_core(cls, value: _core.ContractDescription) -> ContractDescription:
        return cls(
            contract_id=value.contract_id,
            declared_id=value.declared_id,
            slug=value.slug,
            kind=value.kind,
            content_hash=value.content_hash,
            snapshot_id=value.snapshot_id,
            snapshot_ts=value.snapshot_ts,
            owners=[ContractOwner.from_core(o) for o in value.owners],
            recent_changes=[RecentChangeRow.from_core(c) for c in value.recent_changes],
        )


class ProjectSummary(BaseModel):
    model_config = ConfigDict(frozen=True)

    name: str
    slug: str
    kind: str

    @classmethod
    def from_core(cls, value: _core.ProjectSummary) -> ProjectSummary:
        return cls(name=value.name, slug=value.slug, kind=value.kind)


class ContractSummary(BaseModel):
    model_config = ConfigDict(frozen=True)

    slug: str
    declared_id: str | None
    kind: str
    n_owners: int

    @classmethod
    def from_core(cls, value: _core.ContractSummary) -> ContractSummary:
        return cls(
            slug=value.slug,
            declared_id=value.declared_id,
            kind=value.kind,
            n_owners=value.n_owners,
        )


class MonorepoOverview(BaseModel):
    model_config = ConfigDict(frozen=True)

    monorepo_root: str
    snapshot_id: int
    snapshot_ts: str
    n_projects: int
    n_contracts: int
    n_edges: int
    projects: list[ProjectSummary]
    contracts: list[ContractSummary]
    recent_changes: list[RecentChangeRow]

    @classmethod
    def from_core(cls, value: _core.MonorepoOverview) -> MonorepoOverview:
        return cls(
            monorepo_root=value.monorepo_root,
            snapshot_id=value.snapshot_id,
            snapshot_ts=value.snapshot_ts,
            n_projects=value.n_projects,
            n_contracts=value.n_contracts,
            n_edges=value.n_edges,
            projects=[ProjectSummary.from_core(p) for p in value.projects],
            contracts=[ContractSummary.from_core(c) for c in value.contracts],
            recent_changes=[RecentChangeRow.from_core(c) for c in value.recent_changes],
        )


class EdgeRow(BaseModel):
    model_config = ConfigDict(frozen=True)

    id: int
    kind: str
    from_kind: str
    from_id: int
    from_name: str
    to_kind: str
    to_id: int
    to_name: str
    attrs: dict[str, object]
    first_seen: int
    last_seen: int

    @classmethod
    def from_core(cls, value: _core.EdgeRow) -> EdgeRow:
        return cls(
            id=value.id,
            kind=value.kind,
            from_kind=value.from_kind,
            from_id=value.from_id,
            from_name=value.from_name,
            to_kind=value.to_kind,
            to_id=value.to_id,
            to_name=value.to_name,
            attrs=json.loads(value.attrs_json) if value.attrs_json else {},
            first_seen=value.first_seen,
            last_seen=value.last_seen,
        )


class EdgeEvidenceRow(BaseModel):
    model_config = ConfigDict(frozen=True)

    edge_id: int
    project_id: int
    project_name: str
    rel_path: str
    line: int
    snippet: str | None

    @classmethod
    def from_core(cls, value: _core.EdgeEvidenceRow) -> EdgeEvidenceRow:
        return cls(
            edge_id=value.edge_id,
            project_id=value.project_id,
            project_name=value.project_name,
            rel_path=value.rel_path,
            line=value.line,
            snippet=value.snippet,
        )


class SearchHit(BaseModel):
    model_config = ConfigDict(frozen=True)

    entity_kind: str
    entity_id: int
    name: str
    snippet: str
    rank: float

    @classmethod
    def from_core(cls, value: _core.SearchHit) -> SearchHit:
        return cls(
            entity_kind=value.entity_kind,
            entity_id=value.entity_id,
            name=value.name,
            snippet=value.snippet,
            rank=value.rank,
        )


class DiffEdgeRow(BaseModel):
    """Edge enriched with diff status relative to a `since` snapshot."""

    model_config = ConfigDict(frozen=True)

    id: int
    kind: str
    from_kind: str
    from_id: int
    from_name: str
    to_kind: str
    to_id: int
    to_name: str
    attrs: dict[str, object]
    first_seen: int
    last_seen: int
    status: str  # "added" | "removed" | "unchanged"

    @classmethod
    def from_core(cls, value: _core.DiffEdgeRow) -> DiffEdgeRow:
        return cls(
            id=value.id,
            kind=value.kind,
            from_kind=value.from_kind,
            from_id=value.from_id,
            from_name=value.from_name,
            to_kind=value.to_kind,
            to_id=value.to_id,
            to_name=value.to_name,
            attrs=json.loads(value.attrs_json) if value.attrs_json else {},
            first_seen=value.first_seen,
            last_seen=value.last_seen,
            status=value.status,
        )
