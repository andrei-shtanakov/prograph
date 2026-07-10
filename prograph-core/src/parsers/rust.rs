//! Rust project parser — reads `Cargo.toml` + scans `.rs` files for MCP tool decls/uses.

use std::path::Path;

use serde::Deserialize;
use tree_sitter::{Language, Parser, Query, QueryCursor};
use walkdir::WalkDir;

use super::ParserOutput;
use crate::errors::{PrographError, Result};
use crate::facts::{DepRequirement, Manifest, McpClientUse, McpToolDecl, ParseWarning};

#[derive(Debug, Deserialize)]
struct CargoToml {
    package: Option<CargoPackage>,
    workspace: Option<toml::Value>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, CargoDep>,
    #[serde(default, rename = "dev-dependencies")]
    dev_dependencies: std::collections::BTreeMap<String, CargoDep>,
    #[serde(default, rename = "build-dependencies")]
    build_dependencies: std::collections::BTreeMap<String, CargoDep>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    // `name` and `version` can be either a literal string OR a workspace-inherited
    // form via dotted-key syntax: `version.workspace = true`. Accept toml::Value
    // and extract the string when possible; otherwise leave None (workspace
    // inheritance — prograph doesn't follow the inheritance chain).
    name: Option<toml::Value>,
    version: Option<toml::Value>,
}

impl CargoPackage {
    fn name_str(&self) -> Option<String> {
        self.name
            .as_ref()
            .and_then(|v| v.as_str().map(String::from))
    }
    fn version_str(&self) -> Option<String> {
        self.version
            .as_ref()
            .and_then(|v| v.as_str().map(String::from))
    }
}

/// A single `[dependencies]` entry — short form (`serde = "1.0"`) or table form
/// (`serde = { version = "1.0", features = [...] }`). Also handles workspace
/// inheritance: `serde = { workspace = true }` or `serde.workspace = true`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CargoDep {
    Simple(String),
    Detailed {
        // toml::Value handles both `version = "1.0"` and `version.workspace = true`.
        version: Option<toml::Value>,
    },
}

impl CargoDep {
    fn version_req(&self) -> Option<String> {
        match self {
            CargoDep::Simple(v) => Some(v.clone()),
            CargoDep::Detailed { version } => {
                version.as_ref().and_then(|v| v.as_str().map(String::from))
            }
        }
    }
}

/// Return `true` if the given project's `Cargo.toml` declares a `[workspace]`
/// table. Used by the discovery layer to decide whether to recurse into
/// sub-packages.
pub fn declares_workspace(project_root: &Path) -> bool {
    let cargo_toml = project_root.join("Cargo.toml");
    let Ok(contents) = std::fs::read_to_string(cargo_toml) else {
        return false;
    };
    let Ok(root) = toml::from_str::<CargoToml>(&contents) else {
        return false;
    };
    root.workspace.is_some()
}

