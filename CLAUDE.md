# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository scope

`prograph` is a Rust + Python tool that maps a monorepo of independent projects, detecting cross-project structure (package dependencies, shared contracts, MCP calls) and exposing the result to humans (browser UI) and AI agents (MCP server).

This is a from-scratch design replacing the vendored archived `Sourcetrail/` subdirectory (C++/Qt5, kept as historical reference only — its own .git, untouched by our build).

## Design and plans

- Full design spec: `docs/superpowers/specs/2026-05-25-prograph-design.md`
- M1 (foundation) plan: `docs/superpowers/plans/2026-05-25-prograph-m1-foundation.md`
- M2 (Python indexer) plan: `docs/superpowers/plans/2026-05-25-prograph-m2-python-indexer.md`
- M3 (multi-language) plan: `docs/superpowers/plans/2026-05-26-prograph-m3-multilang-indexer.md`
- M4 (contracts + MCP) plan: `docs/superpowers/plans/2026-05-26-prograph-m4-contracts-mcp.md`
- M5 (Markdown exporter) plan: `docs/superpowers/plans/2026-05-26-prograph-m5-md-exporter.md`
- M7 (MCP stdio server) plan: `docs/superpowers/plans/2026-05-26-prograph-m7-mcp-server.md`
- M6 (browser UI) plan: `docs/superpowers/plans/2026-05-26-prograph-m6-browser-ui.md`
- M8 (polish & v1.0) plan: `docs/superpowers/plans/2026-05-26-prograph-m8-polish.md`
- M9 (module-level facts) plan: `docs/superpowers/plans/2026-05-26-prograph-m9-module-facts.md`
- M10 (cross-project symbol references) plan: `docs/superpowers/plans/2026-05-26-prograph-m10-symbol-refs.md`
- M11 (drift detection) plan: `docs/superpowers/plans/2026-05-26-prograph-m11-drift-detection.md`

## Build system

**Rust core** (`prograph-core/` crate) — PyO3 extension module, built into `prograph._core`. Pinned to Rust 1.85 via `rust-toolchain.toml` (bumped from 1.75 for PyO3 0.29, which needs rustc ≥ 1.83).
**Python wrapper** (`prograph/` package) — CLI (typer), pydantic mirrors of Rust dataclasses, paths helper. Uses uv for dependency management.

Build via maturin (mixed layout): `uv sync` resolves Python deps AND invokes maturin to build the Rust extension. The maturin manifest path is in `pyproject.toml` under `[tool.maturin]`.

**After editing any Rust source, rebuild before Python picks up the change** — Python imports the compiled `prograph/_core.*.so`, not the crate:

```sh
uv run maturin develop            # recompile + reinstall the extension in the venv
```

`cargo test` exercises Rust logic without rebuilding the extension, but pytest/CLI/MCP will keep running the stale `.so` until you `maturin develop`. Regenerate the type stub (`prograph/_core.pyi`) by hand when the PyO3 surface changes — it is not auto-generated.

### Tooling pins worth knowing

- **PyO3 is 0.29** (bumped from 0.22). The crate uses the `Bound` API throughout; `#[pyclass]` data classes that derive `Clone` carry `skip_from_py_object` (output-only) while the enum pyclasses carry `from_py_object` (e.g. `ProjectKind`, consumed by `ProjectCandidate::new`). GIL access is `Python::attach` / `Python::initialize` (0.29 renamed `with_gil` / `prepare_freethreaded_python`).
- `.cargo/config.toml` sets `PYO3_USE_ABI3_FORWARD_COMPATIBILITY=1` so `cargo test` works against Python newer than PyO3 0.29's abi3 cap (e.g. 3.14).
- The `extension-module` PyO3 feature is gated on a Cargo feature (`prograph-core/Cargo.toml [features]`) and enabled only via maturin — never by `cargo test` (which needs libpython linking).
- `tempfile` is unpinned (`"3"`) now that MSRV is 1.85; the old `=3.15.0` pin was only needed under Rust 1.75. `indexmap` remains at 2.7.1 in Cargo.lock — 1.85 now permits 2.14+ (edition2024), but it is left pinned for a minimal diff; `cargo update -p indexmap` relaxes it.
- `tree-sitter`, `tree-sitter-python`, `tree-sitter-rust` (M4) compile C source via `cc-rs`. First build takes ~60s; subsequent builds reuse the cache.

## Common commands

