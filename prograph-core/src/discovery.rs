//! Project discovery — file-system scan and classification by signal files.
//!
//! The discovery layer is intentionally cheap: it touches only signal files
//! at the project root (pyproject.toml, Cargo.toml, package.json, README.md,
//! CLAUDE.md, TODO.md). Deep parsing happens in M2+ via `parsers`.

use std::path::Path;

use pyo3::prelude::*;

use crate::errors::Result;
use crate::models::{ProjectCandidate, ProjectKind};

const PYTHON_SIGNALS: &[&str] = &["pyproject.toml", "setup.py"];
const RUST_SIGNAL: &str = "Cargo.toml";
const JS_SIGNAL: &str = "package.json";
const DOC_SIGNALS: &[&str] = &["README.md", "CLAUDE.md", "TODO.md"];

/// Classify a single project directory by examining signal files at its root.
///
/// Returns `None` if the directory is not a project candidate (no recognised
/// signal files at all). Returns `Some` with the candidate's classification
/// otherwise.
pub fn classify_project(
    root: &Path,
    name: &str,
    rel_root: &str,
) -> Result<Option<ProjectCandidate>> {
    let mut manifests = Vec::new();
    let mut has_python = false;
    let mut has_rust = false;
    let mut has_js = false;
    let mut has_docs = false;

    for signal in PYTHON_SIGNALS {
        if root.join(signal).is_file() {
            manifests.push(signal.to_string());
            has_python = true;
        }
    }
    if root.join(RUST_SIGNAL).is_file() {
        manifests.push(RUST_SIGNAL.to_string());
        has_rust = true;
    }
    if root.join(JS_SIGNAL).is_file() {
        manifests.push(JS_SIGNAL.to_string());
        has_js = true;
    }
    for signal in DOC_SIGNALS {
        if root.join(signal).is_file() {
            manifests.push(signal.to_string());
            has_docs = true;
        }
    }

    let code_signals = [has_python, has_rust, has_js]
        .iter()
        .filter(|x| **x)
        .count();

    let kind = match (code_signals, has_docs) {
        (0, false) => return Ok(None),
        (0, true) => ProjectKind::Docs,
        (1, _) if has_python => ProjectKind::Python,
        (1, _) if has_rust => ProjectKind::Rust,
        (1, _) if has_js => ProjectKind::Js,
        _ => ProjectKind::Mixed,
    };

    Ok(Some(ProjectCandidate {
        name: name.to_string(),
        root_path: rel_root.to_string(),
        kind,
        manifests,
    }))
}

