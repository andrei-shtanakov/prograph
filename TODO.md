# TODO

## TODO

- [ ] **Declared edges (M12 candidate): file-based integrations the detectors cannot see.**
  Case study (2026-07-10): `dispatcher/core/collectors/proctor.py` reads proctor's
  `config/proctor.yaml`, `data/state.db` and logs straight off disk — no import, no MCP
  call, no shared contract file. All three detectors (deps / contracts / mcp) are blind to
  it, so the graph shows proctor as fully isolated while the integration map in
  COWORK_CONTEXT has dispatcher ↔ proctor connected since 2026-07-05. Dispatcher is built
  this way on purpose ("reads on-disk artifacts, projects need not be running"), so ALL of
  its edges to the projects it watches are invisible, not just proctor.
  Proposal: let a project *declare* such integrations in its manifest, e.g.
  `[tool.prograph] reads = ["proctor/data/state.db", "proctor/config/proctor.yaml"]`
  (and/or `writes = [...]`). The indexer resolves the path prefix to a publisher project
  and emits an edge with a new evidence kind `declared` — rendered dashed in the browser
  UI as "declared, not detected". Drift detection extends naturally: a declared edge whose
  target path no longer exists is a `stale declaration` finding.
  Related noise for any graph tool: repo namespace vs runtime service-id split
  (repo `proctor` vs service `proctor-a`, ADR 2026-07-07) — declared edges should name
  repo paths, not runtime ids.
- [ ] **Graph-vs-registry drift check** — "every link in the COWORK_CONTEXT integration
  map has a corresponding graph edge" is a fleet-agent invariant, not a prograph feature;
  tracked in `devtools/proposals/2026-07-10-graph-vs-registry-check.md`. prograph's part
  is only to expose the edge list cheaply (already done: `find_edges` MCP tool /
  `/api/graph`). Once declared edges exist, that check stops false-positiving on
  file-based integrations.

## Exporter hygiene (from prograph-vault PR #10 Copilot review, 2026-07-11)

Three issues surfaced when Copilot reviewed a `derived/` refresh export. All are
exporter/indexer bugs — the fix belongs here, then regenerate the vault (`export-md`).

- [ ] **Absolute monorepo path leaks into the export.** `derived/graph/index.md` renders
  `# Monorepo: /Users/<user>/labs/all_ai_orchestrators`, embedding a personal filesystem
  path + username. Makes the exported vault non-portable and leaks workstation details.
  Render a repo-relative identifier (e.g. the basename `all_ai_orchestrators`) or a
  redacted placeholder in the graph-index renderer (`prograph/export/`), not the resolved
  monorepo root.
- [ ] **Graph-index contract list is not de-duplicated.** `derived/graph/index.md` lists
  the same logical contract multiple times for one `declared_id` (e.g. spec-runner
  `costs`/`json-result`/`spec-frontmatter`/`status` schemas) with differing owner counts,
  making the index misleading. De-duplicate by `declared_id` (fallback `content_hash`)
  when rendering the index and recompute the displayed owner counts — mirror the grouping
  already used elsewhere for contract co-owners.
- [ ] **kb-save journals live under regenerable `derived/`.** `derived/journal/**` (e.g.
  `derived/journal/prograph/journal.md`) is written by the `kb-save` skill and marks
  itself "not authoritative, not regenerable / append-only", yet sits under `derived/`
  whose contract is "regenerable export output". A future `export-md` refresh could
  overwrite an append-only log. Either move kb-save journals out of `derived/` or have the
  exporter treat `derived/journal/**` as off-limits (never write/overwrite there).