```sh
uv sync                                      # install deps + build Rust extension
uv run prograph init [--monorepo PATH]       # create .prograph/ skeleton
uv run prograph status [--monorepo PATH] [--json]   # show project candidates
uv run prograph index [--monorepo PATH] [--export-md] [--json]   # index with optional MD export
uv run prograph export-md [--monorepo PATH]                      # re-render MD from latest snapshot
uv run prograph mcp [--monorepo PATH]                            # run MCP stdio server
uv run prograph serve [--monorepo PATH] [--host 127.0.0.1] [--port 7700]  # browser UI + REST

cargo test --all-targets                     # Rust tests
uv run pytest -v                             # Python tests (excludes realmonorepo + bench)
uv run pytest -m realmonorepo -v             # opt-in real-monorepo smoke
uv run pytest -m bench -v                    # opt-in performance baselines
uv run pytest tests/unit/test_models.py -v   # single file
uv run pytest tests/unit/test_models.py::test_name   # single test
uv run pytest -k drift                        # tests matching a keyword
cargo test --all-targets drift               # single Rust module/test by name

cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
uv run ruff check .
uv run ruff format --check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
```

## Architecture (M11 state)

Two-layer build:

- **`prograph-core` (Rust crate via PyO3):**
  - `discovery` recurses one level into Cargo + Python workspaces (M8)
  - `parsers/{python,rust,js,contracts}` — Python + Rust now ALSO emit `external_imports` per Module (M10); JS still manifest-only for cross-project refs
  - `intent/{mod,markdown}` — line-based markdown intent parser (M11)
  - `detectors/{deps,contracts,mcp}` — all three kinds populate `EdgeCandidate.evidence` (M1-M8)
  - `resolvers/{python,rust}` — M10: dotted-name / `crate::a::b::Sym` → publisher project + sub-module path + symbol
  - `drift` — detect_missing / detect_extra / detect_stale_todos (M11)
  - `diff`, `lock`, `indexer`
  - `store` — SQLite schema **v9** (M11 adds `drift_findings`; v8 = M10 cross_project_symbol_refs; v7 = M9 module tables); query helpers `describe_*`, `monorepo_overview`, `project_by_name`, `snapshot_by_id`, `find_edges_filtered`, `find_edges_with_status_since`, `edge_evidence_for`, `search_fts`, `changelog_paginated`, **`refs_to_symbol`**, **`refs_from_project`**, **`drifts_for_project`**, **`find_drifts_filtered`**, **`recent_changelog_labels`**
  - `models` — pyclasses incl. M11 `DriftFindingRow`, M10 `SymbolRefRow`, M9 `ModuleRow`/`PublicSymbolRow`/`InternalImportRow`, M8 `DiffEdgeRow`, M7 `EdgeRow`/`EdgeEvidenceRow`/`SearchHit`
  - `facts` — `Manifest`, `McpToolDecl`, `McpClientUse`, `ContractFile`, `Module`, `PublicSymbol`, `InternalImport`, `ExternalImport` (M10), `IntentDoc`/`IntentItem`/`TodoItem` (M11), `SymbolKind`, `ProjectFacts`
  - `parsers/{python,rust}` append `.prograph/mcp_patterns/{python,rust}.scm` overrides to the bundled tree-sitter queries
  - `ts_queries/{python,rust,js}_symbols.scm` — module-level queries
  - `migrations/v1.sql..v9.sql` — additive schema chain (v6 = edge_evidence FK repair, v7 = module tables, v8 = cross_project_symbol_refs, v9 = drift_findings)
- **`prograph` (Python package):**
  - `cli.py` — `init`, `index`, `status`, `export-md`, `mcp`, `serve`, `drift` (M11), `--version`
  - `web_app.py` — FastAPI app + 13 REST endpoints; `/api/drifts?project=X[&kind=...]` (M11); `/api/symbol_refs?project=X[&symbol=Y][&direction=...]` (M10); `/api/graph?since=<snap>` (M8)
  - `web_static/` — Static frontend; M11 side panel adds Drift findings section; M10 Inbound/Outbound references sections; XSS-safe DOM helpers
  - `mcp_server.py` — MCP stdio server with **10 tools** (M11 adds `find_drifts`; M10 added `find_symbol_references`)
  - `export/` — Markdown rendering with M11 Drift findings, M10 Inbound/Outbound references sections
  - `config.py`, `models.py` (incl. `DriftFindingRow`, `SymbolRefRow`, `DiffEdgeRow`, `ModuleRow`, etc.), `paths.py`

### Frontend DOM safety

All static frontend code uses `prograph/web_static/dom.js`'s `el(tag, attrs, children)` helper to construct DOM. **No `innerHTML` assignments anywhere.** The unit test `tests/unit/test_web_static.py::test_app_js_does_not_use_innerHTML` enforces this. If you add UI code, route it through `el()` — pass user-controlled values as string children (auto-escaped via `createTextNode`) or as attribute values.

