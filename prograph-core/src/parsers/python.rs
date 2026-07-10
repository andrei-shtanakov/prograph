//! Python project parser — reads `pyproject.toml` + scans `.py` files for MCP tool decls/uses.

use std::path::Path;

use serde::Deserialize;
use tree_sitter::{Language, Parser, Query, QueryCursor};
use walkdir::WalkDir;

use super::ParserOutput;
use crate::errors::{PrographError, Result};
use crate::facts::{DepRequirement, Manifest, McpClientUse, McpToolDecl, ParseWarning};

#[derive(Debug, Deserialize)]
struct PyprojectRoot {
    project: Option<PyprojectProject>,
    #[serde(rename = "dependency-groups", default)]
    dependency_groups: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(default)]
    tool: Option<PyprojectTool>,
}

#[derive(Debug, Deserialize)]
struct PyprojectProject {
    name: Option<String>,
    version: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default, rename = "optional-dependencies")]
    optional_dependencies: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct PyprojectTool {
    #[serde(default)]
    prograph: Option<PyprojectToolPrograph>,
    #[serde(default)]
    uv: Option<PyprojectToolUv>,
}

#[derive(Debug, Deserialize)]
struct PyprojectToolUv {
    #[serde(default)]
    workspace: Option<PyprojectToolUvWorkspace>,
}

