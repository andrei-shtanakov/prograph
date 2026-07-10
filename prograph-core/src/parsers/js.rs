//! JS / TypeScript project parser — reads `package.json` to extract name + deps.

use std::path::Path;

use serde::Deserialize;
use tree_sitter::{Language, Parser, Query, QueryCursor};
use walkdir::WalkDir;

use super::ParserOutput;
use crate::errors::{PrographError, Result};
use crate::facts::{DepRequirement, Manifest, ParseWarning};

#[derive(Debug, Deserialize)]
struct PackageJson {
    name: Option<String>,
    version: Option<String>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: std::collections::BTreeMap<String, String>,
    #[serde(default, rename = "peerDependencies")]
    peer_dependencies: std::collections::BTreeMap<String, String>,
}

pub fn parse(project_root: &Path) -> Result<ParserOutput> {
    let package_json = project_root.join("package.json");
    if !package_json.is_file() {
        return Ok(ParserOutput {
            manifest: None,
            warnings: vec![ParseWarning {
                rel_path: "package.json".into(),
                message: "no package.json found".into(),
            }],
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules: vec![],
            declared_paths: vec![],
        });
    }

    let contents = std::fs::read_to_string(&package_json).map_err(|source| PrographError::Io {
        path: package_json.display().to_string(),
        source,
    })?;

    let pkg: PackageJson = serde_json::from_str(&contents).map_err(|e| PrographError::Parse {
        path: package_json.display().to_string(),
        reason: e.to_string(),
    })?;

    let declared_name = match pkg.name {
        Some(n) => n,
        None => {
            return Ok(ParserOutput {
                manifest: None,
                warnings: vec![ParseWarning {
                    rel_path: "package.json".into(),
                    message: "package.json missing 'name' key".into(),
                }],
                mcp_decls: vec![],
                mcp_uses: vec![],
                contracts: vec![],
                modules: vec![],
                declared_paths: vec![],
            });
        }
    };

    let mut declared_deps: Vec<DepRequirement> = Vec::new();
    for (name, version) in &pkg.dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: Some(version.clone()),
        });
    }
    for (name, version) in &pkg.dev_dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: Some(version.clone()),
        });
    }
    for (name, version) in &pkg.peer_dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: Some(version.clone()),
        });
    }

    let (modules, module_warnings) = scan_js_modules(project_root);

    Ok(ParserOutput {
        manifest: Some(Manifest {
            declared_name,
            version: pkg.version,
            declared_deps,
            aliases: Vec::new(),
        }),
        warnings: module_warnings,
        mcp_decls: vec![],
        mcp_uses: vec![],
        contracts: vec![],
        modules,
        declared_paths: vec![],
    })
}

