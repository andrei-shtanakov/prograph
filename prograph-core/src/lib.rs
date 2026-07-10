//! prograph-core — Rust core for the prograph monorepo mapper.

mod detectors;
mod diff;
mod discovery;
mod drift;
mod errors;
mod facts;
mod indexer;
mod intent;
mod lock;
mod models;
mod parsers;
mod resolvers;
mod store;

pub use discovery::{classify_project, scan_monorepo};
pub use models::{
    ChangeEvent, ChangeKind, Contract, ContractDescription, ContractFileRow, ContractOwner,
    ContractSummary, DiffEdgeRow, DriftFindingRow, Edge, EdgeEvidenceRow, EdgeKind, EdgeRow,
    EntityKind, InboundEdge, IndexSummary, InternalImportRow, McpToolDeclRow, ModuleRow,
    MonorepoOverview, NodeKind, OutboundEdge, ProjectCandidate, ProjectDescription, ProjectKind,
    ProjectSummary, PublicSymbolRow, RecentChangeRow, SearchHit, SnapshotInfo, SymbolRefRow,
};
pub use store::Store;

use pyo3::prelude::*;

#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Python entry point for `prograph index`.
#[pyfunction]
#[pyo3(name = "index_monorepo", signature = (monorepo_root, db_path, tracked=None))]
fn py_index_monorepo(
    monorepo_root: &str,
    db_path: &str,
    tracked: Option<Vec<String>>,
) -> PyResult<IndexSummary> {
    let mut store = Store::open(std::path::Path::new(db_path))?;
    Ok(indexer::index_monorepo(
        std::path::Path::new(monorepo_root),
        &mut store,
        tracked,
    )?)
}

/// Python entry point: return SnapshotInfo for the latest snapshot, or None.
#[pyfunction]
#[pyo3(name = "latest_snapshot_info")]
fn py_latest_snapshot_info(db_path: &str) -> PyResult<Option<SnapshotInfo>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.latest_snapshot_info()?)
}

#[pyfunction]
#[pyo3(name = "describe_project")]
fn py_describe_project(db_path: &str, project_id: i64) -> PyResult<Option<ProjectDescription>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.describe_project(project_id)?)
}

#[pyfunction]
#[pyo3(name = "describe_contract")]
fn py_describe_contract(db_path: &str, contract_id: i64) -> PyResult<Option<ContractDescription>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.describe_contract(contract_id)?)
}

#[pyfunction]
#[pyo3(name = "monorepo_overview")]
fn py_monorepo_overview(db_path: &str) -> PyResult<Option<MonorepoOverview>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.monorepo_overview()?)
}

#[pyfunction]
#[pyo3(name = "project_by_name")]
fn py_project_by_name(db_path: &str, name: &str) -> PyResult<Option<i64>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.project_by_name(name)?)
}

#[pyfunction]
#[pyo3(name = "snapshot_by_id")]
fn py_snapshot_by_id(db_path: &str, id: i64) -> PyResult<Option<SnapshotInfo>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.snapshot_by_id(id)?)
}

#[pyfunction]
#[pyo3(name = "find_edges_filtered")]
#[pyo3(signature = (db_path, from_name=None, to_name=None, kind=None, since_snapshot=None))]
fn py_find_edges_filtered(
    db_path: &str,
    from_name: Option<&str>,
    to_name: Option<&str>,
    kind: Option<&str>,
    since_snapshot: Option<i64>,
) -> PyResult<Vec<EdgeRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.find_edges_filtered(from_name, to_name, kind, since_snapshot)?)
}

#[pyfunction]
#[pyo3(name = "edge_evidence_for")]
fn py_edge_evidence_for(db_path: &str, edge_id: i64) -> PyResult<Vec<EdgeEvidenceRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.edge_evidence_for(edge_id)?)
}

#[pyfunction]
#[pyo3(name = "find_edges_with_status_since")]
fn py_find_edges_with_status_since(
    db_path: &str,
    since_snapshot: i64,
) -> PyResult<Vec<DiffEdgeRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.find_edges_with_status_since(since_snapshot)?)
}

#[pyfunction]
#[pyo3(name = "search_fts")]
#[pyo3(signature = (db_path, query, kinds=None, limit=20))]
fn py_search_fts(
    db_path: &str,
    query: &str,
    kinds: Option<Vec<String>>,
    limit: i64,
) -> PyResult<Vec<SearchHit>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.search_fts(query, kinds, limit)?)
}

#[pyfunction]
#[pyo3(name = "changelog_paginated")]
#[pyo3(signature = (db_path, since_snapshot=None, entity_kind=None, limit=50))]
fn py_changelog_paginated(
    db_path: &str,
    since_snapshot: Option<i64>,
    entity_kind: Option<&str>,
    limit: i64,
) -> PyResult<Vec<ChangeEvent>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.changelog_paginated(since_snapshot, entity_kind, limit)?)
}

