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
        return Ok(py);
    }
    let rs = rust::parse(root)?;
    if rs.manifest.is_some() {
        return Ok(rs);
    }
    js::parse(root)
}
