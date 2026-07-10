//! Drift detection — compares declared intent against detected reality and
//! produces DriftFinding records. Pure functions; no I/O.

use crate::facts::{IntentItemKind, ProjectFacts};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftFinding {
    pub kind: DriftKind,
    pub entity_kind: EntityKind,
    pub entity_name: String,
    pub source_path: String,
    pub source_line: u32,
    pub confidence: Confidence,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftKind {
    Missing,
    Extra,
    StaleTodo,
    StaleDeclaration,
}

impl DriftKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Extra => "extra",
            Self::StaleTodo => "stale_todo",
            Self::StaleDeclaration => "stale_declaration",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    PublicSymbol,
    McpTool,
    Contract,
    Todo,
    DeclaredPath,
}

impl EntityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PublicSymbol => "public_symbol",
            Self::McpTool => "mcp_tool",
            Self::Contract => "contract",
            Self::Todo => "todo",
            Self::DeclaredPath => "declared_path",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Low,
}

impl Confidence {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Low => "low",
        }
    }
}

/// Helper: extract the "name" of a contract for intent comparison. Prefer the
/// declared_id (e.g. JSON Schema's `$id`); fall back to the file's rel_path.
fn contract_name(c: &crate::facts::ContractFile) -> &str {
    c.declared_id.as_deref().unwrap_or(c.rel_path.as_str())
}

/// Run all three detectors over a single project's facts (+ optional recent
/// change_log labels for stale-TODO matching).
#[allow(dead_code)]
pub fn detect_all(facts: &ProjectFacts, recent_changelog: &[String]) -> Vec<DriftFinding> {
    let mut out = Vec::new();
    out.extend(detect_missing(facts));
    out.extend(detect_extra(facts));
    out.extend(detect_stale_todos(facts, recent_changelog));
    out.sort_by(|a, b| {
        (a.kind.as_str(), a.entity_kind.as_str(), &a.entity_name).cmp(&(
            b.kind.as_str(),
            b.entity_kind.as_str(),
            &b.entity_name,
        ))
    });
    out
}

/// Missing: declared in intent, not found in facts.
pub fn detect_missing(facts: &ProjectFacts) -> Vec<DriftFinding> {
    let mut out = Vec::new();

    // Public symbols.
    let actual_symbols: std::collections::HashSet<&str> = facts
        .modules
        .iter()
        .flat_map(|m| m.public_symbols.iter().map(|s| s.name.as_str()))
        .collect();
    for item in &facts.intent.items {
        if item.kind != IntentItemKind::PublicSymbol {
            continue;
        }
        if !actual_symbols.contains(item.name.as_str()) {
            out.push(DriftFinding {
                kind: DriftKind::Missing,
                entity_kind: EntityKind::PublicSymbol,
                entity_name: item.name.clone(),
                source_path: item.source_path.clone(),
                source_line: item.line,
                confidence: Confidence::High,
                detail: Some(format!(
                    "declared in {}:{}, no matching public symbol found",
                    item.source_path, item.line
                )),
            });
        }
    }

    // MCP tools.
    let actual_tools: std::collections::HashSet<&str> = facts
        .mcp_decls
        .iter()
        .map(|t| t.tool_name.as_str())
        .collect();
    for item in &facts.intent.items {
        if item.kind != IntentItemKind::McpTool {
            continue;
        }
        if !actual_tools.contains(item.name.as_str()) {
            out.push(DriftFinding {
                kind: DriftKind::Missing,
                entity_kind: EntityKind::McpTool,
                entity_name: item.name.clone(),
                source_path: item.source_path.clone(),
                source_line: item.line,
                confidence: Confidence::High,
                detail: Some(format!(
                    "declared in {}:{}, no McpToolDecl found",
                    item.source_path, item.line
                )),
            });
        }
    }

    // Contracts.
    let actual_contracts: std::collections::HashSet<&str> =
        facts.contracts.iter().map(contract_name).collect();
    for item in &facts.intent.items {
        if item.kind != IntentItemKind::Contract {
            continue;
        }
        if !actual_contracts.contains(item.name.as_str()) {
            out.push(DriftFinding {
                kind: DriftKind::Missing,
                entity_kind: EntityKind::Contract,
                entity_name: item.name.clone(),
                source_path: item.source_path.clone(),
                source_line: item.line,
                confidence: Confidence::High,
                detail: Some(format!(
                    "declared in {}:{}, no ContractFile found",
                    item.source_path, item.line
                )),
            });
        }
    }

    out
}

