"""prograph — cross-project structure mapper for monorepos."""

from prograph._core import version as _core_version
from prograph.models import (
    ChangeEvent,
    ChangeKind,
    Contract,
    ContractDescription,
    ContractFileRow,
    ContractOwner,
    ContractSummary,
    DiffEdgeRow,
    DriftFinding,
    Edge,
    EdgeEvidenceRow,
    EdgeKind,
    EdgeRow,
    EntityKind,
    InboundEdge,
    IndexSummary,
    InternalImportRow,
    McpToolDeclRow,
    ModuleRow,
    MonorepoOverview,
    NodeKind,
    OutboundEdge,
    ProjectCandidate,
    ProjectDescription,
    ProjectKind,
    ProjectSummary,
    PublicSymbolRow,
    RecentChangeRow,
    SearchHit,
    SnapshotInfo,
    SymbolRefRow,
)

__version__ = "0.1.0"


def core_version() -> str:
    """Return the Rust core crate version."""
    return _core_version()


__all__ = [
    "ChangeEvent",
    "ChangeKind",
    "Contract",
    "ContractDescription",
    "ContractFileRow",
    "ContractOwner",
    "ContractSummary",
    "DiffEdgeRow",
    "DriftFinding",
    "Edge",
    "EdgeEvidenceRow",
    "EdgeKind",
    "EdgeRow",
    "EntityKind",
    "InboundEdge",
    "IndexSummary",
    "InternalImportRow",
    "McpToolDeclRow",
    "ModuleRow",
    "MonorepoOverview",
    "NodeKind",
    "OutboundEdge",
    "ProjectCandidate",
    "ProjectDescription",
    "ProjectKind",
    "ProjectSummary",
    "PublicSymbolRow",
    "RecentChangeRow",
    "SearchHit",
    "SnapshotInfo",
    "SymbolRefRow",
    "__version__",
    "core_version",
]