#[derive(Debug, Deserialize)]
struct PyprojectToolUvWorkspace {
    #[serde(default)]
    members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PyprojectToolPrograph {
    #[serde(default)]
    aliases: Vec<String>,
    /// Sub-directory names (relative to project root) to skip during workspace
    /// recursion. Typical use: vendored archives or external sub-modules that
    /// happen to look like projects (have README.md / Cargo.toml / etc).
    #[serde(default)]
    exclude: Vec<String>,
}

/// Return the `[tool.prograph] exclude` list — sub-directory names that the
/// discovery layer should skip during workspace recursion. Empty when there's
/// no pyproject.toml or no exclude list.
pub fn read_prograph_excludes(project_root: &Path) -> Vec<String> {
    let pyproject = project_root.join("pyproject.toml");
    let Ok(contents) = std::fs::read_to_string(pyproject) else {
        return Vec::new();
    };
    let Ok(root) = toml::from_str::<PyprojectRoot>(&contents) else {
        return Vec::new();
    };
    root.tool
        .and_then(|t| t.prograph)
        .map(|p| p.exclude)
        .unwrap_or_default()
}

/// Return the `[tool.uv.workspace] members` list, if any. Each entry is a
/// glob-or-literal path relative to the project root, e.g. `"packages/*"` or
/// `"atp-games"`. Empty when no pyproject.toml or no `[tool.uv.workspace]`.
pub fn workspace_members(project_root: &Path) -> Vec<String> {
    let pyproject = project_root.join("pyproject.toml");
    let Ok(contents) = std::fs::read_to_string(pyproject) else {
        return Vec::new();
    };
    let Ok(root) = toml::from_str::<PyprojectRoot>(&contents) else {
        return Vec::new();
    };
    root.tool
        .and_then(|t| t.uv)
        .and_then(|u| u.workspace)
        .map(|w| w.members)
        .unwrap_or_default()
}

/// Return `true` if a project's `pyproject.toml` declares a uv-style workspace
/// (`[tool.uv.workspace]`) or a hatch-style multi-package layout. Substring
/// check is intentional — the discovery layer only cares about presence, not
/// the full schema.
pub fn declares_workspace(project_root: &Path) -> bool {
    let pyproject = project_root.join("pyproject.toml");
    let Ok(contents) = std::fs::read_to_string(pyproject) else {
        return false;
    };
    contents.contains("[tool.uv.workspace]")
        || contents.contains("[tool.hatch.build.targets.wheel.packages")
}

pub fn parse(project_root: &Path) -> Result<ParserOutput> {
    let pyproject = project_root.join("pyproject.toml");
    if !pyproject.is_file() {
        // M2 ignores setup.py-only projects. M3+ can revisit.
        return Ok(ParserOutput {
            manifest: None,
            warnings: vec![ParseWarning {
                rel_path: "pyproject.toml".into(),
                message: "no pyproject.toml found".into(),
            }],
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules: vec![],
            declared_paths: vec![],
        });
    }

    let contents = std::fs::read_to_string(&pyproject).map_err(|source| PrographError::Io {
        path: pyproject.display().to_string(),
        source,
    })?;

    let root: PyprojectRoot = toml::from_str(&contents).map_err(|e| PrographError::Parse {
        path: pyproject.display().to_string(),
        reason: e.to_string(),
    })?;

    let project = match root.project {
        Some(p) => p,
        None => {
            return Ok(ParserOutput {
                manifest: None,
                warnings: vec![ParseWarning {
                    rel_path: "pyproject.toml".into(),
                    message: "no [project] table".into(),
                }],
                mcp_decls: vec![],
                mcp_uses: vec![],
                contracts: vec![],
                modules: vec![],
                declared_paths: vec![],
            });
        }
    };

    let declared_name = match project.name {
        Some(n) => n,
        None => {
            return Ok(ParserOutput {
                manifest: None,
                warnings: vec![ParseWarning {
                    rel_path: "pyproject.toml".into(),
                    message: "[project] missing 'name' key".into(),
                }],
                mcp_decls: vec![],
                mcp_uses: vec![],
                contracts: vec![],
                modules: vec![],
                declared_paths: vec![],
            });
        }
    };

    // Flatten [project].dependencies + [project.optional-dependencies] + [dependency-groups]
    // (PEP 735) into a single declared_deps list. M3 does not model dep groups separately —
    // they're all "this project declares a dependency on X" for matching purposes.
    let mut declared_deps: Vec<DepRequirement> = project
        .dependencies
        .iter()
        .map(|raw| parse_pep508(raw))
        .collect();
    for deps in project.optional_dependencies.values() {
        for raw in deps {
            declared_deps.push(parse_pep508(raw));
        }
    }
    for deps in root.dependency_groups.values() {
        for raw in deps {
            declared_deps.push(parse_pep508(raw));
        }
    }

    let aliases = root
        .tool
        .as_ref()
        .and_then(|t| t.prograph.as_ref())
        .map(|p| p.aliases.clone())
        .unwrap_or_default();

    let (mcp_decls, mcp_uses, mut all_warnings) = scan_python_source(project_root);
    let (modules, module_warnings) = scan_python_modules(project_root, &declared_name);
    all_warnings.extend(module_warnings);
    let declared_paths = extract_declared_paths(&contents, "pyproject.toml", &mut all_warnings);
    Ok(ParserOutput {
        manifest: Some(Manifest {
            declared_name,
            version: project.version,
            declared_deps,
            aliases,
        }),
        warnings: all_warnings,
        mcp_decls,
        mcp_uses,
        contracts: vec![],
        modules,
        declared_paths,
    })
}

/// M12: extract `[tool.prograph] reads/writes` (or `[package.metadata.prograph]` —
/// the caller picks the table) tolerantly. Malformed shapes warn and skip; they
/// never fail the manifest parse.
pub fn extract_declared_from_table(
    table: Option<&toml::Value>,
    contents: &str,
    source_path: &str,
    warnings: &mut Vec<ParseWarning>,
) -> Vec<crate::facts::DeclaredPath> {
    use crate::facts::{DeclaredMode, DeclaredPath};
    let mut out = Vec::new();
    let Some(table) = table else { return out };
    for (key, mode) in [
        ("reads", DeclaredMode::Read),
        ("writes", DeclaredMode::Write),
    ] {
        let Some(value) = table.get(key) else {
            continue;
        };
        let Some(items) = value.as_array() else {
            warnings.push(ParseWarning {
                rel_path: source_path.to_string(),
                message: format!("`{key}` must be a list of strings"),
            });
            continue;
        };
        for item in items {
            let Some(path) = item.as_str() else {
                warnings.push(ParseWarning {
                    rel_path: source_path.to_string(),
                    message: format!("`{key}` items must be strings"),
                });
                continue;
            };
            let path = path.trim().to_string();
            let (line, snippet) = find_manifest_line(contents, &path);
            out.push(DeclaredPath {
                mode,
                path,
                source_path: source_path.to_string(),
                line,
                snippet,
            });
        }
    }
    out
}

/// Best-effort 1-based line of the first manifest line containing `needle`.
fn find_manifest_line(contents: &str, needle: &str) -> (u32, Option<String>) {
    for (i, ln) in contents.lines().enumerate() {
        if ln.contains(needle) {
            return ((i + 1) as u32, Some(ln.trim().to_string()));
        }
    }
    (1, None)
}

/// M12: extract `[tool.prograph] reads/writes` from a pyproject.toml's raw contents.
/// Uses the untyped `toml::Value` path (not `PyprojectToolPrograph`) so a malformed
/// `reads`/`writes` shape warns and is skipped instead of failing the typed parse.
pub fn extract_declared_paths(
    contents: &str,
    source_path: &str,
    warnings: &mut Vec<ParseWarning>,
) -> Vec<crate::facts::DeclaredPath> {
    let value: toml::Value = match toml::from_str(contents) {
        Ok(v) => v,
        Err(_) => return Vec::new(), // whole-file TOML errors are reported by the main parse
    };
    let table = value.get("tool").and_then(|t| t.get("prograph"));
    extract_declared_from_table(table, contents, source_path, warnings)
}

/// Walk all .py files under `project_root` and extract MCP tool decls + uses.
/// Per-file parse errors are swallowed as ParseWarnings so one malformed file doesn't
/// abort the whole project scan.
fn scan_python_source(
    project_root: &Path,
) -> (Vec<McpToolDecl>, Vec<McpClientUse>, Vec<ParseWarning>) {
    let language: Language = tree_sitter_python::language();
    let bundled = include_str!("../ts_queries/python_mcp.scm");
    let override_path =
        super::monorepo_root_from_project(project_root).join(".prograph/mcp_patterns/python.scm");
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
                    rel_path: ".prograph/mcp_patterns/python.scm".into(),
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
            message: "failed to initialise tree-sitter-python".into(),
        });
        return (decls, uses, warnings);
    }

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        e.depth() == 0
            || (!matches!(
                name.as_ref(),
                ".venv" | "__pycache__" | "node_modules" | "target" | "dist" | "build" | ".git"
            ) && !name.starts_with('.'))
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("py") {
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
            let mut tool_name_from_literal: Option<String> = None;
            let mut tool_name_from_ident: Option<String> = None;
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

                if cap_name == &"tool_name" {
                    tool_name_from_ident = Some(text);
                } else if cap_name == &"tool_name_literal" {
                    let stripped = text
                        .trim_start_matches(['"', '\''])
                        .trim_end_matches(['"', '\''])
                        .to_string();
                    tool_name_from_literal = Some(stripped);
                } else if cap_name == &"tool_use_call" {
                    is_use = true;
                }
            }

            let tool_name = tool_name_from_literal.or(tool_name_from_ident);
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

