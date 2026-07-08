//! Line-based markdown intent parser. Recognises:
//! - H2 headings matching known synonym sets → opens a section that emits IntentItems
//! - List items with inline-code identifiers (`` `name` ``) → items within an open section
//! - Checkbox list items (`- [ ]` / `- [x]`) → TodoItems (within TODO context: TODO.md or under ## TODO heading)

use once_cell::sync::Lazy;
use regex::Regex;

use crate::facts::{IntentDoc, IntentItem, IntentItemKind, TodoItem};

static H2_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^##\s+(.+?)\s*$").unwrap());
static LIST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*[-*]\s+(.+?)\s*$").unwrap());
static CHECKBOX_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*[-*]\s+\[([ xX])\]\s+(.+?)\s*$").unwrap());
static INLINE_CODE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"`([^`]+)`").unwrap());

enum HeadingKind {
    Intent(IntentItemKind),
    Todo,
}

fn classify_heading(heading: &str) -> Option<HeadingKind> {
    let lc = heading.to_lowercase();
    // Strip leading non-alphabetic chars (emoji, "✅ ", etc.).
    let core: &str = lc.trim_start_matches(|c: char| !c.is_alphabetic()).trim();
    match core {
        "public surface" | "public api" | "api" | "exports" | "public symbols" => {
            Some(HeadingKind::Intent(IntentItemKind::PublicSymbol))
        }
        "mcp tools exposed" | "mcp tools" => Some(HeadingKind::Intent(IntentItemKind::McpTool)),
        "contracts declared" | "contracts" => Some(HeadingKind::Intent(IntentItemKind::Contract)),
        "todo" | "todos" | "to do" => Some(HeadingKind::Todo),
        _ => None,
    }
}

pub fn parse(text: &str, source_path: &str, file_is_todo: bool) -> IntentDoc {
    let mut items: Vec<IntentItem> = Vec::new();
    let mut todos: Vec<TodoItem> = Vec::new();
    let mut current_intent_kind: Option<IntentItemKind> = None;
    let mut in_todo_section: bool = file_is_todo;

    for (idx, line) in text.lines().enumerate() {
        let line_no = (idx + 1) as u32;

        if let Some(caps) = H2_RE.captures(line) {
            let heading_text = &caps[1];
            current_intent_kind = None;
            if !file_is_todo {
                in_todo_section = false;
            }
            match classify_heading(heading_text) {
                Some(HeadingKind::Intent(k)) => current_intent_kind = Some(k),
                Some(HeadingKind::Todo) => in_todo_section = true,
                None => {}
            }
            continue;
        }

        // H1 / H3+ reset both contexts (when not in TODO.md whole-file mode).
        if line.starts_with('#') {
            current_intent_kind = None;
            if !file_is_todo {
                in_todo_section = false;
            }
            continue;
        }

        // Checkbox: emit TodoItem only if we're in a todo context.
        if in_todo_section {
            if let Some(caps) = CHECKBOX_RE.captures(line) {
                let mark = &caps[1];
                let body = &caps[2];
                let checked = mark == "x" || mark == "X";
                let truncated: String = body.chars().take(200).collect();
                todos.push(TodoItem {
                    text: truncated,
                    checked,
                    source_path: source_path.to_string(),
                    line: line_no,
                });
                continue;
            }
        }

        // Intent item: list line with inline-code identifier.
        if let Some(kind) = current_intent_kind {
            if let Some(caps) = LIST_RE.captures(line) {
                let bullet = &caps[1];
                if let Some(code) = INLINE_CODE_RE.captures(bullet) {
                    let name = normalize_identifier(&code[1]);
                    items.push(IntentItem {
                        kind,
                        name,
                        source_path: source_path.to_string(),
                        line: line_no,
                    });
                }
            }
        }
    }

    IntentDoc { items, todos }
}

