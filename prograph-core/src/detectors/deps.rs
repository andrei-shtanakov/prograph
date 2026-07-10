//! Package-dependency detector — matches consumers' `declared_deps[].name`
//! against publishers' `Manifest.declared_name` OR any entry in `Manifest.aliases`.

use std::cell::RefCell;
use std::collections::HashMap;

use super::{edge_attrs_hash, EdgeCandidate};
use crate::facts::ProjectFacts;
use crate::models::{EdgeKind, NodeKind};

pub fn detect(facts: &[ProjectFacts]) -> Vec<EdgeCandidate> {
    // Build name → publisher index map, registering declared_name and every alias.
    // On name collision, log a warning to the thread-local sink and keep the FIRST
    // registration (deterministic + ordering-stable).
    let mut publishers: HashMap<&str, usize> = HashMap::new();
    let mut collisions: Vec<String> = Vec::new();

    for (idx, p) in facts.iter().enumerate() {
        let Some(m) = &p.manifest else { continue };

        let mut names_for_this: Vec<&str> = Vec::with_capacity(1 + m.aliases.len());
        names_for_this.push(m.declared_name.as_str());
        for alias in &m.aliases {
            names_for_this.push(alias.as_str());
        }

        for name in names_for_this {
            match publishers.entry(name) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(idx);
                }
                std::collections::hash_map::Entry::Occupied(_existing) => {
                    collisions.push(format!(
                        "name '{}' published by multiple projects (kept first)",
                        name
                    ));
                }
            }
        }
    }

    let mut out = Vec::new();
    for (consumer_idx, consumer) in facts.iter().enumerate() {
        let Some(consumer_manifest) = &consumer.manifest else {
            continue;
        };
        for dep in &consumer_manifest.declared_deps {
            let Some(&publisher_idx) = publishers.get(dep.name.as_str()) else {
                continue; // external dep, not in monorepo
            };
            if publisher_idx == consumer_idx {
                continue; // self-dep (shouldn't happen in practice but guard anyway)
            }

            let attrs = serde_json::json!({
                "dep_name": dep.name,
                "version_req": dep.version_req,
            });
            let attrs_json = serde_json::to_string(&attrs).unwrap();

            // Identity hash covers ONLY identity-bearing fields per spec §5.2:
            // for package_dep, that's `dep_name` (version_req is metadata).
            let attrs_hash = edge_attrs_hash("package_dep", &dep.name);

            out.push(EdgeCandidate {
                kind: EdgeKind::PackageDep,
                from_kind: NodeKind::Project,
                from_idx: consumer_idx,
                to_kind: NodeKind::Project,
                to_idx: publisher_idx,
                attrs_json,
                attrs_hash,
                evidence: vec![super::EvidenceLocation {
                    project_idx: consumer_idx,
                    rel_path: guess_manifest_path(&consumer.project_root).into(),
                    line: 1,
                    snippet: Some(format!("declared {}", dep.name)),
                }],
            });
        }
    }
    out.sort_by(|a, b| {
        (a.from_idx, a.to_idx, &a.attrs_hash).cmp(&(b.from_idx, b.to_idx, &b.attrs_hash))
    });

    if !collisions.is_empty() {
        COLLISION_WARNINGS.with(|w| {
            w.borrow_mut().extend(collisions);
        });
    }

    out
}

/// Best-effort manifest filename inference. M8 deps_detector only fires for Python
/// projects (Rust/JS deps detection is a future milestone), so we hardcode
/// `pyproject.toml`. M9+ may thread the exact manifest path through `Manifest`.
fn guess_manifest_path(_project_root: &str) -> &'static str {
    "pyproject.toml"
}

