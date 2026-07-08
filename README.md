# prograph

Cross-project structure mapper for monorepos. Detects how independent projects in a workspace talk to each other (package deps, shared contracts, MCP calls) and exposes the graph to humans (browser UI) and AI agents (MCP).

**Status:** M11 — Spec/TODO drift detection (v1.3). Every index run extracts declared intent from each project's `README.md` + `TODO.md` + `docs/superpowers/specs/*.md` (recognising headings `## Public surface`, `## MCP tools exposed`, `## Contracts declared`, `## TODO`) and compares against detected reality. Three drift kinds are persisted in `drift_findings`: **missing** (declared but not implemented), **extra** (implemented but not declared — fires only when the project has SOME intent docs), and **stale_todo** (open TODO whose tokens overlap a recent change_log label). Exposed via MCP tool `find_drifts`, CLI `prograph drift`, REST endpoint `GET /api/drifts`, MD project-card section "## Drift findings", browser UI side panel. Closes the original 2026-05-25 brainstorm requirement "Spec/TODO-driven target state".

See `docs/superpowers/specs/2026-05-25-prograph-design.md` for the full design and `docs/superpowers/plans/` for milestone plans.

## Install (development)

Requires Rust 1.75+ and Python 3.11+.

```sh
uv sync   # installs Python deps AND builds the Rust extension via maturin
```

## Usage

```sh
cd <your-monorepo-root>
prograph init                 # creates .prograph/config.toml + .gitignore
prograph index [--export-md]  # discovers projects, parses manifests, detects edges,
                              #   persists snapshot, writes change_log entries
prograph status [--json]      # shows discovered projects + latest snapshot summary
prograph index --json         # IndexSummary JSON (snapshot_id, n_projects, n_edges, n_changes, ...)
prograph export-md            # re-render MD from latest snapshot (no reindex)
prograph drift [--kind missing|extra|stale_todo] [--json]   # show drift findings (M11)
prograph mcp                  # MCP stdio server for AI clients
prograph serve [--host 127.0.0.1] [--port 7700]   # browser UI + REST
prograph --version            # print prograph + core versions
```

After M5, `.prograph/` contains the full graph database (`graph.db` — snapshots, projects, edges across three kinds, contracts, change_log, captured `git_commit`s) plus, when `--export-md` / `auto_export` is on, the per-project / per-contract / index Markdown tree.

### Markdown export

After `prograph index --export-md` (or with `[output] auto_export = true`), `.prograph/` contains:

- `index.md` — monorepo overview: project list, contract list, recent activity
- `projects/<slug>.md` — one per project, with manifest / public surface / outbound + inbound edges / recent changes
- `contracts/<slug>.md` — one per shared contract, with owner list and provenance

Files are Obsidian-friendly: open `.prograph/` as a vault and follow `[[wiki-links]]` between projects and contracts.

To re-render without re-indexing (after changing a renderer template):

```sh
prograph export-md
```

To regenerate the test golden files after intentional output changes:

```sh
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_full
```

### Detected edge kinds (M4)

| Kind | Source | Identity |
|---|---|---|
| `package_dep` | Manifest deps (`[project].dependencies`, `[dependencies]`, `dependencies`...) | `(from, to, dep_name)` — version_req in attrs |
| `mcp_call` | `@server.tool()` decorator / `.tool("name", ...)` registration server-side; `.call_tool("name", ...)` / `.invoke_tool(...)` client-side. Python + Rust source scanned via tree-sitter. | `(from, to, tool)` |
| `contract_link` | JSON Schema / OpenAPI / `.proto` files with matching `$id` (or identical content hash) across ≥2 projects. | `(from_project, to_contract)` |

Edges are aggregated regardless of source: a single `prograph index` produces all three kinds in one snapshot.

### Module-level facts (M9)

For each project, prograph scans source files (`.py`, `.rs`, `.js`/`.ts`/`.mjs`/etc.) and extracts:

- **Modules** — one row per source file with rel_path + language.
- **Public symbols** — `def`/`class` at module top level without leading underscore (Python); items with `pub` visibility (Rust); `export` declarations (JS).
- **Internal imports** — imports targeting the same project (Python `import myproj.x`; Rust `use crate::x`; JS `import x from './y'`).

These appear in MD project cards, in MCP `describe_project`, and in the browser UI side panel.