/// Scan the first-level subdirectories of `monorepo_root` and return all classified candidates.
///
/// Hidden directories (those whose name starts with `.`) and the `target/`, `node_modules/`,
/// `.venv/`, `dist/`, `build/` directories are skipped automatically — they're build artefacts,
/// not projects.
pub fn scan_monorepo(monorepo_root: &Path) -> Result<Vec<ProjectCandidate>> {
    if !monorepo_root.is_dir() {
        return Err(crate::errors::PrographError::Discovery {
            root: monorepo_root.display().to_string(),
            reason: "monorepo root is not a directory".into(),
        });
    }

    let mut candidates = Vec::new();
    let entries =
        std::fs::read_dir(monorepo_root).map_err(|source| crate::errors::PrographError::Io {
            path: monorepo_root.display().to_string(),
            source,
        })?;

    for entry in entries {
        let entry = entry.map_err(|source| crate::errors::PrographError::Io {
            path: monorepo_root.display().to_string(),
            source,
        })?;

        let file_type = entry
            .file_type()
            .map_err(|source| crate::errors::PrographError::Io {
                path: entry.path().display().to_string(),
                source,
            })?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| crate::errors::PrographError::Discovery {
                root: monorepo_root.display().to_string(),
                reason: format!("directory has non-UTF-8 name: {}", path.display()),
            })?
            .to_string();
        if is_ignored_dir(&name) {
            continue;
        }
        let rel_root = format!("./{name}");
        if let Some(candidate) = classify_project(&path, &name, &rel_root)? {
            candidates.push(candidate);
        }
    }

    candidates.sort_by(|a, b| a.name.cmp(&b.name));

    // M8: workspace recursion. For each top-level candidate whose manifest
    // declares a workspace (`[workspace]` in Cargo.toml, `[tool.uv.workspace]` or
    // hatch packages in pyproject.toml), scan one level deeper for sub-package
    // manifests. Each sub-package becomes its own ProjectCandidate.
    let mut workspace_subs: Vec<ProjectCandidate> = Vec::new();
    for cand in &candidates {
        let abs_root = monorepo_root.join(cand.root_path.trim_start_matches("./"));
        // Mixed projects (e.g. arbiter — has both Cargo.toml + pyproject.toml at root)
        // may declare a workspace via EITHER manifest. Check both so the sub-crates of
        // a Rust workspace whose root also happens to be a Python project aren't missed.
        let declares = match cand.kind {
            ProjectKind::Rust => crate::parsers::rust::declares_workspace(&abs_root),
            ProjectKind::Python => crate::parsers::python::declares_workspace(&abs_root),
            ProjectKind::Mixed => {
                crate::parsers::python::declares_workspace(&abs_root)
                    || crate::parsers::rust::declares_workspace(&abs_root)
            }
            _ => false,
        };
        if !declares {
            continue;
        }

        // Per-project `[tool.prograph] exclude = [...]` list (Python side only —
        // pure-Rust projects can use `.prograph/config.toml` at the monorepo
        // level). Used to skip vendored archives / nested sub-modules that
        // happen to look like projects.
        let excludes = crate::parsers::python::read_prograph_excludes(&abs_root);

        // Collect declared workspace members (uv + Cargo, glob-expanded). These
        // give us nested members like `packages/atp-sdk` that read_dir at the
        // project root would miss.
        let mut declared_members: Vec<std::path::PathBuf> = Vec::new();
        let mut member_patterns: Vec<String> = Vec::new();
        match cand.kind {
            ProjectKind::Rust => {
                member_patterns.extend(crate::parsers::rust::workspace_members(&abs_root));
            }
            ProjectKind::Python => {
                member_patterns.extend(crate::parsers::python::workspace_members(&abs_root));
            }
            ProjectKind::Mixed => {
                member_patterns.extend(crate::parsers::python::workspace_members(&abs_root));
                member_patterns.extend(crate::parsers::rust::workspace_members(&abs_root));
            }
            _ => {}
        }
        for pat in &member_patterns {
            declared_members.extend(expand_member(&abs_root, pat));
        }

        // First pass: direct subdirs of the workspace root (existing behaviour —
        // catches sub-projects with their own signal files that aren't formally
        // declared workspace members, e.g. atp-platform's `docs/`).
        let entries =
            std::fs::read_dir(&abs_root).map_err(|source| crate::errors::PrographError::Io {
                path: abs_root.display().to_string(),
                source,
            })?;
        let mut already_added: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if is_ignored_dir(&name) || excludes.iter().any(|e| e == &name) {
                continue;
            }
            let sub_rel = format!("{}/{}", cand.root_path, name);
            if let Some(sub_candidate) = classify_project(&path, &name, &sub_rel)? {
                if sub_candidate.name == cand.name {
                    continue;
                }
                already_added.insert(sub_candidate.root_path.clone());
                workspace_subs.push(sub_candidate);
            }
        }

        // Second pass: declared workspace members that live deeper than one
        // level (e.g. `packages/atp-sdk`). Dedupe against pass-1 results.
        for member_path in declared_members {
            if !member_path.is_dir() {
                continue;
            }
            let name = match member_path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if is_ignored_dir(&name) {
                continue;
            }
            let rel = match member_path.strip_prefix(&abs_root) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            let sub_rel = format!("{}/{}", cand.root_path, rel);
            if already_added.contains(&sub_rel) {
                continue;
            }
            if let Some(sub_candidate) = classify_project(&member_path, &name, &sub_rel)? {
                if sub_candidate.name == cand.name {
                    continue;
                }
                already_added.insert(sub_candidate.root_path.clone());
                workspace_subs.push(sub_candidate);
            }
        }
    }

    candidates.extend(workspace_subs);
    candidates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(candidates)
}

