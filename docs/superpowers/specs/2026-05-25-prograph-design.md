# prograph — Design Spec

**Status:** Draft for review
**Date:** 2026-05-25
**Author:** Andrei Shtanakov + Claude (brainstorming session)
**Replaces:** vendored `Sourcetrail/` (archived 2021 upstream, kept as historical reference only)

---

## 1. Context & Motivation

`prograph` is a new tool that maps a **monorepo** of independent projects and shows how they interact at the **API / contract / data-flow level**. It is a from-scratch design inspired by Sourcetrail's general idea (visual code navigator) but with a different center of gravity: **cross-project structure first, single-project symbol indexing second**.

The motivating environment is `all_ai_orchestrators/` — a workspace with ~10 independent projects (`Maestro`, `arbiter`, `atp-platform`, `spec-runner`, `proctor-a`, `open-prose`, `agents-for-game`, plus testing/docs siblings) that interact via package deps, vendored files, MCP tool calls, and shared contracts (JSON Schemas). Holding the inter-project map in one's head no longer scales; existing tools (LSP, ctags, Sourcetrail) operate one project at a time.

`prograph` is built for two primary consumers:

1. **The human author** — wants to see "what talks to what" across the monorepo, drill down, and watch how that map drifts over time as projects evolve.
2. **AI coding agents** (Claude Code, custom skills) — want a structured, queryable map of the monorepo to ground their reasoning before editing code.

---

## 2. Goals & Non-Goals

### Goals (MVP — "Living artifact")

1. Discover projects in a monorepo and classify them by language (Python / Rust / JS / docs-only / mixed).
2. Detect cross-project edges of three kinds:
   - **Package dependencies** (one project declares another in `pyproject.toml` / `Cargo.toml` / `package.json`).
   - **Shared contracts** (JSON Schema / OpenAPI / `.proto` files referenced by ≥2 projects).
   - **MCP calls** (one project exposes an MCP tool, another invokes it).
3. Persist the result as a **temporal graph** in SQLite — each indexing run is a snapshot, entities have `first_seen` / `last_seen` ranges, change-log is append-only.
4. Export the same content as **idempotent per-project Markdown** files compatible with Obsidian (`.prograph/projects/*.md`).
5. Serve a **local browser UI** (FastAPI + static, graph viz with d3-force or cytoscape.js) for interactive exploration.
6. Expose the graph to AI agents via a **stdio MCP server** with a small, stable set of tools.

### Non-Goals (deferred to future phases)

- TUI front-end (Phase 2).
- Spec / target-state drift analysis (Phase 3 — depends on MVP being a reliable baseline first).
- Plugin SDK for additional languages (Phase 4 — premature abstraction before MVP).
- HTTP / REST runtime-call edges (Phase 5 — much harder than MCP, no formal registration).
- Vendored-file detection (Phase 6 — heuristics, false-positive risk).
- Incremental reindex (Phase 7 — current full reindex fits 30s budget).
- Multi-monorepo federation (Phase 8 — single use case in scope).

### Anti-Features (will never do)

- **Not an LSP / rust-analyzer replacement** — no symbol-level intra-file navigation.
- **Not a refactoring tool** — read-only with respect to source code.
- **Not a generic knowledge graph** — strictly code structure + (later) declared intent.
- **Not a SaaS / cloud tool** — local-only, sync via git.
- **Not a linter / quality tool** — describes structure, does not judge it.

---

## 3. High-Level Architecture

Two-layer build, mirroring the `arbiter` crate's PyO3 pattern in the same monorepo:

```
┌─────────────────────────────────────────────────────────────────────┐
│  Monorepo root (e.g. all_ai_orchestrators/)                         │
│   Maestro/   arbiter/   atp-platform/   spec-runner/   ...          │
└──────────────────────────┬──────────────────────────────────────────┘
                           │  filesystem scan
                           ▼
┌─────────────────────────────────────────────────────────────────────┐
│  prograph-core  (Rust crate, PyO3 module)                           │
│                                                                     │
│   Discovery ──► Parsers ──► Edge detectors ──► Graph store + diff   │
│   (manifests)  (tree-sitter (deps, contracts, (rusqlite + change-   │
│                + Python ast) mcp)             log engine)           │
└────────────────────────────────────┬────────────────────────────────┘
                                     │  PyO3 bindings
                                     ▼
┌─────────────────────────────────────────────────────────────────────┐
│  prograph  (Python package — thin orchestration / I/O)              │
│                                                                     │
│   typer CLI   FastAPI web   MCP stdio server   MD exporter          │
└─────────────────────────────────────────────────────────────────────┘

Artifacts produced under monorepo root in   .prograph/   :
   ├─ graph.db          (SQLite — primary store, change-log)
   ├─ projects/*.md     (per-project export, Obsidian-friendly)
   ├─ contracts/*.md    (per-contract export)
   ├─ index.md          (monorepo-level overview)
   ├─ config.toml       (user-editable: project list, ignores, aliases)
   ├─ index.log         (warnings / debug from last index run)
   └─ .gitignore        (auto-generated: ignores graph.db, index.log,
                         keeps projects/*.md, contracts/*.md, index.md,
                         config.toml under version control)
```

### Layer responsibilities

- **Rust core** — anything that touches files or parses ASTs: discovery, language parsers, edge detectors, SQLite read/write, snapshot diff. Returns typed dataclass-like objects to Python through PyO3.
- **Python layer** — no parsing, only I/O: CLI dispatch, MD rendering, FastAPI endpoints (graph JSON for frontend), MCP tools, summary printing.

### Why this boundary

Parsing and graph storage are the hot path and the slow part — keeping them in Rust is a perf and correctness win. CLI, UI, MCP server are interfaces and I/O; iterating in Python is faster.

### Entry points

- `prograph init` — creates `.prograph/` skeleton.
- `prograph index` — full reindex.
- `prograph serve` — local browser UI at `http://127.0.0.1:7700`.
- `prograph mcp` — MCP stdio server for Claude Code / other AI clients.

---

## 4. Components

Each unit has **one role** and a narrow public interface. The boundary between parsers (extract facts) and detectors (combine facts into edges) is deliberate — it shapes future plugin extension.

### 4.1 Rust core (`prograph-core` crate)

#### `discovery`
Walks the first-level subdirectories of the monorepo root and classifies each by signal files:

| Signal | Kind |
|---|---|
| `Cargo.toml` | `rust` |
| `pyproject.toml` or `setup.py` | `python` |
| `package.json` | `js` |
| only `README.md` / `CLAUDE.md` / `TODO.md` | `docs` |
| multiple of the above | `mixed` |

Output: `Vec<ProjectCandidate { name, root, kind, manifests }>`. Config (`.prograph/config.toml`) overrides via include/exclude lists and explicit aliases (when package name differs from directory name).

#### `parsers`
Per-language adapters behind a common trait:

```rust
pub trait LanguageParser: Send + Sync {
    fn kind(&self) -> ProjectKind;
    fn parse(&self, project: &ProjectCandidate) -> Result<ProjectFacts, ParserError>;
}
```

Implementations:
- `python` — Python `ast` via a PyO3 callback (higher fidelity than tree-sitter for imports).
- `rust` — `tree-sitter-rust` + direct `Cargo.toml` parse via `toml` crate.
- `js` — `tree-sitter-{javascript,typescript}` + `package.json` parse.

Output (`ProjectFacts`) is **a stable, flat set of facts** — not an AST:

```rust
pub struct ProjectFacts {
    pub manifest: Manifest,             // declared package name, version, deps
    pub modules: Vec<Module>,           // path, language, public symbols, imports
    pub mcp_decls: Vec<McpToolDecl>,    // @mcp.tool decorators / register_tool calls
    pub mcp_uses: Vec<McpClientUse>,    // MCPClient(...).call("tool", ...)
    pub contracts: Vec<ContractFile>,   // JSON Schema / OpenAPI / .proto
    pub parse_errors: Vec<ParseError>,  // soft failures, project marked partial
}
```

Parsers know nothing about the graph. This isolates them and makes Phase 4 plugin work mechanical.

#### `edge_detectors`
Three independent modules, each reads the full `Vec<ProjectFacts>` and emits edges. Each can fail independently; the snapshot still completes, the affected edge kind is marked `detector_status=failed`.

