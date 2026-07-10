//! Cross-language data classes. Exposed to Python via PyO3.

use pyo3::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[pyclass(eq, frozen, module = "prograph._core", from_py_object)]
pub enum ProjectKind {
    Python,
    Rust,
    Js,
    Docs,
    Mixed,
}

#[pymethods]
impl ProjectKind {
    fn __repr__(&self) -> String {
        format!("ProjectKind.{:?}", self)
    }

    /// Canonical lowercase name used in storage and CLI output.
    pub fn name(&self) -> &'static str {
        match self {
            ProjectKind::Python => "python",
            ProjectKind::Rust => "rust",
            ProjectKind::Js => "js",
            ProjectKind::Docs => "docs",
            ProjectKind::Mixed => "mixed",
        }
    }
}

/// A discovered project candidate (before any parsing).
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct ProjectCandidate {
    pub name: String,
    pub root_path: String, // relative to monorepo root
    pub kind: ProjectKind,
    pub manifests: Vec<String>, // relative paths to detected signal files
}

#[pymethods]
impl ProjectCandidate {
    #[new]
    fn new(name: String, root_path: String, kind: ProjectKind, manifests: Vec<String>) -> Self {
        Self {
            name,
            root_path,
            kind,
            manifests,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "ProjectCandidate(name='{}', kind={:?}, root='{}', manifests={})",
            self.name,
            self.kind,
            self.root_path,
            self.manifests.len()
        )
    }
}

/// Direction-tagged identifier of a graph endpoint (project or contract).
/// M2 only emits 'project'; M4 will add 'contract'.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[pyclass(eq, frozen, module = "prograph._core", from_py_object)]
pub enum NodeKind {
    Project,
    Contract,
}

#[pymethods]
impl NodeKind {
    fn __repr__(&self) -> String {
        format!("NodeKind.{:?}", self)
    }

    fn name(&self) -> &'static str {
        match self {
            NodeKind::Project => "project",
            NodeKind::Contract => "contract",
        }
    }
}

/// Edge kind. M4 emits all three: PackageDep, McpCall, ContractLink. M12 adds Declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[pyclass(eq, frozen, module = "prograph._core", from_py_object)]
pub enum EdgeKind {
    PackageDep,
    McpCall,
    ContractLink,
    Declared,
}

#[pymethods]
impl EdgeKind {
    fn __repr__(&self) -> String {
        format!("EdgeKind.{:?}", self)
    }

    pub fn name(&self) -> &'static str {
        match self {
            EdgeKind::PackageDep => "package_dep",
            EdgeKind::McpCall => "mcp_call",
            EdgeKind::ContractLink => "contract_link",
            EdgeKind::Declared => "declared",
        }
    }
}

/// A persisted edge with full provenance.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct Edge {
    pub id: i64,
    pub kind: EdgeKind,
    pub from_kind: NodeKind,
    pub from_id: i64,
    pub to_kind: NodeKind,
    pub to_id: i64,
    pub attrs_json: String, // serialized JSON; pydantic side parses
    pub first_seen: i64,
    pub last_seen: i64,
}

#[pymethods]
impl Edge {
    fn __repr__(&self) -> String {
        format!(
            "Edge(id={}, kind={:?}, {}#{} → {}#{})",
            self.id,
            self.kind,
            self.from_kind.name(),
            self.from_id,
            self.to_kind.name(),
            self.to_id
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[pyclass(eq, frozen, module = "prograph._core", from_py_object)]
pub enum ChangeKind {
    Added,
    Removed,
    AttrsChanged,
}

#[pymethods]
impl ChangeKind {
    fn __repr__(&self) -> String {
        format!("ChangeKind.{:?}", self)
    }

    fn name(&self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Removed => "removed",
            ChangeKind::AttrsChanged => "attrs_changed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[pyclass(eq, frozen, module = "prograph._core", from_py_object)]
pub enum EntityKind {
    Project,
    Edge,
    Contract,
}

#[pymethods]
impl EntityKind {
    fn __repr__(&self) -> String {
        format!("EntityKind.{:?}", self)
    }

    fn name(&self) -> &'static str {
        match self {
            EntityKind::Project => "project",
            EntityKind::Edge => "edge",
            EntityKind::Contract => "contract",
        }
    }
}

/// A row from the change_log table.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct ChangeEvent {
    pub id: i64,
    pub snapshot_id: i64,
    pub ts: String,
    pub entity_kind: EntityKind,
    pub entity_id: i64,
    pub change: ChangeKind,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
}

#[pymethods]
impl ChangeEvent {
    fn __repr__(&self) -> String {
        format!(
            "ChangeEvent(snap={}, {:?} {}#{}: {:?})",
            self.snapshot_id,
            self.change,
            self.entity_kind.name(),
            self.entity_id,
            self.change
        )
    }
}

/// Metadata about a single `prograph index` snapshot.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct SnapshotInfo {
    pub id: i64,
    pub ts: String,
    pub monorepo_root: String,
    pub git_commit: Option<String>,
    pub prograph_version: String,
    pub n_projects: i64,
    pub n_edges: i64,
    pub n_changes: i64,
}

#[pymethods]
impl SnapshotInfo {
    fn __repr__(&self) -> String {
        format!(
            "SnapshotInfo(#{}, ts={}, projects={}, edges={}, changes={})",
            self.id, self.ts, self.n_projects, self.n_edges, self.n_changes
        )
    }
}

/// A persisted contract node — JSON Schema / OpenAPI / .proto file shared across projects.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct Contract {
    pub id: i64,
    /// Optional declared identifier (JSON Schema $id, OpenAPI info.title, .proto package).
    pub declared_id: Option<String>,
    pub content_hash: String,
    /// One of "json_schema", "openapi", "proto".
    pub kind: String,
    pub first_seen: i64,
    pub last_seen: i64,
}

#[pymethods]
impl Contract {
    fn __repr__(&self) -> String {
        format!(
            "Contract(id={}, kind={}, declared_id={:?}, hash={}...)",
            self.id,
            self.kind,
            self.declared_id,
            &self.content_hash[..self.content_hash.len().min(8)]
        )
    }
}

/// The summary `prograph index` returns to the caller after running.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct IndexSummary {
    pub snapshot_id: i64,
    pub ts: String,
    pub n_projects: i64,
    pub n_edges: i64,
    pub n_changes: i64,
    pub n_warnings: i64,
    pub duration_ms: i64,
}

