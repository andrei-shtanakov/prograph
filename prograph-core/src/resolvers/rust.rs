//! Rust resolver — `use foo::a::b::Baz` → publisher project "foo" +
//! inside-crate path "a::b" + symbol "Baz".

use std::collections::HashMap;

use super::ResolvedRef;
use crate::facts::ProjectFacts;

pub fn resolve(facts: &[ProjectFacts], publishers: &HashMap<String, usize>) -> Vec<ResolvedRef> {
    let mut out = Vec::new();

    for (from_idx, p) in facts.iter().enumerate() {
        for module in &p.modules {
            if module.language != "rust" {
                continue;
            }
            for ext in &module.external_imports {
                // Root segment is the crate name.
                let (root, rest) = match ext.target_path.split_once("::") {
                    Some((r, rest)) => (r.to_string(), rest.to_string()),
                    None => (ext.target_path.clone(), String::new()),
                };

                let Some(&to_idx) = publishers.get(&root) else {
                    continue;
                };
                if to_idx == from_idx {
                    continue;
                }

                // For Rust, `rest` is the path inside the crate. If
                // target_symbol is set the parser stripped it from the path;
                // `rest` is therefore the containing module path already.
                let to_module_path = if ext.target_symbol.is_some() {
                    // Parser already trimmed the final symbol off — but in
                    // case it didn't (e.g. `use foo::a::b::Baz` where Baz IS
                    // the symbol AND IS the last segment of target_path), peel
                    // the trailing segment off if it matches the symbol.
                    if let Some(sym) = ext.target_symbol.as_deref() {
                        if rest == sym {
                            String::new()
                        } else if let Some(prefix) = rest.strip_suffix(&format!("::{}", sym)) {
                            prefix.to_string()
                        } else {
                            rest.clone()
                        }
                    } else {
                        rest.clone()
                    }
                } else {
                    rest.clone()
                };

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
            declared_paths: vec![],
        }
    }

    fn module(rel_path: &str, externals: Vec<ExternalImport>) -> Module {
        Module {
            rel_path: rel_path.to_string(),
            language: "rust".into(),
            public_symbols: vec![],
            internal_imports: vec![],
            external_imports: externals,
        }
    }

    #[test]
    fn resolves_rust_use_via_crate_name() {
        let facts = vec![
            proj(
                "consumer",
                vec![module(
                    "src/lib.rs",
                    vec![ExternalImport {
                        target_path: "atp_platform_sdk::client".into(),
                        target_symbol: Some("Client".into()),
                        line: 3,
                    }],
                )],
            ),
            proj("atp_platform_sdk", vec![]),
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].to_module_path, "client");
        assert_eq!(refs[0].to_symbol_name.as_deref(), Some("Client"));
    }

    #[test]
    fn module_path_strips_trailing_symbol() {
        let facts = vec![
            proj(
                "consumer",
                vec![module(
                    "src/lib.rs",
                    vec![ExternalImport {
                        target_path: "atp_platform_sdk::api::v2::Client".into(),
                        target_symbol: Some("Client".into()),
                        line: 1,
                    }],
                )],
            ),
            proj("atp_platform_sdk", vec![]),
        ];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert_eq!(refs[0].to_module_path, "api::v2");
    }

    #[test]
    fn drops_stdlib_uses() {
        let facts = vec![proj(
            "consumer",
            vec![module(
                "src/lib.rs",
                vec![ExternalImport {
                    target_path: "std::collections".into(),
                    target_symbol: Some("HashMap".into()),
                    line: 1,
                }],
            )],
        )];
        let publishers = super::super::build_publisher_index(&facts);
        let refs = resolve(&facts, &publishers);
        assert!(refs.is_empty());
    }
}