/// Strip parameter signatures / annotations from a backtick-quoted identifier so
/// that intent items like `` `get_current_state(tournament_id)` `` compare cleanly
/// against the bare `get_current_state` symbol name produced by the parsers.
///
/// Why: specs often write tool signatures with parens (`fn(arg: T)`) while
/// `mcp_decls` / `public_symbols` only carry the identifier. Without this
/// normalisation the M11 drift detector double-reports the same symbol as both
/// missing (spec form) and extra (impl form). See dogfood report 2026-05-27.
fn normalize_identifier(raw: &str) -> String {
    // First trim and split on common separator chars. Take the part before the
    // first whitespace, paren, square bracket, or colon — that's the identifier.
    let trimmed = raw.trim();
    let cutoff = trimmed
        .find(|c: char| c == '(' || c == '[' || c == ':' || c.is_whitespace())
        .unwrap_or(trimmed.len());
    trimmed[..cutoff].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_public_surface_section() {
        let text = "# Foo\n\n## Public surface\n\n- `MaestroAPI` (class)\n- `report_decision` (function)\n\n## Other\n- `not_extracted`\n";
        let doc = parse(text, "README.md", false);
        assert_eq!(doc.items.len(), 2);
        assert_eq!(doc.items[0].name, "MaestroAPI");
        assert_eq!(doc.items[0].kind, IntentItemKind::PublicSymbol);
        assert_eq!(doc.items[1].name, "report_decision");
    }

    #[test]
    fn parses_mcp_tools_section() {
        let text = "## MCP tools exposed\n\n- `find_drifts` — surfaces M11 drift\n- `find_symbol_references`\n";
        let doc = parse(text, "README.md", false);
        assert_eq!(doc.items.len(), 2);
        assert!(doc.items.iter().all(|i| i.kind == IntentItemKind::McpTool));
    }

    #[test]
    fn parses_contracts_section_with_aliases() {
        for heading in &["## Contracts declared", "## Contracts"] {
            let text = format!("{heading}\n- `obs-v1`\n");
            let doc = parse(&text, "README.md", false);
            assert_eq!(doc.items.len(), 1, "heading {heading} should match");
            assert_eq!(doc.items[0].kind, IntentItemKind::Contract);
        }
    }

    #[test]
    fn extracts_todo_checkboxes_from_dedicated_file() {
        let text = "# TODO\n\n- [ ] LABS-87 retry path\n- [x] M9 done\n  - [ ] nested still open\n";
        let doc = parse(text, "TODO.md", true);
        assert_eq!(doc.todos.len(), 3);
        assert!(!doc.todos[0].checked);
        assert_eq!(doc.todos[0].text, "LABS-87 retry path");
        assert!(doc.todos[1].checked);
        assert_eq!(doc.todos[2].text, "nested still open");
    }

    #[test]
    fn extracts_todos_only_under_todo_heading_in_non_todo_file() {
        let text = "## API\n- [ ] not a todo, in api\n\n## TODO\n- [ ] real todo\n";
        let doc = parse(text, "README.md", false);
        assert_eq!(doc.todos.len(), 1);
        assert_eq!(doc.todos[0].text, "real todo");
    }

    #[test]
    fn list_items_without_inline_code_are_skipped() {
        let text = "## Public surface\n\n- plain text, no backticks\n- `WithCode`\n";
        let doc = parse(text, "README.md", false);
        assert_eq!(doc.items.len(), 1);
        assert_eq!(doc.items[0].name, "WithCode");
    }

    #[test]
    fn truncates_long_todo_text() {
        let long = "x".repeat(500);
        let text = format!("- [ ] {long}");
        let doc = parse(&text, "TODO.md", true);
        assert_eq!(doc.todos.len(), 1);
        assert_eq!(doc.todos[0].text.len(), 200);
    }

    #[test]
    fn normalizes_signature_form_to_identifier() {
        let text = "## MCP tools exposed\n- `get_current_state(tournament_id)`\n- `list_tournaments(game_type: str | None = None)`\n- `make_move`\n";
        let doc = parse(text, "spec.md", false);
        assert_eq!(doc.items.len(), 3);
        assert_eq!(doc.items[0].name, "get_current_state");
        assert_eq!(doc.items[1].name, "list_tournaments");
        assert_eq!(doc.items[2].name, "make_move");
    }

    #[test]
    fn normalize_identifier_strips_type_annotations() {
        assert_eq!(normalize_identifier("Foo"), "Foo");
        assert_eq!(normalize_identifier("foo(bar)"), "foo");
        assert_eq!(normalize_identifier("foo[T]"), "foo");
        assert_eq!(normalize_identifier("foo: Bar"), "foo");
        assert_eq!(normalize_identifier("foo bar"), "foo");
        assert_eq!(normalize_identifier("  baz  "), "baz");
    }

    #[test]
    fn unknown_heading_resets_intent_kind() {
        let text = "## Public surface\n- `A`\n\n## Random\n- `B`\n";
        let doc = parse(text, "README.md", false);
        assert_eq!(doc.items.len(), 1);
        assert_eq!(doc.items[0].name, "A");
    }
}