Module identity is `(project_id, rel_path)`; symbol identity is `(module_id, name)`. Renaming a file or symbol surfaces as remove + add events in the change_log.

#### Limitations

- Type signatures and docstrings are not extracted.
- The Python heuristic uses simple prefix matching for internal imports; `__all__` whitelists are not consulted.

### Cross-project symbol references (M10)

For each external import in a project's source file (e.g. `from atp_platform.sdk import Client` in Maestro), prograph resolves the target to a publisher project + module + symbol when both sides are in the same monorepo.

- **Inbound**: "who imports my X?" — `find_symbol_references project_name=X` returns from/to module + line.
- **Outbound**: "what do I import?" — `find_symbol_references project_name=X direction=outbound`.

Language coverage: Python (dotted paths, dash↔underscore norm, alias-aware) and Rust (`use crate_name::a::b::Symbol`). JS deferred (no driver in scope).

Resolution is conservative: stdlib + PyPI + crates.io imports are dropped silently. `pub use` re-exports in Rust land at the directly-imported crate, not the re-export source (best-effort; full chain following is a future enrichment).

### Drift detection (M11)

For each project, prograph reads markdown intent docs and extracts:

- Items under `## Public surface` / `## Public API` / `## Exports` → declared public symbols
- Items under `## MCP tools exposed` / `## MCP tools` → declared MCP tools
- Items under `## Contracts declared` / `## Contracts` → declared contracts
- Checkboxes (`- [ ]`) in `TODO.md` or under `## TODO` → open TODOs

These are compared against M4's `mcp_decls` / `contracts` and M9's `public_symbols`:

- **Missing**: in intent, not in reality. Confidence=high.
- **Extra**: in reality, not in intent (only flagged when SOME intent exists for that kind).
- **Stale TODO**: open `[ ]` item whose 2+ significant tokens (or strong ticket-ID like LABS-87) overlap with a recent change_log label. Confidence=low.

Auto-generated MD files (those prograph itself wrote) are skipped via the `<!-- prograph:generated -->` marker on the first line.

Query: `prograph drift --kind missing` for CLI; `find_drifts` MCP tool for AI agents.

### Working with workspace sub-packages

If a project publishes multiple package names (common for workspace orchestrators), declare them in `[tool.prograph].aliases`:

```toml
# atp-platform/pyproject.toml
[project]
name = "atp-platform"

[tool.prograph]
aliases = ["atp-platform-sdk", "atp-platform-cli"]
```

Now any consumer declaring `atp-platform-sdk>=2.0` in its `dependencies` resolves to this project. Without aliases, M3 only matches the project's `[project].name`.

## AI agent integration (MCP)

Configure Claude Code or another MCP client to spawn `prograph mcp` for your monorepo:

```json
{
  "mcpServers": {
    "prograph": {
      "command": "uv",
      "args": ["run", "prograph", "mcp", "--monorepo", "/path/to/monorepo"]
    }
  }
}
```

The 10 tools are:

| Tool | Purpose |
|---|---|
| `monorepo_overview` | Hello-world: list of projects + contracts + recent changes. |
| `list_projects` | Filter projects by kind. |
| `describe_project` | Full project card by name (includes modules, public symbols, inbound/outbound refs, drifts). |
| `find_edges` | Query edges with from/to/kind/since filters. |
| `edge_evidence` | File:line locations of MCP call sites for a given edge. |
| `changelog` | Paginated history of changes. |
| `search` | FTS over project + contract names. |
| `snapshot_info` | Snapshot metadata (latest or by id). |
| `find_symbol_references` (M10) | Cross-project symbol citations — inbound ("who imports my X?") or outbound. |
| `find_drifts` (M11) | Spec/TODO drift findings — filter by project_name and/or kind (missing/extra/stale_todo). |

### Extending MCP detection

Drop a `python.scm` or `rust.scm` into `.prograph/mcp_patterns/` to extend the bundled tree-sitter queries:

```scheme
; arbiter-style: tools registered via .mcp_tool("name", ...)
(call_expression
  function: (field_expression field: (field_identifier) @method)
  arguments: (arguments . (string_literal) @tool_name_literal)
  (#eq? @method "mcp_tool")) @tool_decl_arbiter
```

Required capture names: `tool_name` (identifier capture for decorator-style decls), `tool_name_literal` (string-literal capture for call-style decls and uses), `tool_use_call` (Python) / `tool_use_method` (Rust) marker captures to distinguish use sites from decl sites.