thread_local! {
    /// Collisions detected during the last `detect`/`detect_all` invocation on this thread.
    /// The indexer drains this after each call and folds the messages into the
    /// snapshot's warning count. Per-thread isolation keeps cargo test parallelism safe.
    pub static COLLISION_WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Drain and return any collision warnings accumulated on this thread since the last drain.
/// The indexer calls this once per index pipeline run.
pub fn drain_collision_warnings() -> Vec<String> {
    COLLISION_WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{DepRequirement, Manifest, ParseStatus, ProjectFacts};

    fn fact(name: &str, deps: &[(&str, Option<&str>)]) -> ProjectFacts {
        ProjectFacts {
            project_root: format!("./{name}"),
            project_name: name.to_string(),
            manifest: Some(Manifest {
                declared_name: name.to_string(),
                version: Some("1.0".into()),
                declared_deps: deps
                    .iter()
                    .map(|(n, v)| DepRequirement {
                        name: (*n).to_string(),
                        version_req: v.map(String::from),
                    })
                    .collect(),
                aliases: Vec::new(),
            }),
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules: vec![],
            intent: Default::default(),
            declared_paths: vec![],
        }
    }

    fn fact_no_manifest(name: &str) -> ProjectFacts {
        ProjectFacts {
            project_root: format!("./{name}"),
            project_name: name.to_string(),
            manifest: None,
            warnings: vec![],
            parse_status: ParseStatus::Failed,
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules: vec![],
            intent: Default::default(),
            declared_paths: vec![],
        }
    }

    fn fact_with_aliases(
        name: &str,
        aliases: &[&str],
        deps: &[(&str, Option<&str>)],
    ) -> ProjectFacts {
        let base = fact(name, deps);
        ProjectFacts {
            manifest: Some(Manifest {
                aliases: aliases.iter().map(|s| s.to_string()).collect(),
                ..base.manifest.unwrap()
            }),
            ..base
        }
    }

    #[test]
    fn matches_consumer_to_publisher_by_name() {
        let _ = drain_collision_warnings();
        let facts = vec![
            fact("orchestrator", &[("eval-sdk", Some(">=1.0"))]),
            fact("eval-sdk", &[]),
        ];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1);
        let e = &edges[0];
        assert_eq!(e.from_idx, 0);
        assert_eq!(e.to_idx, 1);
        assert!(e.attrs_json.contains("\"dep_name\":\"eval-sdk\""));
        assert!(e.attrs_json.contains("\"version_req\":\">=1.0\""));
        // M8: evidence row points at consumer manifest.
        assert_eq!(e.evidence.len(), 1);
        assert_eq!(e.evidence[0].rel_path, "pyproject.toml");
        assert_eq!(e.evidence[0].line, 1);
        assert_eq!(e.evidence[0].project_idx, 0);
    }

    #[test]
    fn skips_external_deps() {
        let _ = drain_collision_warnings();
        let facts = vec![
            fact("orchestrator", &[("eval-sdk", None), ("httpx", None)]),
            fact("eval-sdk", &[]),
        ];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1, "only eval-sdk is in-monorepo");
    }

    #[test]
    fn skips_projects_without_manifest() {
        let _ = drain_collision_warnings();
        let facts = vec![
            fact("orchestrator", &[("eval-sdk", None)]),
            fact_no_manifest("docs_only"),
            fact("eval-sdk", &[]),
        ];
        let edges = detect(&facts);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to_idx, 2); // eval-sdk
    }

    #[test]
    fn identity_hash_excludes_version_req() {
        let _ = drain_collision_warnings();
        let v1 = detect(&[fact("a", &[("b", Some(">=1.0"))]), fact("b", &[])]);
        let v2 = detect(&[fact("a", &[("b", Some(">=2.0"))]), fact("b", &[])]);
        assert_eq!(
            v1[0].attrs_hash, v2[0].attrs_hash,
            "version_req must NOT be part of identity (spec §5.2)"
        );
        assert_ne!(
            v1[0].attrs_json, v2[0].attrs_json,
            "but attrs_json DOES capture the change for change-log"
        );
    }

    #[test]
    fn deterministic_ordering() {
        let _ = drain_collision_warnings();
        let facts = vec![
            fact("a", &[("c", None), ("b", None)]),
            fact("b", &[]),
            fact("c", &[]),
        ];
        let edges1 = detect(&facts);
        let edges2 = detect(&facts);
        let keys1: Vec<_> = edges1.iter().map(|e| (e.from_idx, e.to_idx)).collect();
        let keys2: Vec<_> = edges2.iter().map(|e| (e.from_idx, e.to_idx)).collect();
        assert_eq!(keys1, keys2);
    }

    #[test]
    fn handles_no_matches() {
        let _ = drain_collision_warnings();
        let facts = vec![fact("a", &[("external", None)])];
        assert_eq!(detect(&facts).len(), 0);
    }

    #[test]
    fn alias_matches_consumer_to_publisher() {
        let _ = drain_collision_warnings();
        let facts = vec![
            fact("consumer", &[("atp-platform-sdk", Some(">=2.0"))]),
            fact_with_aliases("atp-platform", &["atp-platform-sdk"], &[]),
        ];
        let edges = detect(&facts);
        assert_eq!(
            edges.len(),
            1,
            "expected consumer -> atp-platform via alias match"
        );
        assert_eq!(edges[0].from_idx, 0);
        assert_eq!(edges[0].to_idx, 1);
    }

    #[test]
    fn name_collision_emits_warning_and_keeps_first() {
        let _ = drain_collision_warnings();
        let facts = vec![
            fact("first-publisher", &[]),
            fact_with_aliases("second-publisher", &["first-publisher"], &[]),
            fact("consumer", &[("first-publisher", None)]),
        ];
        let edges = detect(&facts);
        let warnings = drain_collision_warnings();
        assert!(
            !warnings.is_empty(),
            "expected at least one collision warning"
        );
        assert!(warnings.iter().any(|w| w.contains("first-publisher")));
        // The collision keeps the FIRST registration -> consumer's edge goes to facts[0].
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].to_idx, 0);
    }

    #[test]
    fn drain_collision_warnings_is_idempotent_after_drain() {
        let _ = drain_collision_warnings();
        let facts = vec![fact("a", &[]), fact_with_aliases("b", &["a"], &[])];
        let _ = detect(&facts);
        let first = drain_collision_warnings();
        assert!(!first.is_empty());
        let second = drain_collision_warnings();
        assert!(second.is_empty(), "drain should empty the thread-local");
    }
}
