# TODO

## TODO

- [x] **Declared edges (M12): file-based integrations the detectors cannot see.**
  Case study (2026-07-10): `dispatcher/core/collectors/proctor.py` reads proctor's
  `config/proctor.yaml`, `data/state.db` and logs straight off disk — no import, no MCP
  call, no shared contract file. All three detectors (deps / contracts / mcp) are blind to
  it, so the graph shows proctor as fully isolated while the integration map in
  COWORK_CONTEXT has dispatcher ↔ proctor connected since 2026-07-05. Dispatcher is built
  this way on purpose ("reads on-disk artifacts, projects need not be running"), so ALL of
  its edges to the projects it watches are invisible, not just proctor.
  Shipped: a project can *declare* such integrations in its manifest, e.g.
  `[tool.prograph] reads = ["proctor/data/state.db", "proctor/config/proctor.yaml"]`
  (and/or `writes = [...]`). The indexer resolves the path prefix to a publisher project
  and emits an edge with kind `declared` — rendered dashed in the browser UI
  as "declared, not detected". Drift detection reports a declared path whose
  target no longer exists as a `stale_declaration` finding.
  Related noise for any graph tool: repo namespace vs runtime service-id split
  (repo `proctor` vs service `proctor-a`, ADR 2026-07-07) — declared edges should name
  repo paths, not runtime ids.
- [ ] **Graph-vs-registry drift check** — "every link in the COWORK_CONTEXT integration
  map has a corresponding graph edge" is a fleet-agent invariant, not a prograph feature;
  tracked in `devtools/proposals/2026-07-10-graph-vs-registry-check.md`. prograph's part
  is only to expose the edge list cheaply (already done: `find_edges` MCP tool /
  `/api/graph`). With declared edges shipped, the next step is to require
  registry links to have either detected or manifest-declared graph evidence.

## Exporter hygiene (from prograph-vault PR #10 Copilot review, 2026-07-11)

Three issues surfaced when Copilot reviewed a `derived/` refresh export. All are
exporter/indexer bugs — the fix belongs here, then regenerate the vault (`export-md`).

- [x] **Absolute monorepo path leaks into the export.** ✅ `render_index` now renders the
  repo-relative basename (`# Monorepo: all_ai_orchestrators`) via `PurePath(...).name`, not
  the resolved absolute root — no more home dir / username leak. (`render.py`; test
  `test_render_index_uses_repo_relative_root_not_absolute_path`.)
- [x] **Graph-index contract list is not de-duplicated.** ✅ `render_index` de-duplicates by
  `declared_id` (contracts without one stay per-slug); the displayed owner count is the max
  across the merged rows (rows overlap — never sum). (`render.py`; tests
  `test_render_index_dedups_contracts_by_declared_id`,
  `test_render_index_keeps_hashonly_contracts_distinct`.)
- [x] **kb-save journals live under regenerable `derived/`.** ✅ Verified safe by design: the
  stale-MD cleanup (`_cleanup_stale_project_mds`) is scoped to `projects/` and gated on the
  `<!-- prograph:generated -->` marker, and the exporter never writes under `journal/`. A
  journal file — even one carrying the marker — survives an export refresh. Locked by a
  regression test (`test_export_md_leaves_journal_untouched`). No code change needed.