fn is_ignored_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "target" | "node_modules" | "dist" | "build" | "__pycache__"
        )
}

/// Expand one workspace-member pattern (relative to `root`) into the list of
/// directory paths it refers to. Supports literal patterns like `"atp-games"`
/// and trailing-wildcard patterns like `"packages/*"` (the only glob form uv
/// and Cargo actually generate). Returns paths that exist and are directories.
fn expand_member(root: &Path, pattern: &str) -> Vec<std::path::PathBuf> {
    if let Some(idx) = pattern.find('*') {
        let prefix = &pattern[..idx];
        let suffix = &pattern[idx + 1..];
        // Only support `prefix/*` and `prefix/*suffix` — the wildcard must be
        // a path-segment boundary on the left. Anything else is unsupported.
        if !prefix.is_empty() && !prefix.ends_with('/') {
            return Vec::new();
        }
        // `suffix` (e.g. ".rs" theoretical) — unused in practice for workspace
        // members; only `prefix/*` form appears in real Cargo.toml / pyproject.toml.
        if !suffix.is_empty() && !suffix.starts_with('/') {
            return Vec::new();
        }
        let parent = root.join(prefix.trim_end_matches('/'));
        let Ok(entries) = std::fs::read_dir(&parent) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                out.push(p);
            }
        }
        out
    } else {
        let p = root.join(pattern);
        if p.is_dir() {
            vec![p]
        } else {
            Vec::new()
        }
    }
}