/// Return the `[workspace] members` list, if any. Each entry is a glob-or-literal
/// path relative to the project root, e.g. `"crates/*"` or `"arbiter-mcp"`.
/// Empty when no Cargo.toml or no `[workspace]` table.
pub fn workspace_members(project_root: &Path) -> Vec<String> {
    let cargo_toml = project_root.join("Cargo.toml");
    let Ok(contents) = std::fs::read_to_string(cargo_toml) else {
        return Vec::new();
    };
    let Ok(root) = toml::from_str::<CargoToml>(&contents) else {
        return Vec::new();
    };
    let Some(ws) = root.workspace else {
        return Vec::new();
    };
    // [workspace] is parsed as toml::Value; pluck `members` field manually.
    ws.get("members")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse(project_root: &Path) -> Result<ParserOutput> {
    let cargo_toml = project_root.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Ok(ParserOutput {
            manifest: None,
            warnings: vec![ParseWarning {
                rel_path: "Cargo.toml".into(),
                message: "no Cargo.toml found".into(),
            }],
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules: vec![],
            declared_paths: vec![],
        });
    }

    let contents = std::fs::read_to_string(&cargo_toml).map_err(|source| PrographError::Io {
        path: cargo_toml.display().to_string(),
        source,
    })?;

    let root: CargoToml = toml::from_str(&contents).map_err(|e| PrographError::Parse {
        path: cargo_toml.display().to_string(),
        reason: e.to_string(),
    })?;

    // Workspace-only manifest (no [package]) — warn, return no manifest. M3 does NOT
    // auto-scan workspace members; that's a discovery-layer change deferred to M4+.
    if root.package.is_none() {
        let msg = if root.workspace.is_some() {
            "workspace root with no [package] — sub-packages must be scanned separately".into()
        } else {
            "Cargo.toml has no [package] section".into()
        };
        return Ok(ParserOutput {
            manifest: None,
            warnings: vec![ParseWarning {
                rel_path: "Cargo.toml".into(),
                message: msg,
            }],
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules: vec![],
            declared_paths: vec![],
        });
    }
    let package = root.package.unwrap();

    let declared_name = match package.name_str() {
        Some(n) => n,
        None => {
            return Ok(ParserOutput {
                manifest: None,
                warnings: vec![ParseWarning {
                    rel_path: "Cargo.toml".into(),
                    message: "[package] missing or non-literal 'name' key".into(),
                }],
                mcp_decls: vec![],
                mcp_uses: vec![],
                contracts: vec![],
                modules: vec![],
                declared_paths: vec![],
            });
        }
    };
    let declared_version = package.version_str();

    // Flatten all three dep tables. BTreeMap iteration is sorted-by-key, giving stable order.
    let mut declared_deps: Vec<DepRequirement> = Vec::new();
    for (name, dep) in &root.dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: dep.version_req(),
        });
    }
    for (name, dep) in &root.dev_dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: dep.version_req(),
        });
    }
    for (name, dep) in &root.build_dependencies {
        declared_deps.push(DepRequirement {
            name: name.clone(),
            version_req: dep.version_req(),
        });
    }

    let (mcp_decls, mcp_uses, mut all_warnings) = scan_rust_source(project_root);
    let (modules, module_warnings) = scan_rust_modules(project_root);
    all_warnings.extend(module_warnings);

    // M12: `[package.metadata.prograph] reads/writes` — reuse the tolerant
    // extraction shared with the Python parser via the untyped toml::Value path,
    // since `CargoToml` above doesn't model `package.metadata`.
    let raw: toml::Value =
        toml::from_str(&contents).unwrap_or(toml::Value::Table(Default::default()));
    let declared_table = raw
        .get("package")
        .and_then(|p| p.get("metadata"))
        .and_then(|m| m.get("prograph"));
    let declared_paths = super::python::extract_declared_from_table(
        declared_table,
        &contents,
        "Cargo.toml",
        &mut all_warnings,
    );

    Ok(ParserOutput {
        manifest: Some(Manifest {
            declared_name,
            version: declared_version,
            declared_deps,
            aliases: Vec::new(),
        }),
        warnings: all_warnings,
        mcp_decls,
        mcp_uses,
        contracts: vec![],
        modules,
        declared_paths,
    })
}

