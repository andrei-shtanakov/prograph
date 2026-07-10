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