## Browser UI

```sh
prograph serve [--port 7700] [--host 127.0.0.1]
```

Opens a local web UI:
- **Graph view** (cytoscape.js): projects as rectangles colored by language, contracts as diamonds. Edges colored by kind (gray=package_dep, teal=mcp_call, amber=contract_link).
- **Side panel**: click a node or edge to see full details — manifest, MCP tools, contract owners, evidence, recent changes.
- **Search box**: FTS query against project + contract names.
- **Activity feed**: last 10 change_log entries.

Static assets (cytoscape.js, cose-bilkent, Pico CSS) load from a CDN. Internet required at page load. Offline bundling is a later milestone.

### REST endpoints

| Endpoint | Purpose |
|---|---|
| `GET /api/health` | Liveness probe. |
| `GET /api/graph[?since=<snap>]` | Full graph (nodes + edges) for the latest snapshot; `since` adds diff status tags (added/removed/unchanged). |
| `GET /api/projects/by-name/{name}` | Project description by name. |
| `GET /api/projects/by-id/{id}` | Project description by id. |
| `GET /api/contracts/by-id/{id}` | Contract description by id. |
| `GET /api/contracts/by-slug/{slug}` | Contract description by slug. |
| `GET /api/edges/{edge_id}` | Edge + evidence + history. |
| `GET /api/changelog?since=&entity_kind=&limit=` | Paginated changelog. |
| `GET /api/search?q=&kinds=&limit=` | FTS search. |
| `GET /api/snapshots[?limit=]` | List of snapshots. |
| `GET /api/snapshots/{id}` | Snapshot metadata. |
| `GET /api/symbol_refs?project=&symbol=&direction=` (M10) | Cross-project symbol citations. |
| `GET /api/drifts?project=&kind=` (M11) | Spec/TODO drift findings. |

No auth — bind to 127.0.0.1 (default). `--host 0.0.0.0` prints a warning.

### Deferred to M12+ (post-1.3)

- **Type signatures + docstrings** on `PublicSymbol` — tree-sitter already sees them; surfacing them in MCP / MD is a small enrichment task.
- **JS cross-project symbol resolution** — package.json `exports` maze; M10 covers Python + Rust only.
- **`pub use` re-export chain following** in Rust — M10 lands at the directly-imported crate.
- **Auto-fix proposals for drift** — M11 reports, doesn't suggest patches.
- **Renamed-symbol pairing** — M11 emits missing/extra separately; no "looks like a rename" heuristic.
- **Drift trend visualisation** — temporal drift counts over time. Storage supports it.
- **Cross-project drift** — "Maestro spec says it uses arbiter::Decider but symbol_refs doesn't show it".
- **External tracker matching** — TODO ↔ Linear / GitHub issues.
- **HTTP / REST runtime edges** — heuristic detection of FastAPI/Flask/axum routes and matching client calls.
- **JS MCP source scanning** — no JS MCP servers in scope.
- **WebSocket live updates** (`/ws/changes`) — page reload remains the upgrade path.
- **Offline asset bundling** — CDN works fine for the local-dev tool.
- **Playwright / Selenium E2E** — REST + static-structure tests cover the regression surface.
- **Authentication / TLS** — bind-to-127.0.0.1 remains the security boundary.
- **Mobile / responsive design** — desktop-only.

## Development

- Rust core: `prograph-core/` (built via maturin into `prograph._core`).
- Python wrapper: `prograph/` (CLI, models, paths).
- Tests: `cargo test --all-targets` for Rust, `uv run pytest -v` for Python.
- Lint: `cargo clippy --all-targets -- -D warnings`, `uv run ruff check .`, `uv run pyrefly check 'prograph/**/*.py' 'tests/**/*.py'`.

### Opt-in smoke against the parent monorepo

If `all_ai_orchestrators/` is the parent directory (the user's real monorepo):

```sh
uv run pytest -m realmonorepo -v
```

### Performance baselines

```sh
uv run pytest -m bench -v                                  # run
uv run pytest -m bench --benchmark-compare                 # compare with previous run
uv run pytest -m bench --benchmark-compare-fail=mean:200%  # fail if 2× slower
```

Baselines are saved under `.benchmarks/`. Commit the
`Darwin-CPython-*/*.json` (or platform equivalent) file when you intentionally
accept a perf change — CI compares against the committed baseline.

## License

MIT.