#[pymethods]
impl IndexSummary {
    fn __repr__(&self) -> String {
        format!(
            "IndexSummary(snap={}, projects={}, edges={}, changes={}, warnings={}, {}ms)",
            self.snapshot_id,
            self.n_projects,
            self.n_edges,
            self.n_changes,
            self.n_warnings,
            self.duration_ms
        )
    }
}

// --- M5 aggregation pyclasses (read-only views from Store::describe_* methods) ---

/// A single outbound edge as seen from a source project's MD card. `target_name` is the
/// project name for project endpoints, or the contract `declared_id` / hash prefix.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct OutboundEdge {
    pub kind: String,
    pub target_kind: String,
    pub target_name: String,
    pub target_slug: String,
    pub attrs_json: String,
}

#[pymethods]
impl OutboundEdge {
    fn __repr__(&self) -> String {
        format!(
            "OutboundEdge({} → {}:{})",
            self.kind, self.target_kind, self.target_name
        )
    }
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct InboundEdge {
    pub kind: String,
    pub source_name: String,
    pub source_slug: String,
    pub attrs_json: String,
}

#[pymethods]
impl InboundEdge {
    fn __repr__(&self) -> String {
        format!("InboundEdge({} ← {})", self.kind, self.source_name)
    }
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct McpToolDeclRow {
    pub tool_name: String,
    pub rel_path: String,
    pub line: i64,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct ContractFileRow {
    pub contract_declared_id: Option<String>,
    pub contract_slug: String,
    pub contract_kind: String,
    pub rel_path: String,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct RecentChangeRow {
    pub snapshot_id: i64,
    pub ts: String,
    pub change: String,
    pub summary: String,
}

/// Bundle of everything the renderer needs for one project's MD file.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct ProjectDescription {
    pub project_id: i64,
    pub name: String,
    pub slug: String,
    pub kind: String,
    pub root_path: String,
    pub attrs_json: String,
    pub snapshot_id: i64,
    pub snapshot_ts: String,
    pub mcp_decls: Vec<McpToolDeclRow>,
    pub contract_files: Vec<ContractFileRow>,
    pub outbound: Vec<OutboundEdge>,
    pub inbound: Vec<InboundEdge>,
    pub recent_changes: Vec<RecentChangeRow>,
    /// M9: per-file module facts.
    pub modules: Vec<ModuleRow>,
    pub public_symbols: Vec<PublicSymbolRow>,
    pub internal_imports: Vec<InternalImportRow>,
    /// M10: cross-project refs INTO this project.
    pub inbound_refs: Vec<SymbolRefRow>,
    /// M10: cross-project refs FROM this project.
    pub outbound_refs: Vec<SymbolRefRow>,
    /// M11: drift findings for this project at the current snapshot.
    pub drifts: Vec<DriftFindingRow>,
}

#[pymethods]
impl ProjectDescription {
    fn __repr__(&self) -> String {
        format!(
            "ProjectDescription(name={}, kind={}, outbound={}, inbound={})",
            self.name,
            self.kind,
            self.outbound.len(),
            self.inbound.len()
        )
    }
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct ContractOwner {
    pub project_name: String,
    pub project_slug: String,
    pub rel_path: String,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct ContractDescription {
    pub contract_id: i64,
    pub declared_id: Option<String>,
    pub slug: String,
    pub kind: String,
    pub content_hash: String,
    pub snapshot_id: i64,
    pub snapshot_ts: String,
    pub owners: Vec<ContractOwner>,
    pub recent_changes: Vec<RecentChangeRow>,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct ProjectSummary {
    pub name: String,
    pub slug: String,
    pub kind: String,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct ContractSummary {
    pub slug: String,
    pub declared_id: Option<String>,
    pub kind: String,
    pub n_owners: i64,
}

#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct MonorepoOverview {
    pub monorepo_root: String,
    pub snapshot_id: i64,
    pub snapshot_ts: String,
    pub n_projects: i64,
    pub n_contracts: i64,
    pub n_edges: i64,
    pub projects: Vec<ProjectSummary>,
    pub contracts: Vec<ContractSummary>,
    pub recent_changes: Vec<RecentChangeRow>,
}

// --- M7 pyclasses for MCP query helpers ---

/// One edge row as returned by `find_edges_filtered` — denormalised with target/source names.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct EdgeRow {
    pub id: i64,
    pub kind: String,
    pub from_kind: String,
    pub from_id: i64,
    pub from_name: String,
    pub to_kind: String,
    pub to_id: i64,
    pub to_name: String,
    pub attrs_json: String,
    pub first_seen: i64,
    pub last_seen: i64,
}

#[pymethods]
impl EdgeRow {
    fn __repr__(&self) -> String {
        format!(
            "EdgeRow({}: {} → {})",
            self.kind, self.from_name, self.to_name
        )
    }
}

/// One edge_evidence row.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct EdgeEvidenceRow {
    pub edge_id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub rel_path: String,
    pub line: i64,
    pub snippet: Option<String>,
}

/// One search FTS hit.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct SearchHit {
    pub entity_kind: String, // 'project' | 'contract'
    pub entity_id: i64,
    pub name: String,
    pub snippet: String, // FTS snippet with markers around matched terms
    pub rank: f64,
}

/// Edge row enriched with diff status relative to a `since` snapshot.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct DiffEdgeRow {
    pub id: i64,
    pub kind: String,
    pub from_kind: String,
    pub from_id: i64,
    pub from_name: String,
    pub to_kind: String,
    pub to_id: i64,
    pub to_name: String,
    pub attrs_json: String,
    pub first_seen: i64,
    pub last_seen: i64,
    /// "added" | "removed" | "unchanged"
    pub status: String,
}

#[pymethods]
impl DiffEdgeRow {
    fn __repr__(&self) -> String {
        format!(
            "DiffEdgeRow({} {} → {}: {})",
            self.kind, self.from_name, self.to_name, self.status
        )
    }
}

/// M9: one source file (module) belonging to a project.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct ModuleRow {
    pub id: i64,
    pub rel_path: String,
    pub language: String,
}

/// M9: one public symbol exposed by a module — denormalised with module rel_path.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct PublicSymbolRow {
    pub module_id: i64,
    pub rel_path: String,
    pub name: String,
    pub kind: String,
    pub line: i64,
}