/// M9: walk all .js/.ts/.mjs/.cjs/.jsx/.tsx files under `project_root` and extract
/// module-level facts. Public filter is the JS query (matches `export` decls);
/// internal imports are relative (`./...` or `../...`) only.
fn scan_js_modules(project_root: &Path) -> (Vec<crate::facts::Module>, Vec<ParseWarning>) {
    let language: Language = tree_sitter_javascript::language();
    let query_src = include_str!("../ts_queries/js_symbols.scm");

    let query = match Query::new(&language, query_src) {
        Ok(q) => q,
        Err(e) => {
            return (
                vec![],
                vec![ParseWarning {
                    rel_path: "ts_queries/js_symbols.scm".into(),
                    message: format!("failed to compile query: {}", e),
                }],
            );
        }
    };

    let mut modules: Vec<crate::facts::Module> = Vec::new();
    let mut warnings: Vec<ParseWarning> = Vec::new();

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        warnings.push(ParseWarning {
            rel_path: "<tree-sitter init>".into(),
            message: "failed to initialise tree-sitter-javascript".into(),
        });
        return (modules, warnings);
    }

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        e.depth() == 0
            || (!matches!(
                name.as_ref(),
                "node_modules" | "dist" | "build" | ".git" | "target"
            ) && !name.starts_with('.'))
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let ext = entry.path().extension().and_then(|s| s.to_str());
        if !matches!(
            ext,
            Some("js") | Some("mjs") | Some("cjs") | Some("ts") | Some("tsx") | Some("jsx")
        ) {
            continue;
        }

        let rel_path = entry
            .path()
            .strip_prefix(project_root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");

        let source = match std::fs::read_to_string(entry.path()) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let tree = match parser.parse(&source, None) {
            Some(t) => t,
            None => {
                warnings.push(ParseWarning {
                    rel_path: rel_path.clone(),
                    message: "tree-sitter parse failed".into(),
                });
                continue;
            }
        };

        let source_bytes = source.as_bytes();
        let mut cursor = QueryCursor::new();
        let mut public_symbols: Vec<crate::facts::PublicSymbol> = Vec::new();
        let mut internal_imports: Vec<crate::facts::InternalImport> = Vec::new();

        for m in cursor.matches(&query, tree.root_node(), source_bytes) {
            let mut symbol_name: Option<String> = None;
            let mut import_source: Option<String> = None;
            let mut line: u32 = 1;
            let mut kind: Option<crate::facts::SymbolKind> = None;

            for capture in m.captures {
                let cap_name = &query.capture_names()[capture.index as usize];
                let text = capture
                    .node
                    .utf8_text(source_bytes)
                    .unwrap_or("")
                    .to_string();
                line = capture.node.start_position().row as u32 + 1;
                match *cap_name {
                    "symbol_name" => symbol_name = Some(text),
                    "symbol_function_export" => kind = Some(crate::facts::SymbolKind::Function),
                    "symbol_class_export" => kind = Some(crate::facts::SymbolKind::Class),
                    "symbol_const_export" => kind = Some(crate::facts::SymbolKind::Const),
                    "import_source" => {
                        let stripped = text
                            .trim_start_matches(['"', '\''])
                            .trim_end_matches(['"', '\''])
                            .to_string();
                        import_source = Some(stripped);
                    }
                    _ => {}
                }
            }

            if let Some(src) = import_source {
                internal_imports.push(crate::facts::InternalImport {
                    target_path: src,
                    line,
                });
                continue;
            }

            let Some(name) = symbol_name else { continue };
            let kind = kind.unwrap_or(crate::facts::SymbolKind::Function);
            public_symbols.push(crate::facts::PublicSymbol { name, kind, line });
        }

        public_symbols.sort_by(|a, b| (a.line, &a.name).cmp(&(b.line, &b.name)));
        internal_imports.sort_by(|a, b| (a.line, &a.target_path).cmp(&(b.line, &b.target_path)));

        modules.push(crate::facts::Module {
            rel_path,
            language: "js".into(),
            public_symbols,
            internal_imports,
            external_imports: vec![],
        });
    }

    modules.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    (modules, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_package_json(json: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("package.json"), json).unwrap();
        dir
    }

    #[test]
    fn parses_minimal_package_json() {
        let dir = write_package_json(
            r#"{
  "name": "my-app",
  "version": "1.0.0"
}"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        assert_eq!(manifest.declared_name, "my-app");
        assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
        assert!(manifest.declared_deps.is_empty());
    }

    #[test]
    fn parses_dependencies_and_dev_dependencies() {
        let dir = write_package_json(
            r#"{
  "name": "consumer",
  "version": "1.0.0",
  "dependencies": {
    "react": "^18.2.0",
    "lodash": "~4.17.21"
  },
  "devDependencies": {
    "typescript": "5.0.0",
    "vitest": ">=1.0"
  }
}"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let names: std::collections::HashSet<_> = manifest
            .declared_deps
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(names.contains("react"));
        assert!(names.contains("lodash"));
        assert!(names.contains("typescript"));
        assert!(names.contains("vitest"));
        let react_dep = manifest
            .declared_deps
            .iter()
            .find(|d| d.name == "react")
            .unwrap();
        assert_eq!(react_dep.version_req.as_deref(), Some("^18.2.0"));
    }

    #[test]
    fn parses_peer_dependencies() {
        let dir = write_package_json(
            r#"{
  "name": "plugin",
  "peerDependencies": {
    "host-app": "^2.0"
  }
}"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let host_dep = manifest
            .declared_deps
            .iter()
            .find(|d| d.name == "host-app")
            .unwrap();
        assert_eq!(host_dep.version_req.as_deref(), Some("^2.0"));
    }

    #[test]
    fn warns_when_no_package_json() {
        let dir = TempDir::new().unwrap();
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("no package.json"));
    }

    #[test]
    fn warns_when_no_name() {
        let dir = write_package_json(r#"{"version": "1.0"}"#);
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("missing 'name'"));
    }

    #[test]
    fn errors_on_invalid_json() {
        let dir = write_package_json("{not json");
        let err = parse(dir.path()).unwrap_err();
        assert!(err.to_string().contains("parse error"));
    }

    #[test]
    fn scans_js_exports_and_relative_imports() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("package.json"), r#"{"name": "my-pkg"}"#).unwrap();
        fs::write(
            dir.path().join("index.js"),
            r#"import { helper } from './util';
import lodash from 'lodash';

export function publicFn() {}
export class PublicClass {}
export const PUBLIC_CONST = 1;

function privateFn() {}
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out
            .modules
            .iter()
            .find(|m| m.rel_path == "index.js")
            .expect("expected index.js module");
        let names: Vec<_> = module
            .public_symbols
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert!(names.contains(&"publicFn"));
        assert!(names.contains(&"PublicClass"));
        assert!(names.contains(&"PUBLIC_CONST"));
        assert!(
            !names.contains(&"privateFn"),
            "non-export decls filtered: {:?}",
            names
        );

        let imports: Vec<_> = module
            .internal_imports
            .iter()
            .map(|i| i.target_path.as_str())
            .collect();
        assert!(imports.contains(&"./util"));
        assert!(
            !imports.contains(&"lodash"),
            "non-relative imports filtered: {:?}",
            imports
        );
    }
}
