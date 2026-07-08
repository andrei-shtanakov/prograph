//! Intent extraction layer. Reads markdown intent docs (README/TODO/specs) and
//! produces an IntentDoc per project. Pure file I/O + line parsing — no
//! heavy markdown parsing dependency.

pub mod markdown;

use std::path::Path;

use crate::facts::IntentDoc;

const SCAN_FILES: &[&str] = &["README.md", "TODO.md"];
const SCAN_GLOB_DIRS: &[&str] = &["docs/superpowers/specs", "docs/specs"];

/// Read a project's intent. Returns Default if the project has no recognised
/// intent files (no error — most projects have at least README.md).
pub fn extract_intent(project_root: &Path) -> IntentDoc {
    let mut combined = IntentDoc::default();

    for rel in SCAN_FILES {
        let path = project_root.join(rel);
        if let Ok(text) = std::fs::read_to_string(&path) {
            if is_generated_doc(&text) {
                continue;
            }
            let doc = markdown::parse(&text, rel, rel.ends_with("TODO.md"));
            combined.items.extend(doc.items);
            combined.todos.extend(doc.todos);
        }
    }

    for spec_dir in SCAN_GLOB_DIRS {
        let dir = project_root.join(spec_dir);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut sorted: Vec<_> = entries.flatten().collect();
        sorted.sort_by_key(|e| e.path());
        for entry in sorted {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if is_generated_doc(&text) {
                continue;
            }
            let rel = path
                .strip_prefix(project_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let doc = markdown::parse(&text, &rel, false);
            combined.items.extend(doc.items);
            combined.todos.extend(doc.todos);
        }
    }

    combined
        .items
        .sort_by(|a, b| (&a.source_path, a.line, &a.name).cmp(&(&b.source_path, b.line, &b.name)));
    combined
        .todos
        .sort_by(|a, b| (&a.source_path, a.line).cmp(&(&b.source_path, b.line)));
    combined
}

/// Skip files prograph itself generated (M9 MD export, M11 drift section).
fn is_generated_doc(text: &str) -> bool {
    text.lines()
        .take(5)
        .any(|line| line.contains("<!-- prograph:generated -->"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn extracts_from_readme_and_todo() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("README.md"),
            "## MCP tools exposed\n- `t1`\n",
        )
        .unwrap();
        fs::write(dir.path().join("TODO.md"), "- [ ] open\n- [x] done\n").unwrap();
        let doc = extract_intent(dir.path());
        assert_eq!(doc.items.len(), 1);
        assert_eq!(doc.items[0].name, "t1");
        assert_eq!(doc.todos.len(), 2);
    }

    #[test]
    fn skips_generated_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("README.md"),
            "<!-- prograph:generated -->\n## Public surface\n- `Auto`\n",
        )
        .unwrap();
        let doc = extract_intent(dir.path());
        assert!(doc.items.is_empty(), "generated marker should exclude file");
    }

    #[test]
    fn scans_specs_directory() {
        let dir = tempfile::tempdir().unwrap();
        let specs = dir.path().join("docs/superpowers/specs");
        fs::create_dir_all(&specs).unwrap();
        fs::write(
            specs.join("2026-01-01-thing.md"),
            "## Contracts declared\n- `obs-v1`\n",
        )
        .unwrap();
        let doc = extract_intent(dir.path());
        assert_eq!(doc.items.len(), 1);
        assert_eq!(doc.items[0].name, "obs-v1");
        assert_eq!(
            doc.items[0].source_path,
            "docs/superpowers/specs/2026-01-01-thing.md"
        );
    }
}