/// M9: one internal import within a module — denormalised with module rel_path.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct InternalImportRow {
    pub module_id: i64,
    pub rel_path: String,
    pub target_path: String,
    pub line: i64,
}

/// M10: one cross-project symbol reference row, joined with project names +
/// the source module's rel_path.
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct SymbolRefRow {
    pub from_project_name: String,
    pub from_module_rel_path: String,
    pub line: i64,
    pub to_project_name: String,
    pub to_module_path: String,
    pub to_symbol_name: Option<String>,
}

#[pymethods]
impl SymbolRefRow {
    fn __repr__(&self) -> String {
        format!(
            "SymbolRefRow({}/{}:{} → {}::{}::{})",
            self.from_project_name,
            self.from_module_rel_path,
            self.line,
            self.to_project_name,
            self.to_module_path,
            self.to_symbol_name.as_deref().unwrap_or("(module)"),
        )
    }
}

/// M11: one drift finding row (joined with project name).
#[derive(Debug, Clone)]
#[pyclass(frozen, module = "prograph._core", get_all, skip_from_py_object)]
pub struct DriftFindingRow {
    pub project_name: String,
    pub kind: String,
    pub entity_kind: String,
    pub entity_name: String,
    pub source_path: String,
    pub source_line: i64,
    pub confidence: String,
    pub detail: Option<String>,
}

#[pymethods]
impl DriftFindingRow {
    fn __repr__(&self) -> String {
        format!(
            "DriftFindingRow({}/{}/{}::{} @ {}:{})",
            self.project_name,
            self.kind,
            self.entity_kind,
            self.entity_name,
            self.source_path,
            self.source_line
        )
    }
}