- **`deps_detector`** — for each `declared_dep`, find a project whose `Manifest.declared_name == dep.name`. Edge: `{ kind: package_dep, from: consumer, to: publisher, attrs: { dep_name, version_req } }`. **Identity excludes `version_req`** — version bumps are `attrs_changed` events, not remove+add.
- **`contracts_detector`** — group `ContractFile` by `declared_id` (or content hash if no id). For each group spanning ≥1 projects, create a `contract` node and `contract_link` edges. ≥2 projects = a real cross-project link.
- **`mcp_detector`** — match `McpClientUse.tool_name_invoked` against `McpToolDecl.tool_name`. Edge: `{ kind: mcp_call, from: client, to: server, attrs: { tool, transport } }`. Identity includes `tool`.

```rust
pub trait EdgeDetector: Send + Sync {
    fn name(&self) -> &str;
    fn detect(&self, facts: &[ProjectFacts]) -> Vec<EdgeCandidate>;
}
```

#### `graph_store`
SQLite-backed (rusqlite, WAL mode). API: `Store::open(path)`, `Store::write_snapshot(...)`, `Store::query(...)`. See Section 5 for schema.

#### `diff_engine`
Given `prev_snapshot_id` and a new fact set, computes `Vec<ChangeEvent { kind: Added | Removed | AttrsChanged, ... }>` via deterministic identity keys.

### 4.2 Python layer (`prograph` package)

- **`prograph.cli`** — `typer` dispatch. See Section 6.
- **`prograph.web`** — FastAPI app, static SPA. See Section 6.
- **`prograph.mcp`** — Python MCP SDK stdio server. See Section 6.
- **`prograph.export`** — pure renderer: snapshot → `.prograph/projects/*.md` + `contracts/*.md` + `index.md`. Idempotent.
- **`prograph.models`** — pydantic v2 models — single source of truth for data shapes flowing between Rust → Python → JSON consumers.

### 4.3 Rust ↔ Python boundary

PyO3 exposes only data classes (`#[pyclass]` with read-only fields): `Project`, `Edge`, `Contract`, `ChangeEvent`, `SnapshotInfo`, `QueryResult`. Python does not call parsers directly or touch SQLite; everything flows through `Store` methods. This isolates the layers for testing (cargo test for Rust, pytest with a mock `Store` for Python) and allows swapping the storage backend in Rust without Python changes.

---

## 5. Data Model

### 5.1 SQLite schema — temporal model

Key idea: **no full snapshot duplication**. Entities live across time with `first_seen` / `last_seen` references into a `snapshots` table. The current state is `WHERE last_seen = (SELECT MAX(id) FROM snapshots)`. Any historical state restores symmetrically: `WHERE first_seen <= S AND last_seen >= S`.