/// Walk all .rs files under `project_root` and extract MCP tool decls + uses.
/// Per-file parse errors are swallowed as ParseWarnings so one malformed file doesn't
/// abort the whole project scan.
fn scan_rust_source(
    project_root: &Path,
) -> (Vec<McpToolDecl>, Vec<McpClientUse>, Vec<ParseWarning>) {
    let language: Language = tree_sitter_rust::language();
    let bundled = include_str!("../ts_queries/rust_mcp.scm");
    let override_path =
        super::monorepo_root_from_project(project_root).join(".prograph/mcp_patterns/rust.scm");
    let combined = match std::fs::read_to_string(override_path).ok() {
        Some(extra) => format!("{}\n\n; --- user override ---\n{}", bundled, extra),
        None => bundled.to_string(),
    };

    let query = match Query::new(&language, &combined) {
        Ok(q) => q,
        Err(e) => {
            return (
                vec![],
                vec![],
                vec![ParseWarning {
                    rel_path: ".prograph/mcp_patterns/rust.scm".into(),
                    message: format!("failed to compile combined tree-sitter query: {}", e),
                }],
            );
        }
    };

    let mut decls = Vec::new();
    let mut uses = Vec::new();
    let mut warnings = Vec::new();

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        warnings.push(ParseWarning {
            rel_path: "<tree-sitter init>".into(),
            message: "failed to initialise tree-sitter-rust".into(),
        });
        return (decls, uses, warnings);
    }

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        e.depth() == 0
            || (!matches!(
                name.as_ref(),
                "target" | "node_modules" | ".venv" | "dist" | "build" | ".git" | "__pycache__"
            ) && !name.starts_with('.'))
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("rs") {
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
                    message: "tree-sitter failed to parse".into(),
                });
                continue;
            }
        };

        let source_bytes = source.as_bytes();
        let mut cursor = QueryCursor::new();

        for m in cursor.matches(&query, tree.root_node(), source_bytes) {
            let mut tool_name: Option<String> = None;
            let mut start_line: u32 = 1;
            let mut is_use = false;

            for capture in m.captures {
                let cap_name = &query.capture_names()[capture.index as usize];
                let text = capture
                    .node
                    .utf8_text(source_bytes)
                    .unwrap_or("")
                    .to_string();
                start_line = capture.node.start_position().row as u32 + 1;

                if cap_name == &"tool_name_literal" {
                    // string_literal node text in Rust includes the surrounding quotes —
                    // and may include `r"..."` prefixes. Strip conservatively.
                    let stripped = text
                        .trim_start_matches('r')
                        .trim_start_matches(['"', '\''])
                        .trim_end_matches(['"', '\''])
                        .to_string();
                    tool_name = Some(stripped);
                } else if cap_name == &"tool_use_method" {
                    is_use = true;
                }
            }

            let Some(name) = tool_name else { continue };

            if is_use {
                uses.push(McpClientUse {
                    tool_name: name,
                    rel_path: rel_path.clone(),
                    line: start_line,
                });
            } else {
                decls.push(McpToolDecl {
                    tool_name: name,
                    rel_path: rel_path.clone(),
                    line: start_line,
                });
            }
        }
    }

    (decls, uses, warnings)
}

