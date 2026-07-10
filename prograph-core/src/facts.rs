//! Cross-language data shapes parsers produce. Detectors consume slices of these.
//!
//! M2 only populates `Manifest`. M3+ adds `modules`, `mcp_decls`, `mcp_uses`, `contracts`
//! without breaking consumers — all fields are owned by the parser and read-only downstream.

use serde::{Deserialize, Serialize};

/// A single declared dependency in a project's manifest.
///
/// Identity-relevant fields: `name`. `version_req` is metadata (version bumps surface as
/// `attrs_changed` events in the change-log, not remove+add).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepRequirement {
    pub name: String,
    pub version_req: Option<String>,
}

/// A project's declared manifest — the canonical view downstream detectors use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Published package name (e.g. "atp-platform-sdk"), NOT the directory name.
    /// Detectors match consumers' `declared_deps[].name` against this field
    /// AND against any entry in `aliases`.
    pub declared_name: String,
    pub version: Option<String>,
    pub declared_deps: Vec<DepRequirement>,
    /// Additional names this project also publishes (workspace orchestrators).
    /// Populated from `[tool.prograph].aliases` in pyproject.toml. Empty by default.
    /// `#[serde(default)]` keeps M2 snapshots (no `aliases` field in `projects.attrs_json`)
    /// readable — backward-compat seam, do not remove.
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// Soft parse errors that should warn the user without aborting the snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseWarning {
    pub rel_path: String,
    pub message: String,
}

/// A JSON Schema / OpenAPI / .proto file discovered inside a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractFile {
    /// Path relative to the project root, e.g. "schemas/obs-v1.json".
    pub rel_path: String,
    pub kind: ContractKind,
    /// Optional declared identifier — `$id` for JSON Schema, `info.title` for OpenAPI,
    /// `package` name for .proto. None when undeclared.
    pub declared_id: Option<String>,
    /// SHA256 hex of the file content. Used as a fallback identity key when `declared_id`
    /// is None AND as part of the contract's full identity per spec §5.2.
    pub content_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContractKind {
    JsonSchema,
    OpenApi,
    Proto,
}

impl ContractKind {
    #[allow(dead_code)] // Used by store/indexer in later M4 tasks
    pub fn as_str(&self) -> &'static str {
        match self {
            ContractKind::JsonSchema => "json_schema",
            ContractKind::OpenApi => "openapi",
            ContractKind::Proto => "proto",
        }
    }
}

/// An MCP tool registered by a project (server-side declaration).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolDecl {
    pub tool_name: String,
    /// File path relative to the project root, e.g. "src/server.py".
    pub rel_path: String,
    /// 1-based line number where the declaration starts. Best-effort.
    pub line: u32,
}

/// M12: file-based integration declared in a manifest (`[tool.prograph] reads/writes`
/// in pyproject.toml, `[package.metadata.prograph]` in Cargo.toml).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeclaredMode {
    Read,
    Write,
}

impl DeclaredMode {
    #[allow(dead_code)] // Used by detectors/store in later M12 tasks
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// A single declared path with its manifest source location — captured by the
/// parser in one pass so evidence and stale checks never re-scan the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredPath {
    pub mode: DeclaredMode,
    /// Workspace-relative path exactly as declared (whitespace-trimmed).
    /// Validation/normalization happens in `detectors::declared`.
    pub path: String,
    /// Manifest file relative to the project root ("pyproject.toml" | "Cargo.toml").
    pub source_path: String,
    /// 1-based line of the entry in the manifest. Best-effort text scan.
    pub line: u32,
    pub snippet: Option<String>,
}

/// An MCP tool invocation by a project (client-side usage).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpClientUse {
    pub tool_name: String,
    pub rel_path: String,
    pub line: u32,
}

/// A source file inside a project with its public symbols + internal imports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Module {
    pub rel_path: String,
    pub language: String, // 'python' | 'rust' | 'js'
    #[serde(default)]
    pub public_symbols: Vec<PublicSymbol>,
    #[serde(default)]
    pub internal_imports: Vec<InternalImport>,
    /// M10: imports the parser couldn't classify as definitely-internal. The indexer's
    /// resolver layer decides which point at an in-monorepo project; the rest are dropped.
    #[serde(default)]
    pub external_imports: Vec<ExternalImport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Class,
    Struct,
    Enum,
    Trait,
    Const,
    Type,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::Function => "function",
            SymbolKind::Class => "class",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Const => "const",
            SymbolKind::Type => "type",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InternalImport {
    /// Dotted (Python) or `::`-separated (Rust) or relative (JS) path.
    pub target_path: String,
    pub line: u32,
}