```sql
-- 1. snapshots: one row per `prograph index` invocation
CREATE TABLE snapshots (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  ts              TEXT NOT NULL,           -- ISO8601
  monorepo_root   TEXT NOT NULL,
  git_commit      TEXT,                    -- only if clean working tree
  prograph_version TEXT NOT NULL
);

-- 2. projects (nodes)
CREATE TABLE projects (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL,
  root_path   TEXT NOT NULL UNIQUE,        -- relative to monorepo_root; identity
  kind        TEXT NOT NULL,               -- python|rust|js|docs|mixed
  attrs_json  TEXT NOT NULL,               -- declared package name, version, summary
  first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
  last_seen   INTEGER NOT NULL REFERENCES snapshots(id)
);

-- 3. contracts — first-class nodes (participate in hyperedge-style relations)
CREATE TABLE contracts (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  declared_id   TEXT,                      -- $id from JSON Schema / info.title from OpenAPI
  content_hash  TEXT NOT NULL,             -- SHA256 of canonicalized content
  kind          TEXT NOT NULL,             -- json_schema|openapi|proto
  first_seen    INTEGER NOT NULL REFERENCES snapshots(id),
  last_seen     INTEGER NOT NULL REFERENCES snapshots(id),
  UNIQUE(declared_id, content_hash)
);

CREATE TABLE contract_files (
  contract_id INTEGER NOT NULL REFERENCES contracts(id),
  project_id  INTEGER NOT NULL REFERENCES projects(id),
  rel_path    TEXT NOT NULL,
  first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
  last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
  PRIMARY KEY(contract_id, project_id, rel_path)
);

-- 4. edges. identity = (kind, from, to, attrs_hash)
CREATE TABLE edges (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  kind        TEXT NOT NULL,               -- package_dep|mcp_call|contract_link
  from_kind   TEXT NOT NULL,               -- 'project' | 'contract'
  from_id     INTEGER NOT NULL,
  to_kind     TEXT NOT NULL,
  to_id       INTEGER NOT NULL,
  attrs_json  TEXT NOT NULL,               -- full attrs payload
  attrs_hash  TEXT NOT NULL,               -- hash of identity-bearing attrs only
  first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
  last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
  UNIQUE(kind, from_kind, from_id, to_kind, to_id, attrs_hash)
);

-- evidence: file:line where a detector found this edge
CREATE TABLE edge_evidence (
  edge_id     INTEGER NOT NULL REFERENCES edges(id),
  project_id  INTEGER NOT NULL REFERENCES projects(id),
  rel_path    TEXT NOT NULL,
  line        INTEGER,
  snippet     TEXT,
  first_seen  INTEGER NOT NULL REFERENCES snapshots(id),
  last_seen   INTEGER NOT NULL REFERENCES snapshots(id),
  PRIMARY KEY(edge_id, project_id, rel_path, line)
);

-- 5. change_log — append-only
CREATE TABLE change_log (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  snapshot_id  INTEGER NOT NULL REFERENCES snapshots(id),
  ts           TEXT NOT NULL,
  entity_kind  TEXT NOT NULL,              -- project|edge|contract|contract_file|edge_evidence
  entity_id    INTEGER NOT NULL,
  change       TEXT NOT NULL,              -- added|removed|attrs_changed
  before_json  TEXT,
  after_json   TEXT
);
CREATE INDEX idx_change_log_snapshot ON change_log(snapshot_id);
CREATE INDEX idx_change_log_entity   ON change_log(entity_kind, entity_id);

-- 6. FTS — for AI search and UI search-box
CREATE VIRTUAL TABLE search_fts USING fts5(
  entity_kind,
  entity_id  UNINDEXED,
  project    UNINDEXED,
  title,
  body,
  tokenize = 'porter unicode61'
);
```

### 5.2 Identity rules

| Entity | Identity columns |
|---|---|
| `project` | `root_path` (relative). Rename = remove + add. |
| `contract` | `(declared_id, content_hash)`. Content change without id change = remove + add. Id change with same content = remove + add. |
| `edge[package_dep]` | `(from, to, dep_name)`. Version bump = `attrs_changed`. |
| `edge[mcp_call]` | `(from, to, tool)`. Adding another invocation site = new `edge_evidence` row, not new edge. |
| `edge[contract_link]` | `(from_project, to_contract)`. |

### 5.3 MD export structure

Located in `.prograph/projects/<slug>.md`, `.prograph/contracts/<slug>.md`, `.prograph/index.md`. YAML frontmatter + Obsidian wiki-links (`[[name]]`). Sorted deterministically (by identity) for byte-stable output.

Per-project example:

```markdown
---
prograph: project
name: Maestro
kind: python
root: ./Maestro
snapshot: 47
indexed_at: 2026-05-25T14:23:10Z
---

# Maestro

> DAG orchestrator (one-line from CLAUDE.md/README.md if available)

## Manifest
- `pyproject.toml`: package `maestro`, version 0.2.0
- requires: `atp-platform-sdk>=2.0.0`, `arbiter>=0.1`, ...

## Public surface
### MCP tools exposed
- `report_decision` — `maestro/api.py:42`

### Contracts declared
- [[contract:obs-v1]] (json_schema) — `_cowork_output/observability-contract/v1.json`

## Outbound edges
- → [[atp-platform]] · `package_dep` · `atp-platform-sdk>=2.0.0`
- → [[arbiter]] · `mcp_call` · tool `report_decision` · evidence `maestro/clients/arbiter.py:18`
- ↔ [[contract:obs-v1]] · `contract_link`

## Inbound edges
- ← [[spec-runner]] · `package_dep` · `maestro>=0.1`

## Recent changes (last 5)
- snapshot 47 (2026-05-24): edge → arbiter (`mcp_call`) added
- snapshot 42 (2026-04-25): edge → atp-platform version_req `>=1.5` → `>=2.0.0`

## Sources
- README: `README.md`
- CLAUDE: `CLAUDE.md`
- TODO: `TODO.md`
```