/// M9: walk all .rs files under `project_root` and extract module-level facts.
/// The tree-sitter query already filters to `pub` items, so no post-filter is
/// needed for visibility. Internal imports are `use crate::...` only.
fn scan_rust_modules(project_root: &Path) -> (Vec<crate::facts::Module>, Vec<ParseWarning>) {
    let language: Language = tree_sitter_rust::language();
    let query_src = include_str!("../ts_queries/rust_symbols.scm");

    let query = match Query::new(&language, query_src) {
        Ok(q) => q,
        Err(e) => {
            return (
                vec![],
                vec![ParseWarning {
                    rel_path: "ts_queries/rust_symbols.scm".into(),
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
            message: "failed to initialise tree-sitter-rust".into(),
        });
        return (modules, warnings);
    }

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        e.depth() == 0
            || (!matches!(
                name.as_ref(),
                "target" | "node_modules" | ".venv" | "dist" | "build" | ".git" | "__pycache__"
            ) && !name.starts_with('.'))
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("rs") {
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
        let mut external_imports: Vec<crate::facts::ExternalImport> = Vec::new();

        for m in cursor.matches(&query, tree.root_node(), source_bytes) {
            let mut symbol_name: Option<String> = None;
            let mut line: u32 = 1;
            let mut kind: Option<crate::facts::SymbolKind> = None;
            // (use_path, is_internal) — captured from the `import_use` arm; the
            // post-loop block decides what to do based on the root segment.
            let mut use_clean: Option<String> = None;

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
                    "symbol_function" => kind = Some(crate::facts::SymbolKind::Function),
                    "symbol_struct" => kind = Some(crate::facts::SymbolKind::Struct),
                    "symbol_enum" => kind = Some(crate::facts::SymbolKind::Enum),
                    "symbol_trait" => kind = Some(crate::facts::SymbolKind::Trait),
                    "symbol_const" | "symbol_const_static" => {
                        kind = Some(crate::facts::SymbolKind::Const)
                    }
                    "symbol_type" => kind = Some(crate::facts::SymbolKind::Type),
                    "import_use" => {
                        // Capture is the entire `use_declaration` node — parse it manually
                        // for robustness against grammar version drift.
                        let raw = capture.node.utf8_text(source_bytes).unwrap_or("");
                        let cleaned = raw
                            .trim()
                            .trim_start_matches("use")
                            .trim()
                            .trim_end_matches(';')
                            .trim()
                            .to_string();
                        use_clean = Some(cleaned);
                    }
                    _ => {}
                }
            }

            if let Some(cleaned) = use_clean {
                // Root segment determines internal vs external.
                //   crate::a::b           → internal_imports
                //   self::x / super::y    → skip (intra-module sibling)
                //   ext_crate::a::b::Sym  → external_imports
                let (root, rest) = match cleaned.split_once("::") {
                    Some((r, rest)) => (r.trim().to_string(), rest.trim().to_string()),
                    None => (cleaned.clone(), String::new()),
                };

                if root == "crate" {
                    internal_imports.push(crate::facts::InternalImport {
                        target_path: cleaned,
                        line,
                    });
                } else if root == "self" || root == "super" {
                    // Intra-crate relative; don't cross project boundaries.
                } else {
                    // External use. The resolver wants the FULL ::-separated path
                    // (root + rest) so it can compute the inside-crate module path.
                    // Emit one ExternalImport per imported leaf symbol.
                    let symbols = parse_use_list_symbols(&rest);
                    if symbols.is_empty() {
                        external_imports.push(crate::facts::ExternalImport {
                            target_path: if rest.is_empty() {
                                root.clone()
                            } else {
                                format!("{}::{}", root, rest)
                            },
                            target_symbol: None,
                            line,
                        });
                    } else {
                        // For list form `root::a::{X, Y}`, the prefix shared
                        // by both is `root::a` — build target_path as that prefix
                        // and emit X, Y as separate target_symbol values.
                        let common_prefix = {
                            let trimmed_rest = if let Some(brace_pos) = rest.find('{') {
                                rest[..brace_pos].trim_end_matches("::").trim().to_string()
                            } else {
                                // Simple form `a::b::Sym` — drop the last segment.
                                rest.rsplit_once("::")
                                    .map(|(p, _)| p.to_string())
                                    .unwrap_or_default()
                            };
                            if trimmed_rest.is_empty() {
                                root.clone()
                            } else {
                                format!("{}::{}", root, trimmed_rest)
                            }
                        };
                        for sym in symbols {
                            external_imports.push(crate::facts::ExternalImport {
                                target_path: common_prefix.clone(),
                                target_symbol: Some(sym),
                                line,
                            });
                        }
                    }
                }
                continue;
            }

            let Some(name) = symbol_name else { continue };
            let kind = kind.unwrap_or(crate::facts::SymbolKind::Function);
            public_symbols.push(crate::facts::PublicSymbol { name, kind, line });
        }

        public_symbols.sort_by(|a, b| (a.line, &a.name).cmp(&(b.line, &b.name)));
        internal_imports.sort_by(|a, b| (a.line, &a.target_path).cmp(&(b.line, &b.target_path)));
        external_imports.sort_by(|a, b| {
            (a.line, &a.target_path, &a.target_symbol).cmp(&(
                b.line,
                &b.target_path,
                &b.target_symbol,
            ))
        });

        modules.push(crate::facts::Module {
            rel_path,
            language: "rust".into(),
            public_symbols,
            internal_imports,
            external_imports,
        });
    }

    modules.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    (modules, warnings)
}

