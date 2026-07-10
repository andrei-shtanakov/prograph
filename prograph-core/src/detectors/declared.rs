//! M12: declared edges — file-based integrations declared in manifests.

use std::path::Path;

use crate::detectors::{edge_attrs_hash, EdgeCandidate, EvidenceLocation};
use crate::drift::{Confidence, DriftFinding, DriftKind, EntityKind};
use crate::facts::ProjectFacts;
use crate::models::{EdgeKind, NodeKind};

/// Result of the declared-edges pipeline: edges to persist, stale findings
/// (keyed by declaring-project index), and human-readable warnings.
#[derive(Debug, Default)]
pub struct DeclaredDetection {
    pub edges: Vec<EdgeCandidate>,
    /// (declaring-project index, finding) — the indexer resolves the project id.
    pub stale: Vec<(usize, DriftFinding)>,
    pub warnings: Vec<String>,
}

/// Validate a declared path. Returns the normalized path or a rejection reason.
/// Order matters: reject BEFORE normalizing so `/proctor/x` can't sneak through.
fn validate(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Err("empty path".into());
    }
    if path.contains('\\') {
        return Err("backslash separators are not supported (use '/')".into());
    }
    if path.starts_with('/') {
        return Err("absolute paths are not allowed".into());
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("absolute (drive-letter) paths are not allowed".into());
    }
    if path.split('/').any(|seg| seg == "..") {
        return Err("`..` segments are not allowed".into());
    }
    let p = path.strip_prefix("./").unwrap_or(path);
    Ok(p.trim_end_matches('/').to_string())
}

/// Segment-aware "is `root` a path-prefix of `path`". Roots come from
/// ProjectFacts.project_root ("./proctor") — normalize the "./" off first.
fn is_segment_prefix(root: &str, path: &str) -> bool {
    path == root || path.starts_with(&format!("{root}/"))
}

/// Longest segment-aware match among project roots. Returns the facts index.
fn resolve_target(facts: &[ProjectFacts], path: &str) -> Option<usize> {
    facts
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let root = f.project_root.strip_prefix("./").unwrap_or(&f.project_root);
            is_segment_prefix(root, path).then_some((i, root.len()))
        })
        .max_by_key(|&(_, len)| len)
        .map(|(i, _)| i)
}