### 5.4 What lives under git

`.prograph/` is committed to the monorepo. Default `.prograph/.gitignore`:

```
graph.db
graph.db-wal
graph.db-shm
index.log
```

`projects/*.md`, `contracts/*.md`, `index.md`, `config.toml` are tracked — so structural changes show up in PR diffs.

---

## 6. Indexing Flow (`prograph index`)

Six phases. Atomicity at the SQLite transaction level, FS lock for concurrent-run prevention, explicit behavior on partial failures.

```
Phase 0 — Bootstrap                                            (sync, ms)
  ▸ resolve monorepo root (--monorepo / git toplevel / CWD)
  ▸ open .prograph/graph.db, apply migrations
  ▸ read .prograph/config.toml if present
  ▸ acquire exclusive FS lock .prograph/index.lock
                                    │
Phase 1 — Discovery                                            (sync, ms)
  ▸ list first-level subdirs, apply config filters
  ▸ classify each by signal files
  ▸ emit Vec<ProjectCandidate>
                                    │
Phase 2 — Parsing                                       (parallel, seconds)
  ▸ rayon pool, one worker per project
  ▸ each worker → ProjectFacts (with parse_errors on soft failures)
  ▸ Ctrl-C: wait for active workers, exit without snapshot
                                    │
Phase 3 — Edge detection                                       (sync, ms)
  ▸ deps_detector, contracts_detector, mcp_detector — all see full facts
  ▸ each can fail independently → detector_status flag on snapshot
                                    │
Phase 4 — Diff vs previous snapshot                            (sync, ms)
  ▸ load alive set (last_seen = MAX(snapshots.id))
  ▸ compute Added / Removed / AttrsChanged via identity keys
  ▸ emit Vec<ChangeEvent>
                                    │
Phase 5 — Persist                                  (single TX, ms)
  ▸ BEGIN IMMEDIATE
  ▸ INSERT new snapshot row
  ▸ for each entity: INSERT (added) / UPDATE last_seen (unchanged)
                     / UPDATE attrs_json + last_seen (attrs_changed)
                     / no-op (removed)
  ▸ INSERT change_log batch
  ▸ rebuild search_fts rows for touched entities
  ▸ COMMIT
                                    │
Phase 6 — Optional outputs                                    (parallel, ms)
  ▸ if --export-md or config.auto_export: render projects/*.md,
    contracts/*.md, index.md
  ▸ release lock
  ▸ print summary (project count, edge count, change count, warnings)
```

### Failure semantics

- **Parse error on one project**: snapshot still created, project marked `parse_status=partial`, warning logged to `index.log`, exit 0.
- **Detector failure**: snapshot still created, edge kind marked `detector_status=failed`. AI consumers see explicitly which kinds are unreliable.
- **SQLite write failure**: ROLLBACK, no snapshot, lock released, exit non-zero.
- **Lock contention**: exit 1 with `"another prograph index is running"`.
- **SIGKILL during Phase 5**: ROLLBACK; database remains on previous snapshot.
- **SIGKILL during earlier phases**: lock file persists (intentional — surfaces the problem to the user; documented manual cleanup: `rm .prograph/index.lock`).
- **Empty monorepo (no manifests)**: exit 1 with helpful message before any snapshot is created.

### Determinism

Identity hashes use sorted keys (`BTreeMap` at serialization boundaries). Parser output is sorted before hashing. MD export sorts by identity. Same monorepo state + same `prograph_version` → byte-identical snapshot content (modulo `ts` and `git_commit`).

### Performance budget

Target p95 full reindex on a ~10-project monorepo: **< 30 seconds cold cache**. Parsing dominates. Warm cache (incremental, future): < 2 s. CI baseline test asserts `monorepo_full` fixture indexes in < 5 s.

### Concurrency

- Single indexer per monorepo (lock file).
- `prograph serve` / `prograph mcp` open SQLite read-only and coexist with a running indexer thanks to WAL mode.

---

## 7. API Surfaces

### 7.1 CLI — `prograph` (typer)

