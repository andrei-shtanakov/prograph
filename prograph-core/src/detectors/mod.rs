//! Edge detectors — turn `ProjectFacts[]` into edge candidates.
//! M4 ships `deps`, `contracts`, and `mcp`. M12 adds `declared`.

pub mod contracts;
pub mod declared;
pub mod deps;
pub mod mcp;

use sha2::{Digest, Sha256};

use crate::facts::ProjectFacts;
use crate::models::{EdgeKind, NodeKind};

/// Shared identity-hash scheme for `EdgeCandidate.attrs_hash`: sha256 of
/// `"{kind_prefix}|{identity}"`, where `identity` is the identity-bearing
/// subset of attrs (per spec §5.2) — NOT the full attrs_json unless that's
/// exactly the identity-bearing subset.
pub(crate) fn edge_attrs_hash(kind_prefix: &str, identity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind_prefix.as_bytes());
    hasher.update(b"|");
    hasher.update(identity.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// A detector's pre-persistence edge proposal. The indexer assigns DB-level ids
/// when materializing into the `edges` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeCandidate {
    pub kind: EdgeKind,
    pub from_kind: NodeKind,
    /// Index into the `Vec<ProjectFacts>` (or `Vec<ContractCandidate>` for contract_link
    /// targets) passed to the detector. Resolved to a DB id by the indexer's persist phase.
    pub from_idx: usize,
    pub to_kind: NodeKind,
    pub to_idx: usize,
    /// Full attrs payload (e.g. {"dep_name": "...", "version_req": "..."}).
    pub attrs_json: String,
    /// Hash over identity-bearing attrs only (per spec §5.2).
    pub attrs_hash: String,
    /// Source-line locations that justify this edge. Empty for kinds where we
    /// don't yet track evidence (package_dep, contract_link in M7).
    pub evidence: Vec<EvidenceLocation>,
}

/// One call-site / file-path location that justifies an edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceLocation {
    /// Index into the `Vec<ProjectFacts>` — resolved to a project_id at persist time.
    pub project_idx: usize,
    pub rel_path: String,
    pub line: i64,
    pub snippet: Option<String>,
}

/// A contract candidate produced by `contracts::detect`. The indexer assigns the DB id
/// when materializing into the `contracts` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractCandidate {
    pub kind: crate::facts::ContractKind,
    pub declared_id: Option<String>,
    pub content_hash: String,
    /// Per-project file occurrences: (project_idx, rel_path).
    pub files: Vec<(usize, String)>,
}

/// Detector outputs aggregated for the indexer.
#[derive(Debug, Default)]
pub struct DetectionResult {
    pub edges: Vec<EdgeCandidate>,
    pub contracts: Vec<ContractCandidate>,
    /// Human-readable warnings surfaced by detectors (e.g. rejected/unresolved
    /// declared paths). Empty from the existing deps/contracts/mcp detectors;
    /// `declared::detect_declared` warnings are merged separately by the indexer.
    pub warnings: Vec<String>,
}

/// Run every detector against the project facts and return the aggregated result.
pub fn detect_all(facts: &[ProjectFacts]) -> DetectionResult {
    let mut result = DetectionResult::default();
    result.edges.extend(deps::detect(facts));
    let (cc, ce) = contracts::detect(facts);
    result.contracts.extend(cc);
    result.edges.extend(ce);
    result.edges.extend(mcp::detect(facts));
    result
}