/// Extra: present in facts, NOT declared in intent. Only emits findings when
/// intent for that kind exists AT ALL — otherwise we'd flood with "extra" on
/// projects that haven't written intent docs yet.
pub fn detect_extra(facts: &ProjectFacts) -> Vec<DriftFinding> {
    let mut out = Vec::new();

    let declared_symbols: std::collections::HashSet<&str> = facts
        .intent
        .items
        .iter()
        .filter(|i| i.kind == IntentItemKind::PublicSymbol)
        .map(|i| i.name.as_str())
        .collect();
    let declared_tools: std::collections::HashSet<&str> = facts
        .intent
        .items
        .iter()
        .filter(|i| i.kind == IntentItemKind::McpTool)
        .map(|i| i.name.as_str())
        .collect();
    let declared_contracts: std::collections::HashSet<&str> = facts
        .intent
        .items
        .iter()
        .filter(|i| i.kind == IntentItemKind::Contract)
        .map(|i| i.name.as_str())
        .collect();

    if !declared_symbols.is_empty() {
        for module in &facts.modules {
            for sym in &module.public_symbols {
                if !declared_symbols.contains(sym.name.as_str()) {
                    out.push(DriftFinding {
                        kind: DriftKind::Extra,
                        entity_kind: EntityKind::PublicSymbol,
                        entity_name: sym.name.clone(),
                        source_path: module.rel_path.clone(),
                        source_line: sym.line,
                        confidence: Confidence::High,
                        detail: Some(format!(
                            "public symbol in {}:{} not listed in intent docs",
                            module.rel_path, sym.line
                        )),
                    });
                }
            }
        }
    }

    if !declared_tools.is_empty() {
        for tool in &facts.mcp_decls {
            if !declared_tools.contains(tool.tool_name.as_str()) {
                out.push(DriftFinding {
                    kind: DriftKind::Extra,
                    entity_kind: EntityKind::McpTool,
                    entity_name: tool.tool_name.clone(),
                    source_path: tool.rel_path.clone(),
                    source_line: tool.line,
                    confidence: Confidence::High,
                    detail: Some(format!(
                        "MCP tool decl in {}:{} not listed in intent docs",
                        tool.rel_path, tool.line
                    )),
                });
            }
        }
    }

    if !declared_contracts.is_empty() {
        for c in &facts.contracts {
            let name = contract_name(c);
            if !declared_contracts.contains(name) {
                out.push(DriftFinding {
                    kind: DriftKind::Extra,
                    entity_kind: EntityKind::Contract,
                    entity_name: name.to_string(),
                    source_path: c.rel_path.clone(),
                    source_line: 0,
                    confidence: Confidence::High,
                    detail: Some(format!(
                        "contract file {} not listed in intent docs",
                        c.rel_path
                    )),
                });
            }
        }
    }

    out
}

/// Stale TODO: an unchecked TODO whose text overlaps with a recent change_log label.
/// Confidence=low because tokenisation is fuzzy.
pub fn detect_stale_todos(facts: &ProjectFacts, recent_changelog: &[String]) -> Vec<DriftFinding> {
    let mut out = Vec::new();

    for todo in &facts.intent.todos {
        if todo.checked {
            continue;
        }

        let todo_tokens = significant_tokens(&todo.text);
        if todo_tokens.is_empty() {
            continue;
        }

        for log_label in recent_changelog {
            let log_tokens = significant_tokens(log_label);
            let overlap: std::collections::HashSet<&String> =
                todo_tokens.intersection(&log_tokens).collect();
            if overlap.len() >= 2 || matches_strong_token(&overlap) {
                let matched: Vec<String> = overlap.iter().map(|s| (*s).clone()).collect();
                out.push(DriftFinding {
                    kind: DriftKind::StaleTodo,
                    entity_kind: EntityKind::Todo,
                    entity_name: todo.text.clone(),
                    source_path: todo.source_path.clone(),
                    source_line: todo.line,
                    confidence: Confidence::Low,
                    detail: Some(format!(
                        "open TODO matches recent change_log: {}",
                        matched.join(",")
                    )),
                });
                break; // one match is enough.
            }
        }
    }

    out
}

/// Extract identifier-shaped tokens. Lowercases. Strips punctuation. Drops
/// stopwords + very short tokens.
fn significant_tokens(text: &str) -> std::collections::HashSet<String> {
    let lc = text.to_lowercase();
    lc.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|t| t.len() >= 4)
        .filter(|t| !STOPWORDS.contains(t))
        .map(String::from)
        .collect()
}

/// Strong tokens (e.g. "labs-87") are sufficient on their own — no 2-token rule.
fn matches_strong_token(overlap: &std::collections::HashSet<&String>) -> bool {
    overlap.iter().any(|t| {
        let bytes = t.as_bytes();
        bytes.iter().any(|&b| b == b'-') && bytes.iter().any(|&b| b.is_ascii_digit())
    })
}