| Command | Purpose |
|---|---|
| `prograph init` | Create `.prograph/config.toml` skeleton + `.prograph/.gitignore`. |
| `prograph index [--monorepo PATH] [--export-md] [--no-changelog]` | Full reindex. Exit 0 success, 1 user error, 2 internal. |
| `prograph export-md [--out DIR]` | Re-render MD from the current snapshot without reindexing. |
| `prograph serve [--port 7700] [--host 127.0.0.1]` | Local browser UI. |
| `prograph mcp` | MCP stdio server (for Claude Code / others). |
| `prograph query <subcmd> [...] [--json]` | Subcommands: `projects`, `project <name>`, `edges`, `changelog`, `search <q>`. Pretty TTY by default, `--json` for machines. |
| `prograph status` | Snapshot count, last index ts, version, monorepo root. |

Conventions: non-interactive, exit codes documented, `--json` produces strict JSON on stdout, all warnings go to `.prograph/index.log` never stdout.

### 7.2 MCP server — `prograph mcp`

Stdio transport. Tool set (frozen for v1):

| Tool | Args | Returns |
|---|---|---|
| `list_projects` | `kind?: python\|rust\|js\|docs\|mixed` | `[{ id, name, kind, root, summary, n_outbound, n_inbound }]` |
| `describe_project` | `name: string` | Full card: manifest, MCP tools exposed, contracts declared, outbound/inbound edges, last 5 changes. |
| `find_edges` | `from?, to?, kind?, since?` | `[{ id, kind, from, to, attrs, evidence: [{file, line, snippet}], first_seen, last_seen }]` |
| `edge_evidence` | `edge_id: int` | Detailed source lines for an edge — drill-down. |
| `changelog` | `since?, entity_kind?, limit?=50` | Chronological list of `ChangeEvent`s, with before/after for `attrs_changed`. |
| `search` | `q: string, kinds?: [project\|contract\|symbol\|tool]` | FTS results: `[{ entity_kind, entity_id, name, project, snippet, score }]` |
| `snapshot_info` | `id?` (default latest) | `{ id, ts, git_commit, monorepo_root, prograph_version, n_projects, n_edges, n_changes }` |
| `monorepo_overview` | — | High-level summary for a first-time agent: kinds breakdown, top inbound/outbound projects, contracts, last 10 events. One tool call, one screen. |

Properties: imperative tool names; all args optional where possible; structured JSON responses (never embedded markdown); every response includes `snapshot_id` for cache validity.

### 7.3 REST — for the browser UI

FastAPI app served by `prograph serve`. Bound to `127.0.0.1` by default; binding to `0.0.0.0` requires `--host` and emits a warning.

| Endpoint | Returns | Consumer |
|---|---|---|
| `GET /api/graph` | `{ nodes, edges, snapshot_id }` for main view. | Main page graph rendering. |
| `GET /api/graph?since=<snap>` | Same shape, with `node.status` / `edge.status` ∈ `{added, removed, attrs_changed, unchanged}` relative to `since`. | Diff view. |
| `GET /api/projects/{id}` | Project card (same content as MCP `describe_project`). | Side panel on node click. |
| `GET /api/edges/{id}` | Edge + evidence + history. | Side panel on edge click. |
| `GET /api/changelog?since=&kind=&limit=` | Timeline. | Activity tab. |
| `GET /api/search?q=` | FTS. | Search box. |
| `GET /api/snapshots` | Snapshot list with metadata. | "View at snapshot" selector. |
| `WS /ws/changes` *(post-MVP)* | Push `ChangeEvent` when a new index completes. | Live updates. |

### 7.4 Shared model layer

Single source of truth for shapes:

```
Rust pyclass structs (raw)
        │ PyO3
        ▼
prograph.models (pydantic v2)  ← used by CLI --json, MCP tool schemas (via
        │                       model_json_schema), FastAPI response models
        ▼
   API consumers
```

Changing a field in Rust ripples through to CLI / MCP / REST in one place. Adding fields is backwards-compatible by default (pydantic ignores unknown on parse; emits new fields on response — clients tolerant by convention).

### 7.5 Open API decisions

1. **`monorepo_overview` is first-class** — the "hello world" tool for an AI agent entering a new monorepo. One call, one screen.
2. **No auth on REST** — local-dev convention. Binding to non-loopback warns.
3. **`prograph query` CLI stays** as scripting fallback (cron, CI, shell pipes) alongside MCP. Some duplication is fine.

---

## 8. Testing Strategy