/// M9: walk all .py files under `project_root` and extract module-level facts.
/// Filters public symbols (no leading underscore) and internal imports (those
/// whose target path starts with the project's declared package name or is
/// relative).
fn scan_python_modules(
    project_root: &Path,
    declared_package: &str,
) -> (Vec<crate::facts::Module>, Vec<ParseWarning>) {
    let language: Language = tree_sitter_python::language();
    let query_src = include_str!("../ts_queries/python_symbols.scm");

    let query = match Query::new(&language, query_src) {
        Ok(q) => q,
        Err(e) => {
            return (
                vec![],
                vec![ParseWarning {
                    rel_path: "ts_queries/python_symbols.scm".into(),
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
            message: "failed to initialise tree-sitter-python".into(),
        });
        return (modules, warnings);
    }

    // Dashes in distribution names map to underscores in import paths
    // (`atp-platform` ↦ `atp_platform`).
    let pkg_prefix = declared_package.replace('-', "_");

    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        e.depth() == 0
            || (!matches!(
                name.as_ref(),
                ".venv" | "__pycache__" | "node_modules" | "target" | "dist" | "build" | ".git"
            ) && !name.starts_with('.'))
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("py") {
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
            let mut import_target: Option<String> = None;
            let mut imported_symbol: Option<String> = None;
            let mut line: u32 = 1;
            let mut kind_hint: Option<crate::facts::SymbolKind> = None;
            let mut is_import = false;
            let mut is_relative_import = false;

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
                    "import_target" => {
                        import_target = Some(text);
                        is_import = true;
                    }
                    "import_symbol" => {
                        imported_symbol = Some(text);
                    }
                    "symbol_function" => kind_hint = Some(crate::facts::SymbolKind::Function),
                    "symbol_class" => kind_hint = Some(crate::facts::SymbolKind::Class),
                    "symbol_const" => kind_hint = Some(crate::facts::SymbolKind::Const),
                    "import_simple" | "import_from" | "import_from_aliased" => {
                        is_import = true;
                    }
                    "import_from_relative" => {
                        is_import = true;
                        is_relative_import = true;
                    }
                    _ => {}
                }
            }

            if is_import {
                if let Some(target) = import_target.clone() {
                    let is_relative = is_relative_import || target.starts_with('.');
                    let is_internal = is_relative
                        || target == pkg_prefix
                        || target.starts_with(&format!("{pkg_prefix}."));
                    if is_internal {
                        internal_imports.push(crate::facts::InternalImport {
                            target_path: target.clone(),
                            line,
                        });
                    }

                    // M10: emit ExternalImport for every non-relative import; the
                    // resolver layer decides later whether it points at an in-monorepo
                    // project. Relative imports stay inside the project by construction.
                    if !is_relative {
                        external_imports.push(crate::facts::ExternalImport {
                            target_path: target,
                            target_symbol: imported_symbol,
                            line,
                        });
                    }
                }
                continue;
            }

            let Some(name) = symbol_name else { continue };
            if name.starts_with('_') {
                continue;
            }
            let kind = kind_hint.unwrap_or(crate::facts::SymbolKind::Function);
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
            language: "python".into(),
            public_symbols,
            internal_imports,
            external_imports,
        });
    }

    modules.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    (modules, warnings)
}