#[pyfunction]
#[pyo3(name = "refs_to_symbol")]
#[pyo3(signature = (db_path, project_name, symbol_name=None))]
fn py_refs_to_symbol(
    db_path: &str,
    project_name: &str,
    symbol_name: Option<&str>,
) -> PyResult<Vec<SymbolRefRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.refs_to_symbol(project_name, symbol_name)?)
}

#[pyfunction]
#[pyo3(name = "refs_from_project")]
fn py_refs_from_project(db_path: &str, project_name: &str) -> PyResult<Vec<SymbolRefRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.refs_from_project(project_name)?)
}

#[pyfunction]
#[pyo3(name = "drifts_for_project")]
fn py_drifts_for_project(db_path: &str, project_name: &str) -> PyResult<Vec<DriftFindingRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.drifts_for_project(project_name)?)
}

#[pyfunction]
#[pyo3(name = "find_drifts_filtered")]
#[pyo3(signature = (db_path, kind=None))]
fn py_find_drifts_filtered(db_path: &str, kind: Option<&str>) -> PyResult<Vec<DriftFindingRow>> {
    let store = Store::open(std::path::Path::new(db_path))?;
    Ok(store.find_drifts_filtered(kind)?)
}

#[pymodule]
fn _core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(discovery::py_scan_monorepo, m)?)?;
    m.add_function(wrap_pyfunction!(discovery::py_tracked_closure, m)?)?;
    m.add_function(wrap_pyfunction!(discovery::py_missing_names, m)?)?;
    m.add_function(wrap_pyfunction!(py_index_monorepo, m)?)?;
    m.add_function(wrap_pyfunction!(py_latest_snapshot_info, m)?)?;
    m.add_function(wrap_pyfunction!(py_describe_project, m)?)?;
    m.add_function(wrap_pyfunction!(py_describe_contract, m)?)?;
    m.add_function(wrap_pyfunction!(py_monorepo_overview, m)?)?;
    m.add_function(wrap_pyfunction!(py_project_by_name, m)?)?;
    m.add_function(wrap_pyfunction!(py_snapshot_by_id, m)?)?;
    m.add_function(wrap_pyfunction!(py_find_edges_filtered, m)?)?;
    m.add_function(wrap_pyfunction!(py_edge_evidence_for, m)?)?;
    m.add_function(wrap_pyfunction!(py_find_edges_with_status_since, m)?)?;
    m.add_function(wrap_pyfunction!(py_search_fts, m)?)?;
    m.add_function(wrap_pyfunction!(py_changelog_paginated, m)?)?;
    m.add_function(wrap_pyfunction!(py_refs_to_symbol, m)?)?;
    m.add_function(wrap_pyfunction!(py_refs_from_project, m)?)?;
    m.add_function(wrap_pyfunction!(py_drifts_for_project, m)?)?;
    m.add_function(wrap_pyfunction!(py_find_drifts_filtered, m)?)?;
    m.add_class::<ProjectKind>()?;
    m.add_class::<ProjectCandidate>()?;
    m.add_class::<NodeKind>()?;
    m.add_class::<EdgeKind>()?;
    m.add_class::<Edge>()?;
    m.add_class::<ChangeKind>()?;
    m.add_class::<EntityKind>()?;
    m.add_class::<ChangeEvent>()?;
    m.add_class::<SnapshotInfo>()?;
    m.add_class::<IndexSummary>()?;
    m.add_class::<Contract>()?;
    m.add_class::<OutboundEdge>()?;
    m.add_class::<InboundEdge>()?;
    m.add_class::<McpToolDeclRow>()?;
    m.add_class::<ContractFileRow>()?;
    m.add_class::<RecentChangeRow>()?;
    m.add_class::<ProjectDescription>()?;
    m.add_class::<ContractOwner>()?;
    m.add_class::<ContractDescription>()?;
    m.add_class::<ProjectSummary>()?;
    m.add_class::<ContractSummary>()?;
    m.add_class::<MonorepoOverview>()?;
    m.add_class::<EdgeRow>()?;
    m.add_class::<EdgeEvidenceRow>()?;
    m.add_class::<SearchHit>()?;
    m.add_class::<DiffEdgeRow>()?;
    m.add_class::<ModuleRow>()?;
    m.add_class::<PublicSymbolRow>()?;
    m.add_class::<InternalImportRow>()?;
    m.add_class::<SymbolRefRow>()?;
    m.add_class::<DriftFindingRow>()?;
    Ok(())
}