### 8.1 Test layers

```
E2E (small, expensive)
  pytest spawning prograph subprocess + httpx / MCP client
  - prograph index → assert SQLite content + MD files
  - prograph serve → curl /api/graph
  - prograph mcp → send tool call, validate response

Integration (main testing surface)
  cargo test + pytest
  - Rust: full indexer flow on fixture monorepo (tmpdir)
  - Python: CLI on fixture → assert MD bytes + SQLite rows
  - MCP tool dispatcher with mock Store

Unit (many, fast)
  - Rust: discovery, each parser, each edge_detector, diff_engine, store CRUD
  - Python: argparse, MD renderer (str → str), pydantic round-trip from PyO3 objs
```

### 8.2 Fixtures

Three tiers under `tests/fixtures/`:

- **`monorepo_minimal/`** — 2 Python projects, one imports the other. Sanity, used in many unit tests.
- **`monorepo_full/`** — 6–7 synthetic projects covering all edge kinds (python / rust / js / docs / contracts / vendoring style / cross-lang). Trivial code content; structure mimics the real monorepo. This is the CI workhorse.
- **`monorepo_realistic/`** — opt-in (`make test-real`), runs `prograph index` against the real `all_ai_orchestrators/`. Smoke test before each release. Catches real-world edge cases the synthetic fixtures miss.

### 8.3 Golden tests

The highest-value regression net.

```python
def test_md_export_is_idempotent(tmp_path):
    setup_fixture(tmp_path, "monorepo_full")
    run("prograph index --export-md", cwd=tmp_path)
    snap1 = read_dir(tmp_path / ".prograph/projects")
    run("prograph index --export-md", cwd=tmp_path)
    snap2 = read_dir(tmp_path / ".prograph/projects")
    assert snap1 == snap2  # byte-identical

def test_md_matches_golden(tmp_path):
    setup_fixture(tmp_path, "monorepo_full")
    run("prograph index --export-md", cwd=tmp_path)
    for f in (tmp_path / ".prograph/projects").iterdir():
        assert f.read_text() == read(GOLDEN_DIR / f.name)
```

Update via `pytest --update-snapshots`. Golden files are committed; PR diffs surface unexpected output changes.

### 8.4 Diff-engine coverage

Each `(entity_kind, change_kind)` combination has at least one explicit test. Critical cases:

- `package_dep` version bump → `attrs_changed` (NOT remove+add).
- Project directory rename → remove + add (identity = `root_path`).
- Contract content change with same `declared_id` → remove + add.
- Same edge with new evidence row → no new edge, additional `edge_evidence` row.

### 8.5 Failure injection

| Scenario | Expected behavior |
|---|---|
| Corrupted `pyproject.toml` in one project | Snapshot created, project `parse_status=partial`, warning logged, exit 0. |
| Corrupted JSON Schema | Snapshot created, contract skipped, other detectors run, exit 0. |
| Pre-existing `index.lock` | Exit 1 with explanatory message; no DB writes. |
| SIGKILL during Phase 2 | No snapshot; lock file remains (manual cleanup, intentional). |
| SIGKILL during Phase 5 | ROLLBACK; DB on previous snapshot; lock released via crash handler. |
| Empty monorepo | Exit 1 before snapshot creation, helpful message. |
| Cyclic deps | Snapshot has both edges; no infinite loops in graph traversal (property-tested). |

### 8.6 Contract tests on API

- **MCP**: spawn `prograph mcp`, `tools/list` schema-validates; each tool called with typical args, response validated via pydantic.
- **REST**: FastAPI `TestClient` + golden JSON for each endpoint on `monorepo_full`.
- **Pydantic**: round-trip `Model.model_validate(Model(...).model_dump())` for every model.

### 8.7 Performance baseline in CI

```python
def test_index_baseline_speed(tmp_path, benchmark):
    setup_fixture(tmp_path, "monorepo_full")
    duration = benchmark(lambda: run("prograph index", cwd=tmp_path))
    assert duration < 5.0
```

2x baseline regression triggers investigation. Not a hard SLA — CI is noisy.

### 8.8 Not tested