/// M10: an import that may or may not resolve to an in-monorepo publisher.
/// The resolver layer decides; un-resolved entries are silently dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalImport {
    /// Dotted (Python) or `::`-separated (Rust) path. Always includes the leading
    /// segment (the package or crate name) — the resolver uses it to locate the publisher.
    pub target_path: String,
    /// If the import explicitly named a symbol (`from x.y import Z`), this is `Z`.
    /// For module-only imports (`import x.y`), this is None.
    pub target_symbol: Option<String>,
    pub line: u32,
}

/// Per-project bundle of extracted facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFacts {
    /// Project identifier inside the current scan — populated by indexer, not parsers.
    pub project_root: String, // relative path, matches projects.root_path
    pub project_name: String,
    pub manifest: Option<Manifest>,
    pub warnings: Vec<ParseWarning>,
    pub parse_status: ParseStatus,
    /// MCP tool declarations found in this project's source (M4+).
    #[serde(default)]
    pub mcp_decls: Vec<McpToolDecl>,
    /// MCP tool usages found in this project's source (M4+).
    #[serde(default)]
    pub mcp_uses: Vec<McpClientUse>,
    /// Contract files found in this project (M4+).
    #[serde(default)]
    pub contracts: Vec<ContractFile>,
    /// M9: per-file source-level facts.
    #[serde(default)]
    pub modules: Vec<Module>,
    /// M11: intent extracted from this project's markdown docs.
    #[serde(default)]
    pub intent: IntentDoc,
    /// M12: file-based integrations declared in the manifest.
    #[serde(default)]
    pub declared_paths: Vec<DeclaredPath>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParseStatus {
    Ok,
    Partial,
    Failed,
}

/// M11: Declared item extracted from a project's intent docs (README/TODO/specs).
/// Section heading determines `kind`. Example: under "## MCP tools exposed" the
/// list item `` `report_decision` `` produces IntentItem { kind: McpTool, name: "report_decision" }.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentItem {
    pub kind: IntentItemKind,
    pub name: String,
    /// Rel path from project root.
    pub source_path: String,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentItemKind {
    PublicSymbol,
    McpTool,
    Contract,
}

/// M11: A TODO checkbox harvested from a project's TODO.md (or any markdown's
/// "## TODO" section). Both checked and unchecked are emitted so the drift
/// detector can decide what to flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    /// The text after `[ ]` / `[x]`. Whitespace-trimmed. Truncated to 200 chars.
    pub text: String,
    pub checked: bool,
    pub source_path: String,
    pub line: u32,
}

