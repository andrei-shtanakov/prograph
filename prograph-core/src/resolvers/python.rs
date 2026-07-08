//! Python resolver — dotted-name target → publisher project + sub-module path.

use std::collections::HashMap;

use super::ResolvedRef;
use crate::facts::ProjectFacts;

/// Resolve every external Python import in `facts` against the publisher index.
/// Returns one `ResolvedRef` per (line, target_path, target_symbol) that maps
/// to an in-monorepo publisher.
pub fn resolve(facts: &[ProjectFacts], publishers: &HashMap<String, usize>) -> Vec<ResolvedRef> {
    let mut out = Vec::new();

    for (from_idx, p) in facts.iter().enumerate() {
        for module in &p.modules {
            if module.language != "python" {
                continue;
            }
            for ext in &module.external_imports {
                let Some(to_idx) = resolve_dotted(&ext.target_path, publishers) else {
                    continue;
                };
                if to_idx == from_idx {
                    continue;
                }
                let to_module_path = strip_publisher_prefix(&ext.target_path, &facts[to_idx]);

                out.push(ResolvedRef {
                    from_project_idx: from_idx,
                    from_module_rel_path: module.rel_path.clone(),
                    from_line: ext.line,
                    to_project_idx: to_idx,
                    to_module_path,
                    to_symbol_name: ext.target_symbol.clone(),
                });
            }
        }
    }

    out.sort_by(|a, b| {
        (a.from_project_idx, &a.from_module_rel_path, a.from_line).cmp(&(
            b.from_project_idx,
            &b.from_module_rel_path,
            b.from_line,
        ))
    });
    out
}

/// Find the publisher idx for an external import target. Splits the path on
/// '.', then looks up progressively shorter prefixes (longest-first) until a
/// hit.
fn resolve_dotted(target: &str, publishers: &HashMap<String, usize>) -> Option<usize> {
    let parts: Vec<&str> = target.split('.').collect();
    for prefix_len in (1..=parts.len()).rev() {
        let candidate = parts[..prefix_len].join(".");
        if let Some(&idx) = publishers.get(&candidate) {
            return Some(idx);
        }
        // Also try the dashed form (publisher might be `atp-platform`).
        let dashed = candidate.replace('_', "-");
        if let Some(&idx) = publishers.get(&dashed) {
            return Some(idx);
        }
    }
    None
}

/// Strip the publisher's package prefix from the dotted target path. Returns
/// the remainder (which is the module path INSIDE the publisher).
fn strip_publisher_prefix(target: &str, publisher: &ProjectFacts) -> String {
    let Some(manifest) = &publisher.manifest else {
        return target.to_string();
    };
    let mut names: Vec<String> = Vec::new();
    names.push(manifest.declared_name.clone());
    names.extend(manifest.aliases.iter().cloned());
    names.push(manifest.declared_name.replace('-', "_"));
    names.extend(manifest.aliases.iter().map(|a| a.replace('-', "_")));
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));

    for name in &names {
        if target == name {
            return String::new();
        }
        let with_dot = format!("{}.", name);
        if let Some(rest) = target.strip_prefix(&with_dot) {
            return rest.to_string();
        }
    }
    target.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{ExternalImport, Manifest, Module, ParseStatus};

    fn proj(name: &str, modules: Vec<Module>) -> ProjectFacts {
        ProjectFacts {
            project_root: format!("./{name}"),
            project_name: name.to_string(),
            manifest: Some(Manifest {
                declared_name: name.to_string(),
                version: None,
                declared_deps: vec![],
                aliases: vec![],
            }),
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules,
            intent: Default::default(),
        }
    }

    fn module(rel_path: &str, externals: Vec<ExternalImport>) -> Module {
        Module {
            rel_path: rel_path.to_string(),
            language: "python".into(),
            public_symbols: vec![],
            internal_imports: vec![],
            external_imports: externals,
        }
    }

    #[test]
    fn resolves_dotted_to_publisher() {
        let facts = vec![
            proj(
                "consumer",
                vec![module(
                    "api.py",
                    vec![ExternalImport {
                        target_path: "atp_platform.sdk".into(),
                        target_symbol: Some("Client".into()),
                        line: 1,
                    }],
                )],
            ),
            proj("atp_platform", vec![]),
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].from_project_idx, 0);
        assert_eq!(refs[0].to_project_idx, 1);
        assert_eq!(refs[0].to_module_path, "sdk");
        assert_eq!(refs[0].to_symbol_name.as_deref(), Some("Client"));
    }

    #[test]
    fn resolves_via_alias_with_dash_underscore() {
        let mut atp = proj("atp-platform", vec![]);
        if let Some(m) = &mut atp.manifest {
            m.aliases.push("atp-platform-sdk".to_string());
        }
        let facts = vec![
            proj(
                "consumer",
                vec![module(
                    "api.py",
                    vec![ExternalImport {
                        target_path: "atp_platform_sdk.client".into(),
                        target_symbol: None,
                        line: 5,
                    }],
                )],
            ),
            atp,
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].to_module_path, "client");
    }

    #[test]
    fn drops_stdlib_imports() {
        let facts = vec![proj(
            "consumer",
            vec![module(
                "api.py",
                vec![ExternalImport {
                    target_path: "os.path".into(),
                    target_symbol: Some("join".into()),
                    line: 1,
                }],
            )],
        )];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert!(refs.is_empty());
    }

    #[test]
    fn module_path_is_empty_when_import_targets_top_level() {
        let facts = vec![
            proj(
                "consumer",
                vec![module(
                    "api.py",
                    vec![ExternalImport {
                        target_path: "atp_platform".into(),
                        target_symbol: Some("Client".into()),
                        line: 1,
                    }],
                )],
            ),
            proj("atp_platform", vec![]),
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].to_module_path, "");
    }
}