- Tree-sitter / Python `ast` correctness — trust upstream.
- SQLite / rusqlite / PyO3 / FastAPI / mcp-sdk — trust dependencies.
- Exact symbol file:line positions — best-effort, off-by-one not a bug.
- Deep cross-platform — CI on Linux + macOS required, Windows best-effort.
- Full UI — playwright smoke (main page renders, click on node opens panel) only.

### 8.9 CI matrix

| Job | Runs |
|---|---|
| `rust` | `cargo test --workspace`, `cargo clippy -- -D warnings`, `cargo fmt --check` |
| `python` | `uv run pytest`, `uv run ruff check`, `uv run pyrefly check` |
| `e2e-linux` | maturin build + full pytest e2e |
| `e2e-macos` | maturin build + full pytest e2e on macos-latest |
| `smoke-real` | manual / local only — `prograph index` against the real `all_ai_orchestrators/` |

---

## 9. Extensibility & Future Work

The MVP architecture leaves additive hooks for these phases; none require schema migrations of existing tables.

| Phase | Feature | Hook in MVP |
|---|---|---|
| 2 | **TUI (Textual)** | Reads the same SQLite via the same pydantic models. No Rust changes. |
| 3 | **Spec / target-state drift** | New `intents` + `drift_findings` tables (additive). New detector `drift_detector`. New MCP tools `list_intents`, `find_drift`. New MD section behind feature flag in frontmatter. |
| 4 | **Plugin SDK for languages** | `LanguageParser` trait + stable `ProjectFacts` schema are the contract. Python entry-point discovery (`prograph_plugins.languages`). Versioning via `facts_schema_version: 1`. |
| 5 | **HTTP / REST runtime edges** | New parser-level facts (FastAPI / Flask / axum route detection) + new `http_call` detector. |
| 6 | **Vendoring detection** | Content-hash + comment-pattern detector. New `vendored_from` edge kind. |
| 7 | **Incremental reindex** | `Store::ingest_facts(project_id, facts)` already exists as an internal API. Add mtime tracking in `discovery`. |
| 8 | **Multi-monorepo federation** | Reads `.prograph/` from sibling repos, merges into a federated graph. |

### Architectural invariants to preserve

- `ProjectFacts` schema is additive-only; never break the contract.
- MCP tool signatures are additive-only.
- Temporal SQLite schema accepts new entity tables without changes to existing.
- MD frontmatter accepts new keys; body sections append after existing ones.

---

## 10. Risks & Mitigations

| Risk | Mitigation |
|---|---|
| **MCP-call detector unreliable on real codebases** | `detector_status` flag on snapshots; `monorepo_realistic` smoke test required before each release; explicit issue tracker for "broke on real monorepo" cases; can be disabled in `config.toml`. |
| **MD golden tests churn excessively** | Deterministic sort throughout; minimize cosmetic frontmatter; pre-commit hook running `pytest -k golden`. |
| **PyO3 / maturin build complexity in CI** | Dedicated CI job for wheel builds on Linux + macOS; lockfile committed. |
| **Scope creep post-first-demo** | Anti-features list (Section 2); hard rule "new feature = new phase, not MVP scope expansion." |
| **`monorepo_full` ≠ real complexity** | `smoke-real` job; first 2–3 weeks of MVP use treated as field-test, not production. |
| **Idempotency drift breaks all golden tests at once** | Determinism enforced at every serialization boundary; explicit "two consecutive indexes produce identical bytes" test. |

---

## 11. Open Questions

These remain explicitly unresolved and should be revisited during implementation planning:

1. **Project name `prograph`** — is the workspace directory name also the final tool name, or do we want something else? Currently assumed yes.
2. **JS / TS support depth** — TypeScript via `tree-sitter-typescript`, or out of MVP and only plain JS? Affects parser scope by ~30%.
3. **`auto_export_md` default** — true or false? True is friendlier; false saves time on large monorepos. Currently leaning true.
4. **First browser graph library** — d3-force vs cytoscape.js vs reagraph. Affects UI work, not architecture. Decide during UI prototyping.
5. **MCP transport for remote AI** — stdio is enough for Claude Code locally; if remote agents need access, do we add SSE or HTTP transport (in scope for MVP or not)? Currently out of MVP.

---

## 12. Next Step

Per the brainstorming workflow: this spec is reviewed by the user, then handed to the `writing-plans` skill for a step-by-step implementation plan. No code is written before that plan exists and is also reviewed.