/// Given the tail of a `use` declaration after the root segment (e.g. `bar::Baz` or
/// `bar::{Baz, Qux}`), return the list of imported leaf symbol names.
///
/// Best-effort and conservative — handles the common shapes:
///   `bar::Baz`             → ["Baz"]
///   `bar::{Baz, Qux}`      → ["Baz", "Qux"]
///   `{A, B as C}`          → ["A", "B as C"] (alias kept literal)
///   ``                     → []
fn parse_use_list_symbols(rest: &str) -> Vec<String> {
    if rest.is_empty() {
        return Vec::new();
    }
    if let Some(brace_pos) = rest.find('{') {
        if let Some(end_brace) = rest.rfind('}') {
            if end_brace > brace_pos {
                let inside = &rest[brace_pos + 1..end_brace];
                return inside
                    .split(',')
                    .map(|s| s.trim().trim_end_matches(';').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
    }
    rest.rsplit("::")
        .next()
        .map(|s| vec![s.trim().to_string()])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_cargo_toml(toml: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), toml).unwrap();
        dir
    }

    #[test]
    fn parses_minimal_cargo_toml() {
        let dir = write_cargo_toml(
            r#"
[package]
name = "foo"
version = "0.1.0"
"#,
        );
        let out = parse(dir.path()).unwrap();
        let manifest = out.manifest.unwrap();
        assert_eq!(manifest.declared_name, "foo");
        assert_eq!(manifest.version.as_deref(), Some("0.1.0"));
        assert!(manifest.declared_deps.is_empty());
        assert!(manifest.aliases.is_empty());
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn parses_string_form_dependencies() {
        let dir = write_cargo_toml(
            r#"
[package]
name = "consumer"
version = "1.0"

[dependencies]
serde = "1.0"
tokio = "1.36.0"
"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let names: std::collections::HashSet<_> = manifest
            .declared_deps
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(names.contains("serde"));
        assert!(names.contains("tokio"));
        let serde_dep = manifest
            .declared_deps
            .iter()
            .find(|d| d.name == "serde")
            .unwrap();
        assert_eq!(serde_dep.version_req.as_deref(), Some("1.0"));
    }

    #[test]
    fn parses_table_form_dependencies() {
        let dir = write_cargo_toml(
            r#"
[package]
name = "consumer"
version = "1.0"

[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
pyo3 = { workspace = true }
"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let rusqlite_dep = manifest
            .declared_deps
            .iter()
            .find(|d| d.name == "rusqlite")
            .unwrap();
        assert_eq!(rusqlite_dep.version_req.as_deref(), Some("0.31"));
        let pyo3_dep = manifest
            .declared_deps
            .iter()
            .find(|d| d.name == "pyo3")
            .unwrap();
        // workspace = true has no `version` key → version_req is None
        assert_eq!(pyo3_dep.version_req, None);
    }

    #[test]
    fn parses_workspace_inherited_package_fields() {
        // arbiter-cli idiom: `version.workspace = true` (dotted-key form).
        // Pre-fix this failed with "invalid type: map, expected a string".
        let dir = write_cargo_toml(
            r#"
[package]
name = "arb-cli"
version.workspace = true
edition.workspace = true

[dependencies]
serde.workspace = true
tokio = { workspace = true, features = ["full"] }
"#,
        );
        let out = parse(dir.path()).unwrap();
        let manifest = out
            .manifest
            .expect("parse should succeed despite workspace inheritance");
        assert_eq!(manifest.declared_name, "arb-cli");
        // version inherited → no literal available, but parse must NOT fail.
        assert_eq!(manifest.version, None);

        let names: std::collections::HashSet<_> = manifest
            .declared_deps
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(names.contains("serde"));
        assert!(names.contains("tokio"));
        // Both deps use workspace inheritance → version_req None.
        let serde_dep = manifest
            .declared_deps
            .iter()
            .find(|d| d.name == "serde")
            .unwrap();
        assert_eq!(serde_dep.version_req, None);
    }

    #[test]
    fn includes_dev_and_build_dependencies() {
        let dir = write_cargo_toml(
            r#"
[package]
name = "x"

[dependencies]
runtime = "1"

[dev-dependencies]
test-lib = "2"

[build-dependencies]
build-helper = "3"
"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let names: std::collections::HashSet<_> = manifest
            .declared_deps
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(names.contains("runtime"));
        assert!(names.contains("test-lib"));
        assert!(names.contains("build-helper"));
    }

    #[test]
    fn warns_when_workspace_root_only() {
        let dir = write_cargo_toml(
            r#"
[workspace]
members = ["a", "b"]
"#,
        );
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("workspace root"));
    }

    #[test]
    fn warns_when_no_cargo_toml() {
        let dir = TempDir::new().unwrap();
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("no Cargo.toml"));
    }

    #[test]
    fn errors_on_invalid_toml() {
        let dir = write_cargo_toml("[ this is not toml");
        let err = parse(dir.path()).unwrap_err();
        assert!(err.to_string().contains("parse error"));
    }

    #[test]
    fn scans_rust_tool_method_decl() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "rust-server"
version = "0.1.0"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            r#"pub fn setup(builder: &mut ServerBuilder) {
    builder.tool("decide", |args| Ok(()));
    builder.register_tool("evaluate", |args| Ok(()));
}
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(names.contains(&"decide"), "got: {:?}", names);
        assert!(names.contains(&"evaluate"), "got: {:?}", names);
    }

    #[test]
    fn scans_rust_tool_associated_fn_decl() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "rust-server"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/main.rs"),
            r#"fn build() {
    let t = Tool::new("hello");
    let b = ToolBuilder::new("goodbye");
}
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"goodbye"));
    }

    #[test]
    fn scans_rust_call_tool_invocation() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "rust-client"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            r#"async fn use_tool(client: &Client) {
    let _ = client.call_tool("decide", args).await;
}
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_uses.iter().map(|u| u.tool_name.as_str()).collect();
        assert!(names.contains(&"decide"));
    }

    #[test]
    fn skips_target_directory() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "x"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("target/debug")).unwrap();
        fs::write(
            dir.path().join("target/debug/trap.rs"),
            r#"fn x() { Tool::new("trap"); }"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            r#"fn x() { Tool::new("real"); }"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(names.contains(&"real"));
        assert!(!names.contains(&"trap"), "scanner must skip target/");
    }

    #[test]
    fn scans_rust_public_struct_and_fn() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            r#"pub struct Decider {
    pub policy: String,
}

