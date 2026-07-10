//! Per-language parsers. Each parser extracts `Manifest` from a project root.

pub mod contracts;
pub mod js;
pub mod python;
pub mod rust;

use std::path::{Path, PathBuf};

/// Walk upward from `project_root` looking for a `.prograph/` directory.
/// Returns the directory containing `.prograph/` (the monorepo root) on success,
/// or `project_root` itself as a fallback when no marker is found.
///
/// Used by M7 to locate `.prograph/mcp_patterns/{python,rust}.scm` override files.
pub(super) fn monorepo_root_from_project(project_root: &Path) -> PathBuf {
    let mut cur = project_root.to_path_buf();
    loop {
        if cur.join(".prograph").is_dir() {
            return cur;
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => return project_root.to_path_buf(),
        }
    }
}

use crate::errors::Result;
use crate::facts::{Manifest, ParseWarning};
use crate::models::ProjectKind;

/// Parser output for a single project.
#[derive(Debug)]
pub struct ParserOutput {
    pub manifest: Option<Manifest>,
    pub warnings: Vec<ParseWarning>,
    /// MCP server-side tool decls extracted from source. Populated by M4+ parsers.
    pub mcp_decls: Vec<crate::facts::McpToolDecl>,
    /// MCP client-side tool invocations. Populated by M4+ parsers.
    pub mcp_uses: Vec<crate::facts::McpClientUse>,
    /// Contract files found in this project. Populated by M4+ parsers.
    pub contracts: Vec<crate::facts::ContractFile>,
    /// M9: source files with public symbols + internal imports.
    pub modules: Vec<crate::facts::Module>,
    /// M12: declared file-based integrations (`[tool.prograph] reads/writes`).
    pub declared_paths: Vec<crate::facts::DeclaredPath>,
}

/// Dispatch a project to the right per-language parser. The contracts file scan
/// runs for every kind regardless of the language parser — it's a pure file-system
/// pass that surfaces JSON Schema / OpenAPI / .proto files even in docs-only projects.
pub fn parse_project(root: &Path, kind: ProjectKind) -> Result<ParserOutput> {
    let mut out = match kind {
        ProjectKind::Python => python::parse(root)?,
        ProjectKind::Rust => rust::parse(root)?,
        ProjectKind::Js => js::parse(root)?,
        ProjectKind::Mixed => parse_mixed(root)?,
        _ => ParserOutput {
            manifest: None,
            warnings: vec![],
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules: vec![],
            declared_paths: vec![],
        },
    };
    if out.contracts.is_empty() {
        out.contracts = contracts::scan(root);
    }
    Ok(out)
}

/// For Mixed projects (e.g. prograph itself: Python + Rust core), prefer Python's
/// pyproject.toml as the canonical declared_name. The Rust crate's name typically
/// differs (e.g. "prograph-core" vs the Python "prograph") and is the *internal*
/// extension name, not the published name. M3 keeps this heuristic simple; M4+ may
/// expose both via separate sub-projects.
fn parse_mixed(root: &Path) -> Result<ParserOutput> {
    let py = python::parse(root)?;
    if py.manifest.is_some() {
        // Canonical output stays Python, but [package.metadata.prograph] in the
        // co-located Cargo.toml must not be silently ignored (spec: Mixed merges
        // declared_paths from BOTH manifests). Extract straight from Cargo.toml
        // instead of running the full Rust parse: declaration warnings survive
        // (a malformed section must not fail silently) while the secondary
        // manifest's own parse warnings are not duplicated.
        let mut out = py;
        if let Ok(contents) = std::fs::read_to_string(root.join("Cargo.toml")) {
            if let Ok(value) = toml::from_str::<toml::Value>(&contents) {
                let table = value
                    .get("package")
                    .and_then(|p| p.get("metadata"))
                    .and_then(|m| m.get("prograph"));
                let mut warnings = Vec::new();
                out.declared_paths
                    .extend(python::extract_declared_from_table(
                        table,
                        &contents,
                        "Cargo.toml",
                        &mut warnings,
                    ));
                out.warnings.extend(warnings);
            }
        }
        return Ok(out);
    }
    let rs = rust::parse(root)?;
    if rs.manifest.is_some() {
        return Ok(rs);
    }
    js::parse(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn mixed_project_surfaces_malformed_cargo_declaration_warning() {
        // Regression (PR #9 review): a malformed [package.metadata.prograph] in a
        // Mixed root must warn, not fail silently, even though the canonical
        // manifest is Python.
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"m\"\nversion = \"1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"m-core\"\nversion = \"0.1.0\"\n[package.metadata.prograph]\nreads = \"not-a-list\"\n",
        )
        .unwrap();
        let out = parse_project(dir.path(), crate::models::ProjectKind::Mixed).unwrap();
        assert!(out.declared_paths.is_empty());
        assert!(
            out.warnings.iter().any(|w| w.message.contains("reads")),
            "malformed Cargo declaration must surface a warning in Mixed output"
        );
    }

    #[test]
    fn mixed_project_unions_declared_paths_from_both_manifests() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"m\"\nversion = \"1.0\"\n[tool.prograph]\nreads = [\"a/x\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"m-core\"\nversion = \"0.1.0\"\n[package.metadata.prograph]\nreads = [\"b/y\"]\n",
        )
        .unwrap();
        let out = parse_project(dir.path(), crate::models::ProjectKind::Mixed).unwrap();
        assert_eq!(
            out.manifest.as_ref().unwrap().declared_name,
            "m",
            "canonical stays Python"
        );
        let mut paths: Vec<(&str, &str)> = out
            .declared_paths
            .iter()
            .map(|d| (d.path.as_str(), d.source_path.as_str()))
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            vec![("a/x", "pyproject.toml"), ("b/y", "Cargo.toml")]
        );
    }
}
