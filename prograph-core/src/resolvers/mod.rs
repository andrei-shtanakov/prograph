//! Resolver layer — turns parser-side `ExternalImport`s into
//! `cross_project_symbol_refs` rows.
//!
//! Per language, the resolver answers: given an external import like
//! `atp_platform.sdk::MaestroATPAdapter`, which in-monorepo project (if any)
//! is the publisher, and what's the module path + symbol name inside that
//! project?

pub mod python;
pub mod rust;

/// A resolved cross-project symbol reference, ready to be persisted into
/// `cross_project_symbol_refs`. The indexer fills in DB ids when writing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRef {
    /// Index into `Vec<ProjectFacts>` for the source project.
    pub from_project_idx: usize,
    /// Path of the source module within the project (matches `Module.rel_path`).
    pub from_module_rel_path: String,
    pub from_line: u32,
    /// Index into `Vec<ProjectFacts>` for the resolved publisher project.
    pub to_project_idx: usize,
    /// Module path INSIDE the publisher project — for Python this is the dotted
    /// target_path minus the publisher's package prefix (`atp_platform.sdk` →
    /// `sdk`). For Rust: `crate_name::a::b::Sym` → `a::b`.
    pub to_module_path: String,
    pub to_symbol_name: Option<String>,
}

/// Build a publisher-name → project_idx lookup from a slice of ProjectFacts.
/// Honours each project's Manifest.aliases AND applies dash↔underscore
/// normalisation (a project named `atp-platform` matches imports of
/// `atp_platform`).
pub(crate) fn build_publisher_index(
    facts: &[crate::facts::ProjectFacts],
) -> std::collections::HashMap<String, usize> {
    let mut out = std::collections::HashMap::new();
    for (idx, p) in facts.iter().enumerate() {
        let Some(m) = &p.manifest else { continue };

        let mut names: Vec<String> = Vec::new();
        names.push(m.declared_name.clone());
        names.extend(m.aliases.iter().cloned());

        let underscored: Vec<String> = names
            .iter()
            .filter(|n| n.contains('-'))
            .map(|n| n.replace('-', "_"))
            .collect();
        names.extend(underscored);

        for name in names {
            out.entry(name).or_insert(idx);
        }
    }
    out
}