/// Split a PEP 508 dep string like "eval-sdk>=1.0" into (name, version_req).
/// Best-effort: handles `>=`, `<=`, `==`, `~=`, `<`, `>`, `!=` operators and bare names.
/// Extras and environment markers are stripped (e.g. "foo[bar]>=1.0; python_version<'4'").
///
/// Handles:
/// - `foo>=1.0` → name="foo", version_req=">=1.0"
/// - `foo[extras]>=1.0; marker` → strips extras + marker, then operator parse
/// - `foo @ git+https://...` → name="foo", version_req=None (PEP 508 URL form
///   has no PEP 440 version constraint)
/// - `foo` → name="foo", version_req=None
fn parse_pep508(raw: &str) -> DepRequirement {
    let no_marker = raw.split(';').next().unwrap_or(raw).trim();
    let no_extras = strip_extras(no_marker);

    // PEP 508 URL form: `name @ url`. No PEP 440 operator includes `@`, so a
    // bare `@` always signals the URL form; the URL portion carries no usable
    // version constraint so version_req stays None.
    if let Some(at_pos) = no_extras.find('@') {
        let name = no_extras[..at_pos].trim().to_string();
        if !name.is_empty() {
            return DepRequirement {
                name,
                version_req: None,
            };
        }
    }

    // Find first version operator
    const OPS: &[&str] = &[">=", "<=", "==", "~=", "!=", ">", "<"];
    for op in OPS {
        if let Some(pos) = no_extras.find(op) {
            let name = no_extras[..pos].trim().to_string();
            let version_req = no_extras[pos..].trim().to_string();
            return DepRequirement {
                name,
                version_req: Some(version_req),
            };
        }
    }
    DepRequirement {
        name: no_extras.trim().to_string(),
        version_req: None,
    }
}

