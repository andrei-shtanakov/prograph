//! Contracts detector — groups ContractFile facts by (declared_id, content_hash)
//! into ContractCandidates, then emits contract_link edges from each owning project
//! to the contract.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use super::{ContractCandidate, EdgeCandidate};
use crate::facts::ProjectFacts;
use crate::models::{EdgeKind, NodeKind};

/// Group contract files into candidates and emit contract_link edges.
/// Single-owner contracts ARE returned as candidates (so the contract becomes a graph node)
/// but emit NO contract_link edge — only >=2 owners produces cross-project links.
pub fn detect(facts: &[ProjectFacts]) -> (Vec<ContractCandidate>, Vec<EdgeCandidate>) {
    // Group by (declared_id, content_hash). Deterministic key for stable iteration.
    let mut groups: HashMap<String, ContractCandidate> = HashMap::new();
    for (proj_idx, proj) in facts.iter().enumerate() {
        for cf in &proj.contracts {
            let key = format!(
                "{}|{}",
                cf.declared_id.as_deref().unwrap_or(""),
                cf.content_hash
            );
            let candidate = groups.entry(key).or_insert_with(|| ContractCandidate {
                kind: cf.kind,
                declared_id: cf.declared_id.clone(),
                content_hash: cf.content_hash.clone(),
                files: Vec::new(),
            });
            candidate.files.push((proj_idx, cf.rel_path.clone()));
        }
    }

    let mut contracts: Vec<ContractCandidate> = groups.into_values().collect();
    contracts.sort_by(|a, b| {
        (
            a.declared_id.as_deref().unwrap_or(""),
            a.content_hash.as_str(),
        )
            .cmp(&(
                b.declared_id.as_deref().unwrap_or(""),
                b.content_hash.as_str(),
            ))
    });

    let mut edges = Vec::new();
    for (contract_idx, c) in contracts.iter().enumerate() {
        let mut owners: Vec<usize> = c.files.iter().map(|(p, _)| *p).collect();
        owners.sort();
        owners.dedup();

        if owners.len() < 2 {
            continue;
        }

        for &proj_idx in &owners {
            let attrs = serde_json::json!({
                "contract_kind": c.kind.as_str(),
                "declared_id": c.declared_id,
            });
            let attrs_json = serde_json::to_string(&attrs).unwrap();
            let mut hasher = Sha256::new();
            hasher.update(b"contract_link|");
            hasher.update(c.declared_id.as_deref().unwrap_or("").as_bytes());
            hasher.update(b"|");
            hasher.update(c.content_hash.as_bytes());
            let attrs_hash = format!("{:x}", hasher.finalize());

            let evidence: Vec<super::EvidenceLocation> = c
                .files
                .iter()
                .filter(|(p, _)| *p == proj_idx)
                .map(|(_, rel_path)| super::EvidenceLocation {
                    project_idx: proj_idx,
                    rel_path: rel_path.clone(),
                    line: 1,
                    snippet: None,
                })
                .collect();

            edges.push(EdgeCandidate {
                kind: EdgeKind::ContractLink,
                from_kind: NodeKind::Project,
                from_idx: proj_idx,
                to_kind: NodeKind::Contract,
                to_idx: contract_idx, // INDEX into ContractCandidates, NOT a project idx
                attrs_json,
                attrs_hash,
                evidence,
            });
        }
    }
    edges.sort_by(|a, b| (a.from_idx, a.to_idx).cmp(&(b.from_idx, b.to_idx)));

    (contracts, edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{ContractFile, ContractKind, ParseStatus, ProjectFacts};

    fn fact_with_contracts(name: &str, contracts: Vec<ContractFile>) -> ProjectFacts {
        ProjectFacts {
            project_root: format!("./{name}"),
            project_name: name.to_string(),
            manifest: None,
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts,
            modules: vec![],
            intent: Default::default(),
        }
    }

    fn cf(rel_path: &str, declared_id: Option<&str>, content: &str) -> ContractFile {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let content_hash = format!("{:x}", hasher.finalize());
        ContractFile {
            rel_path: rel_path.into(),
            kind: ContractKind::JsonSchema,
            declared_id: declared_id.map(String::from),
            content_hash,
        }
    }

    #[test]
    fn single_owner_contract_produces_node_no_edge() {
        let facts = vec![fact_with_contracts(
            "a",
            vec![cf("schemas/obs.json", Some("obs-v1"), "{...}")],
        )];
        let (contracts, edges) = detect(&facts);
        assert_eq!(contracts.len(), 1);
        assert!(
            edges.is_empty(),
            "single-owner contract must not produce a link edge"
        );
    }

    #[test]
    fn two_owners_produces_two_link_edges() {
        let facts = vec![
            fact_with_contracts("a", vec![cf("schemas/obs.json", Some("obs-v1"), "{...}")]),
            fact_with_contracts("b", vec![cf("vendor/obs.json", Some("obs-v1"), "{...}")]),
        ];
        let (contracts, edges) = detect(&facts);
        assert_eq!(contracts.len(), 1);
        assert_eq!(edges.len(), 2, "expected one edge per owner");
        let owners: std::collections::HashSet<_> = edges.iter().map(|e| e.from_idx).collect();
        assert_eq!(owners, [0, 1].iter().copied().collect());
        // M8: evidence row per owner referencing that owner's contract file.
        for edge in &edges {
            assert!(!edge.evidence.is_empty(), "expected ≥1 evidence row");
            assert_eq!(edge.evidence[0].project_idx, edge.from_idx);
        }
        let evidence_paths: std::collections::HashSet<_> = edges
            .iter()
            .map(|e| e.evidence[0].rel_path.as_str())
            .collect();
        assert!(evidence_paths.contains("schemas/obs.json"));
        assert!(evidence_paths.contains("vendor/obs.json"));
    }

    #[test]
    fn same_id_different_content_are_different_contracts() {
        let facts = vec![
            fact_with_contracts("a", vec![cf("schemas/obs.json", Some("obs-v1"), "v1")]),
            fact_with_contracts("b", vec![cf("schemas/obs.json", Some("obs-v1"), "v2")]),
        ];
        let (contracts, edges) = detect(&facts);
        assert_eq!(
            contracts.len(),
            2,
            "different content -> different contracts"
        );
        assert!(
            edges.is_empty(),
            "no link because neither contract has >=2 owners"
        );
    }

    #[test]
    fn multiple_files_same_project_dedup_owner() {
        let facts = vec![
            fact_with_contracts(
                "a",
                vec![
                    cf("schemas/obs-1.json", Some("obs-v1"), "{...}"),
                    cf("vendor/obs-2.json", Some("obs-v1"), "{...}"),
                ],
            ),
            fact_with_contracts("b", vec![cf("schemas/obs.json", Some("obs-v1"), "{...}")]),
        ];
        let (contracts, edges) = detect(&facts);
        assert_eq!(contracts.len(), 1);
        // Project a has TWO files but should only contribute ONE link edge.
        assert_eq!(edges.len(), 2);
    }
}