/// Validate + resolve every declared path in `facts` into edges (to the target
/// project), stale-declaration drift findings (target doesn't exist on disk),
/// and warnings (path rejected or unresolvable).
pub fn detect_declared(facts: &[ProjectFacts], monorepo_root: &Path) -> DeclaredDetection {
    let mut det = DeclaredDetection::default();
    for (from_idx, fact) in facts.iter().enumerate() {
        for d in &fact.declared_paths {
            let norm = match validate(&d.path) {
                Ok(p) => p,
                Err(why) => {
                    det.warnings.push(format!(
                        "{}: declared path '{}' rejected: {}",
                        fact.project_name, d.path, why
                    ));
                    continue;
                }
            };
            let Some(to_idx) = resolve_target(facts, &norm) else {
                det.warnings.push(format!(
                    "{}: declared path '{}' matches no tracked project",
                    fact.project_name, norm
                ));
                continue;
            };
            if to_idx == from_idx {
                det.warnings.push(format!(
                    "{}: declared path '{}' points at the declaring project itself",
                    fact.project_name, norm
                ));
                continue;
            }
            let attrs = serde_json::json!({ "mode": d.mode.as_str(), "path": norm });
            let attrs_json = serde_json::to_string(&attrs).unwrap();
            let attrs_hash = edge_attrs_hash("declared", &attrs_json);
            det.edges.push(EdgeCandidate {
                kind: EdgeKind::Declared,
                from_kind: NodeKind::Project,
                from_idx,
                to_kind: NodeKind::Project,
                to_idx,
                attrs_json,
                attrs_hash,
                evidence: vec![EvidenceLocation {
                    project_idx: from_idx,
                    rel_path: d.source_path.clone(),
                    line: d.line as i64,
                    snippet: d.snippet.clone(),
                }],
            });
            // Stale check: the declaration resolved, but does the path exist?
            if !monorepo_root.join(&norm).exists() {
                det.stale.push((
                    from_idx,
                    DriftFinding {
                        kind: DriftKind::StaleDeclaration,
                        entity_kind: EntityKind::DeclaredPath,
                        entity_name: norm.clone(),
                        source_path: d.source_path.clone(),
                        source_line: d.line,
                        confidence: Confidence::High,
                        detail: Some(format!(
                            "declared {} target no longer exists on disk",
                            d.mode.as_str()
                        )),
                    },
                ));
            }
        }
    }
    det
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{DeclaredMode, DeclaredPath, IntentDoc, ParseStatus, ProjectFacts};

    fn fact(root: &str, name: &str, decls: Vec<DeclaredPath>) -> ProjectFacts {
        ProjectFacts {
            project_root: root.to_string(),
            project_name: name.to_string(),
            manifest: None,
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules: vec![],
            intent: IntentDoc::default(),
            declared_paths: decls,
        }
    }

    fn decl(mode: DeclaredMode, path: &str) -> DeclaredPath {
        DeclaredPath {
            mode,
            path: path.to_string(),
            source_path: "pyproject.toml".to_string(),
            line: 7,
            snippet: Some(format!("reads = [\"{path}\"]")),
        }
    }

    fn setup(paths_on_disk: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        for p in paths_on_disk {
            let full = dir.path().join(p);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, "x").unwrap();
        }
        dir
    }

    #[test]
    fn resolves_edge_with_evidence_and_attrs() {
        let dir = setup(&["proctor/data/state.db"]);
        let facts = vec![
            fact(
                "./dispatcher",
                "dispatcher",
                vec![decl(DeclaredMode::Read, "proctor/data/state.db")],
            ),
            fact("./proctor", "proctor", vec![]),
        ];
        let det = detect_declared(&facts, dir.path());
        assert!(det.warnings.is_empty());
        assert!(det.stale.is_empty());
        assert_eq!(det.edges.len(), 1);
        let e = &det.edges[0];
        assert_eq!(e.from_idx, 0);
        assert_eq!(e.to_idx, 1);
        assert!(e.attrs_json.contains("\"mode\":\"read\""));
        assert!(e.attrs_json.contains("proctor/data/state.db"));
        assert_eq!(e.evidence.len(), 1);
        assert_eq!(e.evidence[0].rel_path, "pyproject.toml");
        assert_eq!(e.evidence[0].line, 7);
    }

    #[test]
    fn segment_aware_prefix_rejects_lookalike() {
        let dir = setup(&["proctor2/data/x"]);
        let facts = vec![
            fact(
                "./dispatcher",
                "dispatcher",
                vec![decl(DeclaredMode::Read, "proctor2/data/x")],
            ),
            fact("./proctor", "proctor", vec![]),
        ];
        let det = detect_declared(&facts, dir.path());
        assert!(
            det.edges.is_empty(),
            "proctor2 must not match project proctor"
        );
        assert_eq!(det.warnings.len(), 1);
    }

    #[test]
    fn longest_match_wins_for_nested_members() {
        let dir = setup(&["atp-platform/packages/atp-sdk/x.json"]);
        let facts = vec![
            fact(
                "./maestro",
                "maestro",
                vec![decl(
                    DeclaredMode::Read,
                    "atp-platform/packages/atp-sdk/x.json",
                )],
            ),
            fact("./atp-platform", "atp-platform", vec![]),
            fact("./atp-platform/packages/atp-sdk", "atp-sdk", vec![]),
        ];
        let det = detect_declared(&facts, dir.path());
        assert_eq!(det.edges.len(), 1);
        assert_eq!(det.edges[0].to_idx, 2, "nested member wins over its parent");
    }

    #[test]
    fn invalid_paths_warn_without_edges() {
        let dir = setup(&[]);
        let bad = ["/abs/x", "C:\\win\\x", "a/../b", "with\\backslash", ""];
        let decls = bad.iter().map(|p| decl(DeclaredMode::Read, p)).collect();
        let facts = vec![fact("./d", "d", decls), fact("./a", "a", vec![])];
        let det = detect_declared(&facts, dir.path());
        assert!(det.edges.is_empty());
        assert_eq!(det.warnings.len(), bad.len());
    }

    #[test]
    fn self_reference_warns() {
        let dir = setup(&["d/own.db"]);
        let facts = vec![fact("./d", "d", vec![decl(DeclaredMode::Read, "d/own.db")])];
        let det = detect_declared(&facts, dir.path());
        assert!(det.edges.is_empty());
        assert_eq!(det.warnings.len(), 1);
    }

    #[test]
    fn missing_target_file_is_stale_declaration() {
        let dir = setup(&[]); // nothing on disk
        std::fs::create_dir_all(dir.path().join("proctor")).unwrap();
        let facts = vec![
            fact(
                "./dispatcher",
                "dispatcher",
                vec![decl(DeclaredMode::Read, "proctor/gone.db")],
            ),
            fact("./proctor", "proctor", vec![]),
        ];
        let det = detect_declared(&facts, dir.path());
        assert_eq!(
            det.edges.len(),
            1,
            "edge still emitted — the DECLARATION exists"
        );
        assert_eq!(det.stale.len(), 1);
        let (idx, f) = &det.stale[0];
        assert_eq!(*idx, 0, "stale attributed to the DECLARING project");
        assert_eq!(f.kind, DriftKind::StaleDeclaration);
        assert_eq!(f.entity_name, "proctor/gone.db");
        assert_eq!(f.source_path, "pyproject.toml");
        assert_eq!(f.source_line, 7);
    }

    #[test]
    fn directory_target_with_or_without_trailing_slash_is_not_stale() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("vault/derived")).unwrap();
        for declared in ["vault/derived/", "vault/derived"] {
            let facts = vec![
                fact("./p", "p", vec![decl(DeclaredMode::Write, declared)]),
                fact("./vault", "vault", vec![]),
            ];
            let det = detect_declared(&facts, dir.path());
            assert_eq!(det.edges.len(), 1);
            assert!(
                det.stale.is_empty(),
                "existing dir must not be stale ({declared})"
            );
        }
    }

    #[test]
    fn two_declarations_two_edges_distinct_hashes() {
        let dir = setup(&["proctor/a", "proctor/b"]);
        let facts = vec![
            fact(
                "./d",
                "d",
                vec![
                    decl(DeclaredMode::Read, "proctor/a"),
                    decl(DeclaredMode::Read, "proctor/b"),
                ],
            ),
            fact("./proctor", "proctor", vec![]),
        ];
        let det = detect_declared(&facts, dir.path());
        assert_eq!(det.edges.len(), 2);
        assert_ne!(det.edges[0].attrs_hash, det.edges[1].attrs_hash);
    }
}