/// Python entry point: scan a monorepo, return the sorted list of candidates.
#[pyfunction]
#[pyo3(name = "scan_monorepo")]
pub fn py_scan_monorepo(monorepo_root: &str) -> PyResult<Vec<ProjectCandidate>> {
    Ok(scan_monorepo(Path::new(monorepo_root))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_proj(files: &[&str]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for f in files {
            fs::write(dir.path().join(f), "").unwrap();
        }
        dir
    }

    #[test]
    fn classifies_python_by_pyproject() {
        let dir = make_proj(&["pyproject.toml"]);
        let c = classify_project(dir.path(), "proj", "./proj")
            .unwrap()
            .unwrap();
        assert_eq!(c.kind, ProjectKind::Python);
        assert_eq!(c.manifests, vec!["pyproject.toml"]);
    }

    #[test]
    fn classifies_rust_by_cargo_toml() {
        let dir = make_proj(&["Cargo.toml"]);
        let c = classify_project(dir.path(), "proj", "./proj")
            .unwrap()
            .unwrap();
        assert_eq!(c.kind, ProjectKind::Rust);
    }

    #[test]
    fn classifies_js_by_package_json() {
        let dir = make_proj(&["package.json"]);
        let c = classify_project(dir.path(), "proj", "./proj")
            .unwrap()
            .unwrap();
        assert_eq!(c.kind, ProjectKind::Js);
    }

    #[test]
    fn classifies_docs_only_when_no_code_signals() {
        let dir = make_proj(&["README.md", "CLAUDE.md"]);
        let c = classify_project(dir.path(), "proj", "./proj")
            .unwrap()
            .unwrap();
        assert_eq!(c.kind, ProjectKind::Docs);
    }

    #[test]
    fn classifies_mixed_when_multiple_code_signals() {
        let dir = make_proj(&["pyproject.toml", "Cargo.toml"]);
        let c = classify_project(dir.path(), "proj", "./proj")
            .unwrap()
            .unwrap();
        assert_eq!(c.kind, ProjectKind::Mixed);
    }

    #[test]
    fn returns_none_when_no_signals() {
        let dir = TempDir::new().unwrap();
        assert!(classify_project(dir.path(), "x", "./x").unwrap().is_none());
    }

    #[test]
    fn returns_none_when_only_unrelated_files() {
        let dir = make_proj(&["foo.txt", "data.json"]);
        assert!(classify_project(dir.path(), "x", "./x").unwrap().is_none());
    }

    #[test]
    fn code_signal_wins_over_docs_for_kind() {
        let dir = make_proj(&["pyproject.toml", "README.md"]);
        let c = classify_project(dir.path(), "proj", "./proj")
            .unwrap()
            .unwrap();
        assert_eq!(c.kind, ProjectKind::Python);
        assert!(c.manifests.contains(&"pyproject.toml".to_string()));
        assert!(c.manifests.contains(&"README.md".to_string()));
    }

    #[test]
    fn scan_finds_two_projects_sorted() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("zeta")).unwrap();
        fs::write(dir.path().join("zeta/Cargo.toml"), "").unwrap();
        fs::create_dir_all(dir.path().join("alpha")).unwrap();
        fs::write(dir.path().join("alpha/pyproject.toml"), "").unwrap();

        let result = scan_monorepo(dir.path()).unwrap();
        let names: Vec<_> = result.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
    }

    #[test]
    fn scan_skips_hidden_and_artefact_dirs() {
        let dir = TempDir::new().unwrap();
        for hidden in &[".git", ".venv", "target", "node_modules"] {
            fs::create_dir_all(dir.path().join(hidden)).unwrap();
            fs::write(dir.path().join(hidden).join("Cargo.toml"), "").unwrap();
        }
        fs::create_dir_all(dir.path().join("real")).unwrap();
        fs::write(dir.path().join("real/Cargo.toml"), "").unwrap();

        let result = scan_monorepo(dir.path()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "real");
    }

    #[test]
    fn scan_errors_on_nonexistent_root() {
        let err = scan_monorepo(Path::new("/nonexistent_for_test_xyz")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not a directory"), "got: {msg}");
    }

    #[test]
    fn scan_empty_dir_returns_empty_vec() {
        let dir = TempDir::new().unwrap();
        assert!(scan_monorepo(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn uv_workspace_glob_member_packages_star_expands() {
        // Models atp-platform: `[tool.uv.workspace] members = ["packages/*"]`.
        // Pre-fix, packages/atp-sdk was nested one level too deep to be
        // discovered as a direct subdir.
        let dir = TempDir::new().unwrap();
        let proj = dir.path().join("atp-platform");
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            proj.join("pyproject.toml"),
            r#"[project]
name = "atp-platform"

[tool.uv.workspace]
members = ["packages/*"]
"#,
        )
        .unwrap();
        // Two glob-matched members.
        for sub in &["atp-sdk", "atp-core"] {
            let sub_dir = proj.join("packages").join(sub);
            fs::create_dir_all(&sub_dir).unwrap();
            fs::write(
                sub_dir.join("pyproject.toml"),
                format!("[project]\nname = \"atp-platform-{sub}\"\nversion = \"1.0\"\n"),
            )
            .unwrap();
        }

        let result = scan_monorepo(dir.path()).unwrap();
        let names: Vec<_> = result.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"atp-sdk"),
            "atp-sdk under packages/* not discovered: {names:?}"
        );
        assert!(
            names.contains(&"atp-core"),
            "atp-core under packages/* not discovered: {names:?}"
        );
    }

    #[test]
    fn cargo_workspace_glob_member_crates_star_expands() {
        let dir = TempDir::new().unwrap();
        let proj = dir.path().join("rs");
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            proj.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        for sub in &["alpha", "beta"] {
            let sub_dir = proj.join("crates").join(sub);
            fs::create_dir_all(&sub_dir).unwrap();
            fs::write(
                sub_dir.join("Cargo.toml"),
                format!("[package]\nname = \"{sub}\"\nversion = \"0.1.0\"\n"),
            )
            .unwrap();
        }
        let result = scan_monorepo(dir.path()).unwrap();
        let names: Vec<_> = result.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"alpha"), "got: {names:?}");
        assert!(names.contains(&"beta"), "got: {names:?}");
    }

    #[test]
    fn tool_prograph_exclude_skips_subdirs_during_recursion() {
        // Models prograph's own situation: it's a workspace (Cargo.toml has
        // [workspace]), but contains a vendored Sourcetrail/ sub-dir that
        // happens to have README.md → would otherwise be picked up as a
        // docs project. [tool.prograph] exclude in pyproject.toml suppresses it.
        let dir = TempDir::new().unwrap();
        let proj = dir.path().join("p");
        fs::create_dir_all(&proj).unwrap();
        fs::write(
            proj.join("pyproject.toml"),
            "[project]\nname = \"p\"\n\n[tool.prograph]\nexclude = [\"vendored\"]\n",
        )
        .unwrap();
        fs::write(
            proj.join("Cargo.toml"),
            "[workspace]\nmembers = [\"sub\"]\n",
        )
        .unwrap();
        // Real workspace member that SHOULD be picked up.
        fs::create_dir_all(proj.join("sub")).unwrap();
        fs::write(
            proj.join("sub/Cargo.toml"),
            "[package]\nname = \"sub\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        // Vendored archive that SHOULD be excluded.
        fs::create_dir_all(proj.join("vendored")).unwrap();
        fs::write(proj.join("vendored/README.md"), "# vendored archive").unwrap();

        let result = scan_monorepo(dir.path()).unwrap();
        let names: Vec<_> = result.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"p"), "missing parent: {names:?}");
        assert!(names.contains(&"sub"), "real member dropped: {names:?}");
        assert!(
            !names.contains(&"vendored"),
            "exclude failed — vendored picked up: {names:?}"
        );
    }

    #[test]
    fn mixed_project_with_rust_workspace_expands_subcrates() {
        // Reproduces the arbiter dogfood bug: a project root with BOTH
        // pyproject.toml and Cargo.toml (classified as Mixed) whose Rust manifest
        // declares `[workspace]`. Pre-fix, M8 only consulted Python's
        // declares_workspace for Mixed and silently dropped the Rust sub-crates.
        let dir = TempDir::new().unwrap();
        let proj = dir.path().join("arb");
        fs::create_dir_all(&proj).unwrap();
        // Both manifests at root → classified Mixed.
        fs::write(proj.join("pyproject.toml"), "[project]\nname = \"arb\"\n").unwrap();
        fs::write(
            proj.join("Cargo.toml"),
            "[workspace]\nmembers = [\"arb-mcp\", \"arb-core\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        // Two sub-crates.
        for sub in &["arb-mcp", "arb-core"] {
            fs::create_dir_all(proj.join(sub)).unwrap();
            fs::write(
                proj.join(sub).join("Cargo.toml"),
                format!("[package]\nname = \"{sub}\"\nversion = \"0.1.0\"\n"),
            )
            .unwrap();
        }

        let result = scan_monorepo(dir.path()).unwrap();
        let names: Vec<_> = result.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"arb"), "missing parent: {names:?}");
        assert!(
            names.contains(&"arb-mcp"),
            "Mixed-kind Rust workspace member not discovered: {names:?}"
        );
        assert!(
            names.contains(&"arb-core"),
            "Mixed-kind Rust workspace member not discovered: {names:?}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn scan_skips_symlinks_to_directories() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new().unwrap();
        // real project
        fs::create_dir_all(dir.path().join("real_proj")).unwrap();
        fs::write(dir.path().join("real_proj/Cargo.toml"), "").unwrap();
        // symlink that, if followed, would classify the same project under a second name
        symlink(
            dir.path().join("real_proj"),
            dir.path().join("symlink_proj"),
        )
        .unwrap();

        let result = scan_monorepo(dir.path()).unwrap();
        let names: Vec<_> = result.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["real_proj"]);
    }
}