The Rust↔Python boundary remains data-only.

### MCP detection pattern overrides

`.prograph/mcp_patterns/{python,rust}.scm` files are appended to the bundled tree-sitter queries at parse time. Use them to recognise project-specific MCP idioms without forking the crate.

Tests live in `tests/` (pytest) and as inline `#[cfg(test)]` modules in each Rust source file.

The Rust↔Python boundary remains data-only.

### Workspace aliases

Workspace orchestrators that publish under multiple names declare aliases in `pyproject.toml`:

```toml
[tool.prograph]
aliases = ["alt-name-1", "alt-name-2"]
```

The detector then resolves consumer deps against `declared_name` OR any alias. Name collisions across projects emit warnings counted in `IndexSummary.n_warnings`.

## What is NOT in M11 (deferred to M12+)

- Auto-fix proposals — drift is reported, not auto-resolved.
- Renamed-symbol pairing — missing/extra emit as separate findings without "looks like a rename" suggestion.
- Drift trend charts — temporal data is stored; visualisation deferred.
- Cross-project drift — "Maestro spec says it uses arbiter::Decider but M10 doesn't show the import". Possible follow-up using symbol_refs table.
- TODO matching to external issue trackers (Linear / GitHub) — local-only.
- Type signatures + docstrings — still M9 deferred.
- HTTP / REST runtime edges. Still M8 deferred.
- WebSocket live updates, offline asset bundle, Playwright E2E, auth/TLS, mobile/responsive. Still M8 deferred.

(See `docs/superpowers/plans/` for individual milestone plans.)

### Golden tests

`tests/fixtures/<name>/golden/` directories hold the expected MD output for each fixture. After intentional renderer changes, regenerate with:

```sh
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest tests/integration/test_cli_export_md.py::test_golden_monorepo_full
```

Then `git diff` to review the change before committing.

## Conventions

- Type hints throughout Python (pyrefly enforces).
- Use uv (never pip).
- Ruff line-length is **100** here (`pyproject.toml [tool.ruff]`), not the 88 from the global guidelines — this project's config wins. `tests/fixtures/` is excluded from lint/format because its `.py` files carry intentionally-shaped imports/symbols for the parser.
- Always run pyrefly via the CLI with explicit globs (`uv run pyrefly check 'prograph/**/*.py' ...`), never bare `pyrefly check` in project mode — project-mode excludes collide with `.gitignore`'s `*.py[cod]` line (misread as `*.py*`). See the `[tool.pyrefly]` comment in `pyproject.toml`.
- Follow `.gitignore`'s `/.prograph/graph.db` etc. patterns so prograph self-hosting artefacts stay out of git.
- The `Sourcetrail/` subdir is its own git repository; don't recurse into it from our toolchain. It is also listed in `pyproject.toml [tool.prograph] exclude` so workspace recursion skips it.

## Repo scope & boundaries

- **Этот репо:** `prograph` — git-корень `all_ai_orchestrators/prograph/`, remote `git@github.com:andrei-shtanakov/prograph.git`.
- **Соседи (READ-ONLY reference):** `../arbiter/`, `../atp-platform/`, `../deployer/`, `../dispatcher/`, `../Maestro/`, `../open-prose/`, `../proctor/`, `../prograph-vault/`, `../robin-runtime/`, `../robin-toolkit/`, `../spec-runner/`, `../spec-runner-vscode/`, `../steward/` — их код не редактировать.
- Нужна правка у соседа → **стоп**: запиши handoff в `../prograph-vault/authored/notes/`
  (кросс-проектное) или `../_cowork_output/` (черновик), не трогай его файлы.
- Кросс-репные контракты — **вендорить пиненой копией внутрь**, не ссылаться наружу.
- Полное правило (SSOT): `../prograph-vault/authored/rules/repo-boundaries.md`.

## Git workflow (у репо есть remote)

- Ветка `<type>/<slug>` → push → `gh pr create`. **Прямые коммиты в `master` запрещены.**
- После открытия PR — прочитать ревью **GitHub Copilot**: валидные замечания исправлять
  новыми коммитами в ту же ветку; невалидные — ответить с обоснованием, **не применять
  вслепую**; итерировать, пока не останется открытых замечаний.
- **Не мержить.** Мерж делает пользователь.
- После мержа пользователем: `git switch master && git pull --ff-only`, затем удалить
  влитую ветку (`git branch -d <branch>`) и `git fetch --prune`; убрать прочие влитые ветки.
- Никогда не делать force-push в общие ветки; не трогать другие репо (см. scope выше).
- Полное правило (SSOT): `../prograph-vault/authored/rules/git-workflow.md`.
