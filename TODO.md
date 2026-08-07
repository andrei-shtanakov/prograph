# TODO

Open items carry optional plan-fields v2 inline tags — `@owner:`, `@blocked_by:`,
`@trigger:`, `@id:` — in the tail of the checkbox line. For `@owner:` the canonical
values are `github:<login>`, `github-team:<org>/<team>`, `repo:<manifest-key>`, and
`TBD`; `<manifest-key>` is the canonical repository key declared in the workspace
manifest, and bare handle/role values are legacy. `@id:` is the stable item identifier
used by canonical `todo://<repo>/<id>` references. Robin's parser
(robin-runtime#27, merged 2026-07-26) strips them from the item identity key, so tagging
does not orphan an item's history. An absent tag means "not decided yet" — do not invent
values to fill the column.

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
- [x] **Intended graph v1 + `prograph conformance`** — shipped: strict @owner:github:andrei-shtanakov @id:intended-graph-v1
  `intended-graph/v1` loader, three-valued verdict engine (honest
  `unsupported-resolution` per D2), finding taxonomy v1, CLI with byte-stable
  JSON and 0/1/2 exit codes; WS-005 manifest (steward@727a28d) vendored as
  acceptance fixture. Spec:
  `docs/superpowers/specs/2026-08-03-prograph-intended-graph-design.md`; plan:
  `docs/superpowers/plans/2026-08-03-prograph-conformance-v1.md`. Follow-ups
  live in the spec's v1.1 list (module-level resolution, `--since`, layering
  sugar). Consumer: steward `GC-ARCH-CONFORMANCE` (@trigger there is
  "prograph conformance реализован" — теперь выполнен).
- [ ] **Graph-vs-registry drift check** @owner:github:andrei-shtanakov @id:graph-vs-registry-drift-check
  The invariant — "every link in the integration map has a corresponding graph edge, and
  every graph edge is in the map" — is a fleet-agent check, not a prograph feature; it is
  specified in `../devtools/proposals/2026-07-10-graph-vs-registry-check.md` (status:
  proposal, 2026-07-10) and would live in umbrella-workspace `../devtools/`, not in this
  repo — that path resolves only inside the workspace checkout.
  prograph's own part is to expose the edge list cheaply, and that is **done**: `find_edges`
  MCP tool / `GET /api/graph` / `.prograph/graph.db`.
  The proposal's stated precondition — "ложные срабатывания на файловых интеграциях
  исчезнут, когда prograph научится declared edges" — was met when M12 shipped, so the
  allowlist workaround it describes is no longer needed. Authored side of the diff is
  `../prograph-vault/authored/registry/registry.md` ("Integration map"). Kept on this list
  because the check consumes prograph's output; the implementation itself is not ours.

- [ ] **Workspace allowlist and index snapshot have drifted** @owner:github:andrei-shtanakov @id:workspace-allowlist-index-drift
  Measured 2026-07-26 against `../.prograph/tracked.toml` (the umbrella workspace's own
  allowlist, one level up — not this repo's):
  `open-prose` is still listed but the directory no longer exists — it was renamed to
  `libretto` on 2026-07-16 — so `index --discover` reports it as "allowlisted but not
  found", while `libretto` and `discovery` show up as untracked. This is the same stale
  rename that `../_cowork_output/2026-07-26-robin-mirror-list-drift-handoff.md` found in
  Robin's mirror list; both configs were written before the rename.
  `.prograph/graph.db` was last written 2026-07-10, which is why
  `registry.md` still calls coverage for robin-runtime, robin-toolkit, deployer, libretto,
  steward and discovery "sparse/intent-only until the next full prograph export".
  Fix is operational, not code: update the allowlist, re-run `prograph index --discover
  --export-md`, then re-check the registry. Note the file lives in the umbrella workspace
  (root git repo, no remote), not in this repo — it is edited there, not via a prograph PR.

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