/// M11: All intent extracted from a single project's docs. Empty when the
/// project has no recognised intent markers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IntentDoc {
    pub items: Vec<IntentItem>,
    pub todos: Vec<TodoItem>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips_via_serde() {
        let m = Manifest {
            declared_name: "atp-platform".into(),
            version: Some("1.0.0".into()),
            declared_deps: vec![DepRequirement {
                name: "spec-runner".into(),
                version_req: Some(">=0.1.4".into()),
            }],
            aliases: vec!["atp-platform-sdk".into(), "atp-platform-cli".into()],
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn manifest_aliases_default_empty_when_missing() {
        let json = r#"{"declared_name":"x","version":null,"declared_deps":[]}"#;
        let m: Manifest = serde_json::from_str(json).unwrap();
        assert!(m.aliases.is_empty());
    }

    #[test]
    fn dep_requirement_without_version_serializes_null() {
        let d = DepRequirement {
            name: "x".into(),
            version_req: None,
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"version_req\":null"));
    }

    #[test]
    fn contract_kind_as_str_matches_schema_check() {
        assert_eq!(ContractKind::JsonSchema.as_str(), "json_schema");
        assert_eq!(ContractKind::OpenApi.as_str(), "openapi");
        assert_eq!(ContractKind::Proto.as_str(), "proto");
    }

    #[test]
    fn project_facts_round_trip_with_new_fields() {
        let f = ProjectFacts {
            project_root: "./p".into(),
            project_name: "p".into(),
            manifest: None,
            warnings: vec![],
            parse_status: ParseStatus::Ok,
            mcp_decls: vec![McpToolDecl {
                tool_name: "decide".into(),
                rel_path: "src/lib.rs".into(),
                line: 42,
            }],
            mcp_uses: vec![],
            contracts: vec![ContractFile {
                rel_path: "schemas/obs.json".into(),
                kind: ContractKind::JsonSchema,
                declared_id: Some("https://example.org/schemas/obs-v1".into()),
                content_hash: "deadbeef".repeat(8),
            }],
            modules: vec![],
            intent: Default::default(),
            declared_paths: vec![],
        };
        let json = serde_json::to_string(&f).unwrap();
        let back: ProjectFacts = serde_json::from_str(&json).unwrap();
        assert_eq!(back.mcp_decls.len(), 1);
        assert_eq!(back.contracts.len(), 1);
        assert_eq!(back.contracts[0].kind, ContractKind::JsonSchema);
    }

    #[test]
    fn project_facts_back_compat_without_new_fields() {
        // M2/M3 serialized ProjectFacts without mcp_decls/mcp_uses/contracts.
        let json = r#"{
            "project_root": "./p",
            "project_name": "p",
            "manifest": null,
            "warnings": [],
            "parse_status": "Ok"
        }"#;
        let back: ProjectFacts = serde_json::from_str(json).unwrap();
        assert!(back.mcp_decls.is_empty());
        assert!(back.mcp_uses.is_empty());
        assert!(back.contracts.is_empty());
        assert!(back.modules.is_empty());
    }

    #[test]
    fn module_round_trips_via_serde() {
        let m = Module {
            rel_path: "src/lib.rs".into(),
            language: "rust".into(),
            public_symbols: vec![PublicSymbol {
                name: "Decider".into(),
                kind: SymbolKind::Struct,
                line: 42,
            }],
            internal_imports: vec![InternalImport {
                target_path: "crate::policy".into(),
                line: 3,
            }],
            external_imports: vec![],
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: Module = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn external_import_round_trips() {
        let e = ExternalImport {
            target_path: "atp_platform.sdk".into(),
            target_symbol: Some("MaestroATPAdapter".into()),
            line: 18,
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: ExternalImport = serde_json::from_str(&json).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn module_back_compat_without_external_imports() {
        let json = r#"{
            "rel_path": "x.py",
            "language": "python"
        }"#;
        let m: Module = serde_json::from_str(json).unwrap();
        assert!(m.external_imports.is_empty());
    }

    #[test]
    fn symbol_kind_as_str_matches_renderer_strings() {
        assert_eq!(SymbolKind::Function.as_str(), "function");
        assert_eq!(SymbolKind::Class.as_str(), "class");
        assert_eq!(SymbolKind::Struct.as_str(), "struct");
        assert_eq!(SymbolKind::Enum.as_str(), "enum");
        assert_eq!(SymbolKind::Trait.as_str(), "trait");
        assert_eq!(SymbolKind::Const.as_str(), "const");
        assert_eq!(SymbolKind::Type.as_str(), "type");
    }

    #[test]
    fn intent_item_round_trips() {
        let i = IntentItem {
            kind: IntentItemKind::McpTool,
            name: "report_decision".into(),
            source_path: "README.md".into(),
            line: 42,
        };
        let j = serde_json::to_string(&i).unwrap();
        let back: IntentItem = serde_json::from_str(&j).unwrap();
        assert_eq!(i, back);
    }

    #[test]
    fn todo_item_round_trips() {
        let t = TodoItem {
            text: "fix retry path".into(),
            checked: false,
            source_path: "TODO.md".into(),
            line: 3,
        };
        let j = serde_json::to_string(&t).unwrap();
        let back: TodoItem = serde_json::from_str(&j).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn project_facts_back_compat_without_intent() {
        // Older snapshots (M2-M10) had no `intent` field.
        let json = r#"{
            "project_root": "x",
            "project_name": "x",
            "manifest": null,
            "warnings": [],
            "parse_status": "Ok",
            "mcp_decls": [],
            "mcp_uses": [],
            "contracts": [],
            "modules": []
        }"#;
        let f: ProjectFacts = serde_json::from_str(json).unwrap();
        assert!(f.intent.items.is_empty());
        assert!(f.intent.todos.is_empty());
    }
}