pub fn decide(query: &str) -> bool { true }

fn private_helper() {}

pub enum Choice { Yes, No }

pub trait Decidable {}
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out
            .modules
            .iter()
            .find(|m| m.rel_path == "src/lib.rs")
            .expect("expected src/lib.rs module");
        let names: Vec<_> = module
            .public_symbols
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert!(names.contains(&"Decider"));
        assert!(names.contains(&"decide"));
        assert!(names.contains(&"Choice"));
        assert!(names.contains(&"Decidable"));
        assert!(
            !names.contains(&"private_helper"),
            "non-pub items must be filtered: {:?}",
            names
        );
    }

    #[test]
    fn scans_rust_use_crate_imports() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"c\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            r#"use crate::policy::Decider;
use crate::storage;
use std::collections::HashMap;
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out
            .modules
            .iter()
            .find(|m| m.rel_path == "src/lib.rs")
            .unwrap();
        let imports: Vec<_> = module
            .internal_imports
            .iter()
            .map(|i| i.target_path.as_str())
            .collect();
        assert!(
            imports.iter().any(|i| i.contains("crate::policy")),
            "expected crate::policy import, got: {:?}",
            imports
        );
        assert!(
            !imports.iter().any(|i| i.contains("std::collections")),
            "std imports filtered: {:?}",
            imports
        );
    }

    #[test]
    fn scans_external_use_with_symbol() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"c\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            r#"use atp_platform_sdk::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::policy::Decider;
use self::helper;
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out
            .modules
            .iter()
            .find(|m| m.rel_path == "src/lib.rs")
            .unwrap();
        let ext: Vec<_> = module.external_imports.iter().collect();

        let client = ext.iter().find(|e| {
            e.target_path == "atp_platform_sdk" && e.target_symbol.as_deref() == Some("Client")
        });
        assert!(
            client.is_some(),
            "missing atp_platform_sdk::Client: {:?}",
            ext
        );

        let deser = ext.iter().find(|e| {
            e.target_path == "serde" && e.target_symbol.as_deref() == Some("Deserialize")
        });
        let ser = ext
            .iter()
            .find(|e| e.target_path == "serde" && e.target_symbol.as_deref() == Some("Serialize"));
        assert!(
            deser.is_some() && ser.is_some(),
            "serde list use should produce 2 entries: {:?}",
            ext
        );

        // self::, crate::, super:: don't become external.
        assert!(!ext
            .iter()
            .any(|e| e.target_path == "crate" || e.target_path == "self"));
    }

    #[test]
    fn extracts_declared_paths_from_cargo_metadata() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "watcher"
version = "0.1.0"

[package.metadata.prograph]
reads = ["maestro/out/plan.json"]
"#,
        )
        .unwrap();
        let out = parse(dir.path()).unwrap();
        assert_eq!(out.declared_paths.len(), 1);
        assert_eq!(out.declared_paths[0].path, "maestro/out/plan.json");
        assert_eq!(out.declared_paths[0].source_path, "Cargo.toml");
        assert_eq!(out.declared_paths[0].line, 6);
    }

    #[test]
    fn parse_use_list_symbols_handles_braces() {
        assert_eq!(parse_use_list_symbols("Client"), vec!["Client".to_string()]);
        assert_eq!(
            parse_use_list_symbols("a::b::Client"),
            vec!["Client".to_string()]
        );
        assert_eq!(
            parse_use_list_symbols("{A, B, C}"),
            vec!["A".to_string(), "B".to_string(), "C".to_string()]
        );
        assert_eq!(parse_use_list_symbols(""), Vec::<String>::new());
    }
}