const STOPWORDS: &[&str] = &[
    "with", "from", "this", "that", "have", "been", "into", "more", "than", "what", "when",
    "while", "where", "will", "your", "just", "also", "some", "after", "before", "about", "their",
    "there", "should",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::*;

    fn empty_facts() -> ProjectFacts {
        ProjectFacts {
            project_root: ".".into(),
            project_name: "p".into(),
            manifest: None,
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: vec![],
            mcp_uses: vec![],
            contracts: vec![],
            modules: vec![],
            intent: IntentDoc::default(),
            declared_paths: vec![],
        }
    }

    #[test]
    fn detect_missing_finds_undeclared_symbol() {
        let mut facts = empty_facts();
        facts.intent.items.push(IntentItem {
            kind: IntentItemKind::PublicSymbol,
            name: "MaestroAPI".into(),
            source_path: "README.md".into(),
            line: 10,
        });
        let drifts = detect_missing(&facts);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].entity_name, "MaestroAPI");
        assert_eq!(drifts[0].kind, DriftKind::Missing);
        assert_eq!(drifts[0].confidence, Confidence::High);
    }

    #[test]
    fn detect_missing_skips_present_symbol() {
        let mut facts = empty_facts();
        facts.intent.items.push(IntentItem {
            kind: IntentItemKind::PublicSymbol,
            name: "X".into(),
            source_path: "README.md".into(),
            line: 1,
        });
        facts.modules.push(Module {
            rel_path: "a.py".into(),
            language: "python".into(),
            public_symbols: vec![PublicSymbol {
                name: "X".into(),
                kind: SymbolKind::Class,
                line: 5,
            }],
            internal_imports: vec![],
            external_imports: vec![],
        });
        let drifts = detect_missing(&facts);
        assert!(drifts.is_empty());
    }

    #[test]
    fn detect_extra_only_fires_when_intent_exists_for_kind() {
        let mut facts = empty_facts();
        facts.modules.push(Module {
            rel_path: "a.py".into(),
            language: "python".into(),
            public_symbols: vec![PublicSymbol {
                name: "Anything".into(),
                kind: SymbolKind::Function,
                line: 1,
            }],
            internal_imports: vec![],
            external_imports: vec![],
        });
        let drifts = detect_extra(&facts);
        assert!(
            drifts.is_empty(),
            "no intent docs → no extra drift (project hasn't opted in)"
        );
    }

    #[test]
    fn detect_extra_fires_when_some_intent_declared() {
        let mut facts = empty_facts();
        facts.intent.items.push(IntentItem {
            kind: IntentItemKind::PublicSymbol,
            name: "Declared".into(),
            source_path: "README.md".into(),
            line: 1,
        });
        facts.modules.push(Module {
            rel_path: "a.py".into(),
            language: "python".into(),
            public_symbols: vec![
                PublicSymbol {
                    name: "Declared".into(),
                    kind: SymbolKind::Class,
                    line: 5,
                },
                PublicSymbol {
                    name: "Undocumented".into(),
                    kind: SymbolKind::Class,
                    line: 10,
                },
            ],
            internal_imports: vec![],
            external_imports: vec![],
        });
        let drifts = detect_extra(&facts);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].entity_name, "Undocumented");
    }

    #[test]
    fn detect_stale_todo_matches_strong_ticket_id() {
        let mut facts = empty_facts();
        facts.intent.todos.push(TodoItem {
            text: "LABS-87 retry path".into(),
            checked: false,
            source_path: "TODO.md".into(),
            line: 3,
        });
        let log = vec!["fix LABS-87 retry logic".to_string()];
        let drifts = detect_stale_todos(&facts, &log);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].kind, DriftKind::StaleTodo);
        assert_eq!(drifts[0].confidence, Confidence::Low);
    }

    #[test]
    fn detect_stale_todo_skips_checked() {
        let mut facts = empty_facts();
        facts.intent.todos.push(TodoItem {
            text: "LABS-87 retry path".into(),
            checked: true,
            source_path: "TODO.md".into(),
            line: 3,
        });
        let log = vec!["fix LABS-87 retry logic".to_string()];
        let drifts = detect_stale_todos(&facts, &log);
        assert!(drifts.is_empty());
    }

    #[test]
    fn detect_stale_todo_requires_2_token_overlap_without_strong_id() {
        let mut facts = empty_facts();
        facts.intent.todos.push(TodoItem {
            text: "implement caching layer".into(),
            checked: false,
            source_path: "TODO.md".into(),
            line: 1,
        });
        let log = vec!["docs: explain caching".to_string()];
        assert!(detect_stale_todos(&facts, &log).is_empty());

        let log = vec!["add caching layer to store".to_string()];
        assert_eq!(detect_stale_todos(&facts, &log).len(), 1);
    }
}