fn strip_extras(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0;
    for ch in s.chars() {
        match ch {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_pyproject(toml_contents: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("pyproject.toml"), toml_contents).unwrap();
        dir
    }

    #[test]
    fn parses_minimal_pyproject() {
        let dir = write_pyproject(
            r#"
[project]
name = "foo"
version = "1.0"
dependencies = []
"#,
        );
        let out = parse(dir.path()).unwrap();
        let manifest = out.manifest.unwrap();
        assert_eq!(manifest.declared_name, "foo");
        assert_eq!(manifest.version.as_deref(), Some("1.0"));
        assert!(manifest.declared_deps.is_empty());
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn parses_dependencies_with_operators() {
        let dir = write_pyproject(
            r#"
[project]
name = "consumer"
dependencies = ["eval-sdk>=1.0", "policy", "httpx==0.27.0"]
"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let deps = manifest.declared_deps;
        assert_eq!(deps.len(), 3);
        assert_eq!(deps[0].name, "eval-sdk");
        assert_eq!(deps[0].version_req.as_deref(), Some(">=1.0"));
        assert_eq!(deps[1].name, "policy");
        assert_eq!(deps[1].version_req, None);
        assert_eq!(deps[2].name, "httpx");
        assert_eq!(deps[2].version_req.as_deref(), Some("==0.27.0"));
    }

    #[test]
    fn strips_extras_and_markers() {
        let dir = write_pyproject(
            r#"
[project]
name = "x"
dependencies = ["requests[socks,security]>=2.0; python_version<'4'"]
"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let dep = &manifest.declared_deps[0];
        assert_eq!(dep.name, "requests");
        assert_eq!(dep.version_req.as_deref(), Some(">=2.0"));
    }

    #[test]
    fn pep508_url_form_extracts_name_only() {
        let dir = write_pyproject(
            r#"
[project]
name = "consumer"
dependencies = ["foo @ git+https://github.com/x/foo.git"]
"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let dep = &manifest.declared_deps[0];
        assert_eq!(dep.name, "foo");
        assert!(dep.version_req.is_none(), "URL form has no version_req");
    }

    #[test]
    fn pep508_url_form_with_extras_works() {
        let dir = write_pyproject(
            r#"
[project]
name = "consumer"
dependencies = ["foo[bar,baz] @ https://example.org/foo.tar.gz"]
"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let dep = &manifest.declared_deps[0];
        assert_eq!(dep.name, "foo");
    }

    #[test]
    fn warns_when_no_pyproject() {
        let dir = TempDir::new().unwrap();
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].message.contains("no pyproject.toml"));
    }

    #[test]
    fn warns_when_no_project_table() {
        let dir = write_pyproject(
            r#"
[build-system]
requires = ["setuptools"]
"#,
        );
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("no [project] table"));
    }

    #[test]
    fn warns_when_no_name() {
        let dir = write_pyproject(
            r#"
[project]
version = "1.0"
dependencies = []
"#,
        );
        let out = parse(dir.path()).unwrap();
        assert!(out.manifest.is_none());
        assert!(out.warnings[0].message.contains("missing 'name'"));
    }

    #[test]
    fn errors_on_invalid_toml() {
        let dir = write_pyproject("[ this is not toml");
        let err = parse(dir.path()).unwrap_err();
        assert!(err.to_string().contains("parse error"));
    }

    #[test]
    fn reads_dependency_groups_pep735() {
        let dir = write_pyproject(
            r#"
[project]
name = "consumer"
dependencies = []

[dependency-groups]
dev = ["spec-runner>=0.1.4", "pytest"]
docs = ["sphinx"]
"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let names: std::collections::HashSet<_> = manifest
            .declared_deps
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["spec-runner", "pytest", "sphinx"].into_iter().collect()
        );
    }

    #[test]
    fn reads_optional_dependencies() {
        let dir = write_pyproject(
            r#"
[project]
name = "consumer"
dependencies = ["core-lib"]

[project.optional-dependencies]
gui = ["qt-bindings>=6.0"]
cli = ["typer"]
"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        let names: std::collections::HashSet<_> = manifest
            .declared_deps
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(names.contains("core-lib"));
        assert!(names.contains("qt-bindings"));
        assert!(names.contains("typer"));
    }

    #[test]
    fn reads_tool_prograph_aliases() {
        let dir = write_pyproject(
            r#"
[project]
name = "atp-platform"
dependencies = []

[tool.prograph]
aliases = ["atp-platform-sdk", "atp-platform-cli"]
"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        assert_eq!(manifest.declared_name, "atp-platform");
        assert_eq!(
            manifest.aliases,
            vec![
                "atp-platform-sdk".to_string(),
                "atp-platform-cli".to_string()
            ]
        );
    }

    #[test]
    fn aliases_default_to_empty_when_no_tool_block() {
        let dir = write_pyproject(
            r#"
[project]
name = "plain"
dependencies = []
"#,
        );
        let manifest = parse(dir.path()).unwrap().manifest.unwrap();
        assert!(manifest.aliases.is_empty());
    }

    #[test]
    fn scans_fastmcp_tool_decorator() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "server-proj"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("server.py"),
            r#"from mcp.server.fastmcp import FastMCP

server = FastMCP("test")

@server.tool()
def my_tool(x: int) -> int:
    return x + 1
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(
            names.contains(&"my_tool"),
            "expected my_tool decl, got: {:?}",
            names
        );
    }

    #[test]
    fn scans_call_tool_invocation() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "client-proj"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("client.py"),
            r#"async def run(session):
    result = await session.call_tool("decide", arguments={"x": 1})
    return result
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_uses.iter().map(|u| u.tool_name.as_str()).collect();
        assert!(
            names.contains(&"decide"),
            "expected decide use, got: {:?}",
            names
        );
    }

    #[test]
    fn scans_private_call_tool_wrapper() {
        // Maestro's arbiter_client.py wraps the MCP client in a private
        // `_call_tool` method (retry/reconnect logic) and dispatches
        // tool names through it. The bundled pattern catches both the
        // canonical `.call_tool(...)` and the underscore-prefixed wrapper.
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "wrapper-proj"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("client.py"),
            r#"class Client:
    async def route_task(self, args):
        return await self._call_tool("route_task", args)

    async def report_outcome(self, args):
        return await self._call_tool("report_outcome", args)
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_uses.iter().map(|u| u.tool_name.as_str()).collect();
        assert!(
            names.contains(&"route_task"),
            "expected route_task via _call_tool, got: {:?}",
            names
        );
        assert!(
            names.contains(&"report_outcome"),
            "expected report_outcome via _call_tool, got: {:?}",
            names
        );
    }

    #[test]
    fn skips_venv_and_pycache() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "x"
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join(".venv/lib")).unwrap();
        fs::write(
            dir.path().join(".venv/lib/trap.py"),
            r#"@server.tool()
def trap_tool(): pass
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("real.py"),
            r#"@server.tool()
def real_tool(): pass
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(names.contains(&"real_tool"));
        assert!(!names.contains(&"trap_tool"), "scanner must skip .venv");
    }

    #[test]
    fn parser_picks_up_contracts_in_project() {
        // Test wiring through the dispatch — contracts are scanned via parse_project,
        // not via python::parse() directly.
        use crate::parsers::parse_project;
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "x"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("schema.json"),
            r#"{"$id": "x-schema", "$schema": "https://json-schema.org/draft/2020-12/schema"}"#,
        )
        .unwrap();
        let out = parse_project(dir.path(), crate::models::ProjectKind::Python).unwrap();
        assert_eq!(out.contracts.len(), 1);
        assert_eq!(out.contracts[0].declared_id.as_deref(), Some("x-schema"));
    }

    #[test]
    fn scan_records_line_numbers() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "x"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("s.py"),
            "\n\n\n@server.tool()\ndef widget(): pass\n",
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let decl = out
            .mcp_decls
            .iter()
            .find(|d| d.tool_name == "widget")
            .unwrap();
        assert!(decl.line >= 4, "expected line >= 4, got {}", decl.line);
    }

    #[test]
    fn loads_mcp_pattern_override_from_monorepo_root() {
        let monorepo = TempDir::new().unwrap();
        // Set up .prograph dir + an override that detects `.custom_tool("name", ...)` calls.
        fs::create_dir_all(monorepo.path().join(".prograph/mcp_patterns")).unwrap();
        fs::write(
            monorepo.path().join(".prograph/mcp_patterns/python.scm"),
            r#"
(call
  function: (attribute attribute: (identifier) @method)
  arguments: (argument_list . (string) @tool_name_literal)
  (#eq? @method "custom_tool")) @tool_decl_custom
"#,
        )
        .unwrap();

        // Create a project with code that matches the custom pattern.
        let proj = monorepo.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            proj.join("pyproject.toml"),
            r#"[project]
name = "p"
"#,
        )
        .unwrap();
        fs::write(proj.join("server.py"), r#"server.custom_tool("decide_v2")"#).unwrap();

        let out = parse(&proj).unwrap();
        let names: Vec<_> = out.mcp_decls.iter().map(|d| d.tool_name.as_str()).collect();
        assert!(
            names.contains(&"decide_v2"),
            "expected override pattern to fire, got: {:?}",
            names
        );
    }

    #[test]
    fn scans_public_python_function() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"myproj\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("api.py"),
            r#"def public_fn():
    return 1

def _private_fn():
    return 2

class PublicClass:
    pass
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out
            .modules
            .iter()
            .find(|m| m.rel_path == "api.py")
            .expect("expected api.py module");
        let names: Vec<_> = module
            .public_symbols
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert!(names.contains(&"public_fn"));
        assert!(names.contains(&"PublicClass"));
        assert!(
            !names.contains(&"_private_fn"),
            "underscored names must be filtered: {:?}",
            names
        );
    }

    #[test]
    fn scans_internal_imports_only() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"myproj\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("api.py"),
            r#"import os
import myproj.util
from myproj.helpers import foo
from external_lib import bar
"#,
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "api.py").unwrap();
        let targets: Vec<_> = module
            .internal_imports
            .iter()
            .map(|i| i.target_path.as_str())
            .collect();
        assert!(targets.contains(&"myproj.util"));
        assert!(targets.contains(&"myproj.helpers"));
        assert!(!targets.contains(&"os"), "stdlib imports filtered");
        assert!(
            !targets.contains(&"external_lib"),
            "external pkg imports filtered"
        );
    }

    #[test]
    fn scans_python_hyphen_dash_normalisation() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"atp-platform\"\n",
        )
        .unwrap();
        fs::write(dir.path().join("api.py"), "import atp_platform.core\n").unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "api.py").unwrap();
        let targets: Vec<_> = module
            .internal_imports
            .iter()
            .map(|i| i.target_path.as_str())
            .collect();
        assert!(
            targets.contains(&"atp_platform.core"),
            "dash→underscore normalisation must match imports: {:?}",
            targets
        );
    }

    #[test]
    fn scans_external_imports_with_symbol() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"consumer\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("api.py"),
            "from atp_platform.sdk import MaestroATPAdapter, ToolClient\nimport requests\n",
        )
        .unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "api.py").unwrap();

        let ext: Vec<_> = module.external_imports.iter().collect();
        // Two from-imports + one bare import = three external_imports total.
        assert!(
            ext.len() >= 3,
            "expected ≥3 external imports, got {:?}",
            ext
        );

        let adapter = ext.iter().find(|e| {
            e.target_path == "atp_platform.sdk"
                && e.target_symbol.as_deref() == Some("MaestroATPAdapter")
        });
        assert!(
            adapter.is_some(),
            "missing atp_platform.sdk::MaestroATPAdapter import: {:?}",
            ext
        );

        let tool_client = ext.iter().find(|e| {
            e.target_path == "atp_platform.sdk" && e.target_symbol.as_deref() == Some("ToolClient")
        });
        assert!(
            tool_client.is_some(),
            "missing atp_platform.sdk::ToolClient import: {:?}",
            ext
        );

        let requests_imp = ext
            .iter()
            .find(|e| e.target_path == "requests" && e.target_symbol.is_none());
        assert!(
            requests_imp.is_some(),
            "missing bare `import requests`: {:?}",
            ext
        );
    }

    #[test]
    fn relative_imports_dont_become_external() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"consumer\"\n",
        )
        .unwrap();
        fs::write(dir.path().join("api.py"), "from .util import helper\n").unwrap();

        let out = parse(dir.path()).unwrap();
        let module = out.modules.iter().find(|m| m.rel_path == "api.py").unwrap();
        assert!(
            module.external_imports.is_empty(),
            "relative imports must NOT appear in external_imports: {:?}",
            module.external_imports
        );
    }

    #[test]
    fn extracts_declared_reads_and_writes_with_lines() {
        let toml = r#"[project]
name = "dispatcher"
version = "1.0"

[tool.prograph]
reads = ["proctor/config/proctor.yaml", "proctor/data/state.db"]
writes = ["prograph-vault/derived/"]
"#;
        let mut warnings = Vec::new();
        let dp = extract_declared_paths(toml, "pyproject.toml", &mut warnings);
        assert!(warnings.is_empty());
        assert_eq!(dp.len(), 3);
        assert_eq!(dp[0].mode, crate::facts::DeclaredMode::Read);
        assert_eq!(dp[0].path, "proctor/config/proctor.yaml");
        assert_eq!(dp[0].source_path, "pyproject.toml");
        assert_eq!(dp[0].line, 6, "reads entries live on line 6");
        assert!(dp[0].snippet.as_deref().unwrap().contains("proctor/config"));
        assert_eq!(dp[2].mode, crate::facts::DeclaredMode::Write);
        assert_eq!(dp[2].path, "prograph-vault/derived/");
        assert_eq!(dp[2].line, 7);
    }

    #[test]
    fn malformed_reads_warns_but_does_not_break_manifest_parse() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            r#"[project]
name = "p"
version = "1.0"
dependencies = ["requests"]

[tool.prograph]
reads = "not-a-list"
"#,
        )
        .unwrap();
        let out = parse(dir.path()).unwrap();
        // Manifest itself parsed fine — deps intact.
        let m = out
            .manifest
            .expect("manifest must survive broken declarations");
        assert_eq!(m.declared_name, "p");
        assert!(!m.declared_deps.is_empty());
        // Declarations skipped with a warning.
        assert!(out.declared_paths.is_empty());
        assert!(out.warnings.iter().any(|w| w.message.contains("reads")));
    }

    #[test]
    fn non_string_items_warn_and_skip_only_those_items() {
        let toml = "[tool.prograph]\nreads = [\"ok/path\", 42]\n";
        let mut warnings = Vec::new();
        let dp = extract_declared_paths(toml, "pyproject.toml", &mut warnings);
        assert_eq!(dp.len(), 1);
        assert_eq!(dp[0].path, "ok/path");
        assert_eq!(warnings.len(), 1);
    }
}
