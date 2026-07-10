//! MCP detector — matches McpClientUse.tool_name against McpToolDecl.tool_name across
//! projects and emits mcp_call EdgeCandidates.

use std::collections::HashMap;

use super::{edge_attrs_hash, EdgeCandidate};
use crate::facts::ProjectFacts;
use crate::models::{EdgeKind, NodeKind};

pub fn detect(facts: &[ProjectFacts]) -> Vec<EdgeCandidate> {
    // tool_name -> project_idx that declares it (first writer wins).
    let mut servers: HashMap<&str, usize> = HashMap::new();
    for (idx, p) in facts.iter().enumerate() {
        for decl in &p.mcp_decls {
            servers.entry(decl.tool_name.as_str()).or_insert(idx);
        }
    }

    // (consumer_idx, server_idx, tool_name) -> single EdgeCandidate. Each call site appends
    // to the candidate's evidence vec rather than spawning a new edge (M7).
    let mut seen: HashMap<(usize, usize, String), EdgeCandidate> = HashMap::new();

    for (consumer_idx, consumer) in facts.iter().enumerate() {
        for use_site in &consumer.mcp_uses {
            let Some(&server_idx) = servers.get(use_site.tool_name.as_str()) else {
                continue; // unknown tool, external
            };
            if server_idx == consumer_idx {
                continue; // self-call
            }

            let key = (consumer_idx, server_idx, use_site.tool_name.clone());
            let evidence = super::EvidenceLocation {
                project_idx: consumer_idx,
                rel_path: use_site.rel_path.clone(),
                line: use_site.line as i64,
                snippet: None,
            };

            match seen.entry(key) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    e.get_mut().evidence.push(evidence);
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    let attrs = serde_json::json!({
                        "tool": use_site.tool_name,
                    });
                    let attrs_json = serde_json::to_string(&attrs).unwrap();
                    let attrs_hash = edge_attrs_hash("mcp_call", &use_site.tool_name);

                    v.insert(EdgeCandidate {
                        kind: EdgeKind::McpCall,
                        from_kind: NodeKind::Project,
                        from_idx: consumer_idx,
                        to_kind: NodeKind::Project,
                        to_idx: server_idx,
                        attrs_json,
                        attrs_hash,
                        evidence: vec![evidence],
                    });
                }
            }
        }
    }

    let mut out: Vec<EdgeCandidate> = seen.into_values().collect();
    // Deterministically sort evidence inside each candidate (rel_path, line).
    for cand in out.iter_mut() {
        cand.evidence
            .sort_by(|a, b| (a.rel_path.as_str(), a.line).cmp(&(b.rel_path.as_str(), b.line)));
    }
    out.sort_by(|a, b| {
        (a.from_idx, a.to_idx, &a.attrs_hash).cmp(&(b.from_idx, b.to_idx, &b.attrs_hash))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{McpClientUse, McpToolDecl, ParseStatus, ProjectFacts};

    fn fact(name: &str, decls: &[&str], uses: &[&str]) -> ProjectFacts {
        ProjectFacts {
            project_root: format!("./{name}"),
            project_name: name.to_string(),
            manifest: None,
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: decls
                .iter()
                .map(|n| McpToolDecl {
                    tool_name: n.to_string(),
                    rel_path: "src/lib.rs".into(),
                    line: 1,
                })
                .collect(),
            mcp_uses: uses
                .iter()
                .map(|n| McpClientUse {
                    tool_name: n.to_string(),
                    rel_path: "src/lib.rs".into(),
                    line: 1,
                })
                .collect(),
            contracts: vec![],
            modules: vec![],
            intent: Default::default(),
            declared_paths: vec![],
        }
    }

    #[test]
    fn matches_client_to_server_by_tool_name() {
        let facts = vec![
            fact("client", &[], &["decide"]),
            fact("server", &["decide"], &[]),
        ];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_idx, 0);
        assert_eq!(edges[0].to_idx, 1);
        assert_eq!(edges[0].kind, EdgeKind::McpCall);
    }

    #[test]
    fn skips_unknown_tools() {
        let facts = vec![
            fact("client", &[], &["unknown_tool"]),
            fact("server", &["decide"], &[]),
        ];
        let edges = detect(&facts);
        assert!(edges.is_empty());
    }

    #[test]
    fn skips_self_calls() {
        let facts = vec![fact("self", &["decide"], &["decide"])];
        let edges = detect(&facts);
        assert!(edges.is_empty());
    }

    #[test]
    fn dedupes_multiple_call_sites_into_evidence() {
        // Construct a client with three call sites on the same tool.
        let mut consumer = fact("client", &[], &[]);
        consumer.mcp_uses = vec![
            McpClientUse {
                tool_name: "decide".into(),
                rel_path: "a.py".into(),
                line: 10,
            },
            McpClientUse {
                tool_name: "decide".into(),
                rel_path: "a.py".into(),
                line: 20,
            },
            McpClientUse {
                tool_name: "decide".into(),
                rel_path: "b.py".into(),
                line: 5,
            },
        ];
        let facts = vec![consumer, fact("server", &["decide"], &[])];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1, "three call sites → one edge");
        assert_eq!(
            edges[0].evidence.len(),
            3,
            "all three sites should land in evidence"
        );
        // Verify sort order: rel_path then line.
        assert_eq!(edges[0].evidence[0].rel_path, "a.py");
        assert_eq!(edges[0].evidence[0].line, 10);
        assert_eq!(edges[0].evidence[1].line, 20);
        assert_eq!(edges[0].evidence[2].rel_path, "b.py");
    }

    #[test]
    fn identity_hash_includes_tool_name() {
        let e1 = &detect(&[fact("c", &[], &["alpha"]), fact("s", &["alpha"], &[])])[0];
        let e2 = &detect(&[fact("c", &[], &["beta"]), fact("s", &["beta"], &[])])[0];
        assert_ne!(e1.attrs_hash, e2.attrs_hash);
    }
}
