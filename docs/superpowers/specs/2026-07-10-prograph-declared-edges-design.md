# prograph — declared edges (file-based integrations)

**Date:** 2026-07-10
**Status:** Accepted (design approved in session, review corrections applied)

## Problem

Integrations implemented as *reading/writing another project's files on disk* are invisible
to all three detectors (deps / contracts / mcp). Case study: `dispatcher/core/collectors/`
reads proctor's `config/proctor.yaml`, `data/state.db` and logs — no import, no MCP call,
no shared contract file — so the graph shows proctor fully isolated while the integration
map has dispatcher ↔ proctor connected. Dispatcher is built this way on purpose ("reads
on-disk artifacts, projects need not be running"), so ALL of its edges are invisible.
Such integrations cannot be detected statically with acceptable reliability — but they can
be **declared** by the integrating project and rendered as first-class (visually distinct)
edges.

## Decisions (made with the user)

- Declaration lives in the **consumer's manifest**, plain string lists:
  `[tool.prograph] reads = [...] / writes = [...]` in `pyproject.toml`, and
  `[package.metadata.prograph] reads = [...] / writes = [...]` in `Cargo.toml`.
- **Both modes in v1** (`reads` + `writes`); edge direction is always
  declarer → target project, `mode` is an attribute.
- Broken declarations: unresolvable path prefix → **warning** (counted in `n_warnings`,
  no edge); resolved project but path missing on disk → **drift finding**
  `stale_declaration` (new DriftKind).

## Declaration syntax

```toml
# pyproject.toml (dispatcher)
[tool.prograph]
reads  = ["proctor/config/proctor.yaml", "proctor/data/state.db"]
writes = ["prograph-vault/derived/"]

# Cargo.toml (rust project)
[package.metadata.prograph]
reads = ["maestro/out/plan.json"]
```

Paths are **workspace-relative** (from the monorepo root); the first path segment(s)
identify the target project directory. A path may point at a file or a directory
(trailing `/` optional).

### Path normalization and validation

- Normalize both sides before matching: strip leading `./` and leading `/`, strip a
  trailing `/`.
- **Reject with a warning (no edge):** absolute paths, paths containing `..` segments,
  empty strings.
- **Prefix matching is segment-aware:** `proctor/data/x` matches project root `proctor`,
  but `proctor2/data/x` does NOT match `proctor` (compare whole path segments, not string
  prefixes — same trailing-`/` guard as `tracked_closure`).
- **Longest match wins:** `atp-platform/packages/atp-sdk/x.json` resolves to the nested
  workspace member `atp-sdk` (root `atp-platform/packages/atp-sdk`), not to `atp-platform`.
- Self-referencing declarations (path resolves to the declaring project) → warning, no
  edge (a project reading its own files is not an integration).

## Facts model — `DeclaredPath`

A declaration is a full fact with source location, captured by the parser in one pass
(re-scanning manifests later for evidence would risk desync):

```rust
pub enum DeclaredMode { Read, Write }

pub struct DeclaredPath {
    pub mode: DeclaredMode,
    pub path: String,        // normalized workspace-relative path as declared
    pub source_path: String, // "pyproject.toml" | "Cargo.toml" (relative to project root)
    pub line: i64,           // 1-based line of the entry in the manifest
    pub snippet: Option<String>,
}
```

`ProjectFacts` gains `declared_paths: Vec<DeclaredPath>` (serde `#[serde(default)]`, like
the other M4+ fact vectors). Both parsers populate it:

- `parsers/python.rs` — extends the existing `[tool.prograph]` deserialization (aliases /
  exclude already live there); line numbers found by scanning the manifest text for the
  declared string (same technique the deps detector uses for evidence lines).
- `parsers/rust.rs` — reads `[package.metadata.prograph]`.

Malformed sections (non-list `reads`, non-string items) → `ParseWarning` on the project
(consistent with existing manifest-parse tolerance; NOT a hard error — unlike
`tracked.toml`, a broken declaration only loses edges, it cannot pollute the graph).

## Detector — `detectors/declared.rs`

One pipeline owns resolution, edge production, stale checks and warnings (stale detection
does NOT go through `drift::detect_all` — it needs the monorepo root for filesystem checks
and the resolution result, which the per-project intent API deliberately doesn't have):

```rust
pub struct DeclaredDetection {
    pub edges: Vec<EdgeCandidate>,          // kind = Declared
    pub stale: Vec<DriftFinding>,           // kind = StaleDeclaration
    pub warnings: Vec<String>,              // unresolved / rejected declarations
}

pub fn detect_declared(
    facts: &[ProjectFacts],
    monorepo_root: &Path,
) -> DeclaredDetection
```

Per declaration:
1. Normalize + validate (see above) — invalid → warning.
2. Resolve target: longest segment-aware `root_path` prefix among `facts` — no match →
   warning ("declared path 'ghost/x.db' matches no tracked project"); self-match → warning.
3. Emit `EdgeCandidate`:
   - `kind: EdgeKind::Declared`, declarer → target project (project→project only);
   - `attrs_json: {"mode": "read"|"write", "path": "<normalized>"}`;
   - `attrs_hash` over mode+path (each declaration = its own edge; two reads into proctor
     = two dispatcher→proctor edges);
   - `evidence`: one `EvidenceLocation` from the `DeclaredPath` fact (source_path, line,
     snippet) — no manifest re-scan.
4. Stale check (only for resolved declarations): `monorepo_root.join(path)` must exist
   (file OR directory — one literal `Path::exists` check; the trailing-`/` form was
   normalized away). Missing → `DriftFinding { kind: StaleDeclaration,
   entity_kind: DeclaredPath, entity_name: <path>, source_path, source_line,
   confidence: High }` attributed to the DECLARING project.

### Warning plumbing

`DetectionResult` (in `detectors/mod.rs`) gains `warnings: Vec<String>` — the declared
detector's warnings flow through the return value, and the indexer adds them to
`warning_count`. (Deliberately NOT the thread-local drain pattern `deps.rs` uses for
collision warnings — that pattern is a known wart; new code returns data. Migrating
`deps.rs` off the drain is out of scope.)

`indexer::index_monorepo` calls `detect_declared(&facts, monorepo_root)` alongside
`detect_all`, merges edges into the persist phase, stale findings into the drift persist
phase, warnings into `n_warnings`.

## Data model changes

- `EdgeKind::Declared` (`"declared"`) — 4th enum value; PyO3 + hand-updated `_core.pyi`.
- `DriftKind::StaleDeclaration` (`"stale_declaration"`); drift `entity_kind` gains
  `"declared_path"`.
- **Migration v10** (SQLite cannot ALTER a CHECK — table rebuild, precedent v6):
  - `edges`: CHECK extended with `'declared'`;
  - `drift_findings`: `kind` CHECK + `'stale_declaration'`, `entity_kind` CHECK +
    `'declared_path'`;
  - copy data, recreate indexes, additive chain v1..v10, schema_version = 10.

## Surfaces

- **Browser UI (`graph.js`):** declared edges render dashed with a distinct dash pattern
  and violet color `#8a6fc8` (contract_link stays orange dashed; `removed` diff status
  stays dotted). Cytoscape needs an explicit `'line-dash-pattern'` style field (e.g.
  `[2, 4]`) in addition to `KIND_LINESTYLES` — `line-style: 'dashed'` alone shares the
  contract look. Edge side panel needs no changes (attrs render generically → mode/path
  visible).
- **MCP (`mcp_server.py`):** the `find_edges` tool schema hardcodes the kind enum
  (`["package_dep", "mcp_call", "contract_link"]`, line ~244) — extend enum AND the tool
  description text. `find_drifts` kind enum likewise gains `stale_declaration` if it
  enumerates kinds.
- **REST/UI drift panel + `prograph drift`:** flow through generically once the enum
  exists; `--kind stale_declaration` CLI filter value added to help text.
- **MD export:** declared edges appear in the existing Outbound/Inbound edge sections;
  drift section renders the new kind via its generic path. Golden fixtures updated.

## Rollout in the monorepo (post-release, outside this repo)

dispatcher declares `reads` for its five watched projects; prograph declares
`writes = ["prograph-vault/derived/"]`. The `dispatcher ↔ *` and `dispatcher ↔ proctor`
entries then come OUT of `devtools/graph-registry-allowlist.toml` — the graph-vs-registry
checker sees the pairs through real edges.

## Testing

- **Rust parsers:** pyproject + Cargo declarations parsed into `DeclaredPath` with correct
  line numbers; malformed sections → ParseWarning, not error.
- **Rust detector:** segment-aware prefix (`proctor` vs `proctor2`); longest-match nested
  member; unresolved / absolute / `..` / self-reference → warning + no edge; two
  declarations → two edges with distinct attrs_hash; evidence carries manifest line;
  stale: existing file → no finding, deleted file → StaleDeclaration, directory target
  with & without trailing slash → no finding.
- **Rust migration:** v9 database with existing edges/drifts migrates to v10 losslessly;
  new kinds insertable after.
- **Python e2e (pytest):** fixture monorepo with declared reads/writes → `declared` edge
  in `/api/graph` with mode/path attrs; edge evidence via `edge_evidence`; deleted target
  → finding in `/api/drifts?kind=stale_declaration` and `prograph drift --kind
  stale_declaration`; MCP `find_edges` accepts `kind="declared"`.
- **Golden:** regenerate after renderer output settles.

## Out of scope (YAGNI)

Globs in paths (prefix/directory only), per-declaration notes/annotations, auto-deriving
declarations from runtime tracing, inbound declarations ("who writes into me"), hard-error
mode for broken declarations, migrating `deps.rs` off the thread-local warning drain.
