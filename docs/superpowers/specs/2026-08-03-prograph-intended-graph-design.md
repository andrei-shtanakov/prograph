# Intended Graph v1 + `prograph conformance` — design

> Date: 2026-08-03 · Status: **Draft for owner review**
> Upstream decision: ADR behaviour-architecture-lifecycle (2026-08-02, dev
> workspace decision record; D3/D4 define the two-plane model this spec
> implements). Live exemplar: the approved WS-005 governance bundle in steward
> carries an `intended-graph/v0-draft` block (6 components, 4 interfaces,
> 4 constraints with detectors) — schema v1 is designed *from* it, not from a
> blank page.

## Problem

prograph's graph is **descriptive**: it records what the detectors observe
(PackageDep / McpCall / ContractLink / Declared). There is no **prescriptive**
plane — no machine-checkable statement of what the architecture *requires and
forbids* — so architecture drift is invisible until a human notices. M12's
declared edges do not fill this gap: a declaration states "this integration
exists though detectors cannot see it" (descriptive-but-invisible), not "this
integration is intended". Treating declared as intended would let any
violation be legalized by one `reads = [...]` line.

## Decisions

### D1. The manifest is authored data in the *target* repo

- A standalone versioned YAML file, default path `spec/intended-graph.yaml`,
  overridable per project via `[tool.prograph] intended = "<relpath>"`.
- It is an **authored governance artifact**: written by humans/agents, approved
  through the target repo's gates (steward's `architecture`/`design` node owns
  its content). prograph consumes it **read-only** and never generates or
  mutates it.
- Not under `.prograph/` — that directory is derived index data; mixing
  authored and derived planes in one directory is how second-sources-of-truth
  are born.
- Where a governance bundle embeds the graph inside a design artifact (the
  WS-005 case), the standalone file is the machine canon and the design
  artifact references it. Emitting the file from the bundle is the governance
  layer's possible future feature, not prograph's.

### D2. One manifest = one system; components may span projects

The WS-005 exemplar spans steward and dispatcher — cross-project reach is the
normal case, and prograph (the fleet indexer) is exactly the tool that can see
across repos. Therefore:

- `components[].project` names the canonical tracked project (allowlist name).
- Optional `components[].scope` narrows a component to a path prefix inside
  the project. v1 checks at project granularity with scope used only for
  Declared-edge path matching; module-level precision (M9 module facts) is a
  v1.1 extension, listed under open questions.

### D3. Detector vocabulary maps onto existing EdgeKind

| `detector` | Observed as | Notes |
|---|---|---|
| `import` | `PackageDep` | package/dependency edges |
| `mcp` | `McpCall` | MCP client→tool edges |
| `contract` | `ContractLink` | shared-contract edges (`declared_id`/hash identity) |
| `declared` | `Declared` | file integrations via `[tool.prograph] reads/writes` |
| `manual-evidence` | — | no machine detector; verdict is **permanently `unknown`** |

No new detectors are introduced by this feature: the intended plane reuses the
observation machinery that already exists. `manual-evidence` is a first-class
vocabulary member precisely so that unobservable obligations stay *visible* in
every report instead of silently dropped — the WS-005 pilot needed it on its
first artifact (ARCH-C2/C3), not as a corner case.

### D4. Three-valued verdicts; severity is CLI policy, not a verdict property

Every intended element resolves to exactly one of:

- **conformant** — the expected edge/absence is observed;
- **violation** — a forbidden pattern matched an observed edge, or a waiver
  expired;
- **unknown** — the element cannot be machine-observed: `manual-evidence`
  detector, a component whose project is outside the workspace/allowlist, or
  an unresolvable component.

The absence of an observed edge is **not** a violation — dynamic wiring,
generated code and not-yet-written code are indistinguishable to the indexer,
and that indistinguishability is precisely the definition of `unknown`. So the
verdict set stays closed: an interface whose expected edge is absent carries
verdict **`unknown`**, and the `missing-required-edge` finding class preserves
the actionable detail ("the detector was applicable and observed absence") on
top of it. Verdicts attach to intended elements; findings are report entries —
two layers, one closed verdict set. Whether a finding class *blocks* is a
lifecycle-stage policy expressed as CLI flags (`--fail-on`), never baked into
the verdict — the same finding is expected during authoring and blocking
before release.

### D5. Finding taxonomy v1

| Finding | Element verdict | Trigger |
|---|---|---|
| `missing-required-edge` | unknown | an interface with an observable detector has no matching observed edge |
| `forbidden-edge` | violation | a constraint's forbidden pattern matched an observed edge |
| `undeclared-edge` | — (attaches to the manifest, policy-gated) | an observed edge between two *modelled* components appears in no interface |
| `orphan-component` | unknown | `project` not in the workspace, or `scope` matches nothing indexed |
| `expired-waiver` | violation (on the exception) | `exceptions[].expires` is in the past |
| `manual-obligation` | unknown | a `manual-evidence` element, restated in every report |

`undeclared-edge` fires only when **both** endpoints belong to modelled
components — otherwise every unrelated fleet edge becomes noise and the
finding gets muted by habit.

**`contract-pin-drift` is deliberately absent** (it was in the ADR's minimum
set). The WS-005 pilot resolved the ADR's open question 11 with evidence: the
repo-local plane already owns pinned-copy freshness — copy-integrity as a PR
gate plus upstream-drift as scheduled observation (the two-guarantees rule,
dispatcher #99/#107/#110). A third implementation inside prograph would
duplicate an owner-ruled mechanism. prograph's contract coverage in v1 is
`missing-required-edge` on `contract`-detector interfaces; hash-level drift
stays where it lives.

### D6. Constraint rules v1: edge-shaped only

```yaml
constraints:
  - id: ARCH-C1
    rule: "forbidden: dispatcher -> steward"
    detector: import          # kind filter for the match
    evidence: [FR-02]
```

`rule` grammar v1: `forbidden: <endpoint> -> <endpoint>` where `<endpoint>` is
a component id or a project name (glob `*` allowed). Layering shorthands
(`layering: a -> b -> c`) are sugar over forbidden edges and deferred; a
`manual-evidence` constraint carries prose in `rule` and is never matched
mechanically — it exists to be reported as `manual-obligation`.

### D7. `prograph conformance` — CLI surface

```
prograph conformance [--monorepo/-m PATH] [--manifest PATH | --project NAME]
                     [--format text|json]
                     [--fail-on <finding-class>[,<finding-class>...]]
```

- `--monorepo/-m` locates the monorepo root, consistent with every existing
  `prograph` command. `--fail-on` takes the **exact finding-class identifiers
  from D5** (e.g. `--fail-on missing-required-edge,undeclared-edge,
  manual-obligation`) — no shorthand vocabulary to keep the CLI contract and
  the taxonomy one and the same.
- Resolves the manifest (explicit path, or the named project's
  `[tool.prograph] intended`), validates it against the versioned schema,
  diffs against the current snapshot via the existing edge store.
- **Exit codes**: `0` no violations (missing/undeclared/unknown allowed unless
  escalated by `--fail-on`) · `1` violations or escalated findings ·
  `2` tool/config error (unreadable manifest, unknown `schema:`, no snapshot).
  Fail-closed separates "the architecture is broken" from "the instrument
  could not judge" — an unparseable manifest is never a clean run.
- The report lists **every intended element with its verdict** — including
  permanent unknowns — plus provenance: manifest path + content hash, snapshot
  id. No silent truncation: what was not judged is printed as not judged.
- JSON output is byte-stable (sorted keys) for CI artifacts and for the
  governance layer's `GC-ARCH-CONFORMANCE` to consume.

### D8. Ingest: read at check time, not indexed

The manifest is parsed per invocation and never written into `graph.db`. The
store stays purely descriptive; the intended plane remains an input. This
keeps `conformance` runnable against any snapshot (`--since`-style comparisons
are a natural v1.1) and avoids schema migrations for an authored format that
governance, not prograph, evolves.

## Schema v1 (normative sketch)

```yaml
schema: intended-graph/v1
system: ws005-governance-panel        # stable system id (kebab)
components:
  - id: dispatcher.governance-collector
    project: dispatcher               # canonical tracked-project name
    scope: dispatcher/core/governance.py   # optional path prefix
    kind: module                      # service|module|cli|ui|contract|store
    owner: architects                 # role slug (DEC-007 form, no '@')
    responsibility: "чтение файла + git-фактов; классификация в 6 состояний"
    evidence: [BEH-02, NFR-01]        # opaque ids; governance owns their meaning
interfaces:
  - id: I-02
    producer: "file:.steward/gate_verdicts.jsonl"   # file endpoint form
    consumer: dispatcher.governance-collector
    protocol: "jsonl / gate-verdicts/v1"
    detector: declared
constraints:
  - id: ARCH-C1
    rule: "forbidden: dispatcher -> steward"
    detector: import
    evidence: [FR-02]
resources: []                          # informational in v1, not checked
exceptions:
  - id: EX-01
    target: I-02                       # suppresses findings on that element
    reason: "…"
    owner: architects
    expires: 2026-09-01                # ISO date, required
```

- `file:<path>` endpoints match Declared-edge path attributes (M12 semantics:
  repo-relative paths, repo names not runtime ids).
- `evidence` values are opaque to prograph — traceability into FR/BEH/NFR is
  the governance linter's job (`GC-ARCH-EVIDENCE`), not the indexer's.
- Unknown top-level keys are a schema error (strict, like gate-verdicts/v1):
  the producer is an approved artifact, drift must be loud.

## Non-goals

- prograph does not become a lifecycle/state orchestrator; approval, staleness
  and re-approval of the manifest belong to the governance layer.
- Declared edges are never auto-promoted to intended (ADR D4: promotion is a
  PR against the manifest).
- Fleet-level graph-vs-registry checking stays in the umbrella workspace
  tooling (owner ruling, TODO graph-vs-registry item) — this feature is
  repo-local intended-vs-actual for one system at a time.
- No C4/Structurizr import-export until a real manifest outgrows this schema
  (ADR open question 5; the WS-005 exemplar fits with room to spare).

## Open questions for the owner

1. Default manifest path: `spec/intended-graph.yaml` (proposed) vs
   `.prograph/`-adjacent. The proposal keeps authored governance data with the
   other spec artifacts.
2. `undeclared-edge` default policy: report-only (proposed) or fail by
   default? Proposed report-only until one real system has run conformance in
   CI for a while.
3. v1.1 candidates, ranked: module-level component resolution (M9 facts),
   `--since` snapshot comparisons, layering sugar. Anything to promote into
   v1?

## Rollout

1. Owner review of this spec → decisions on the open questions.
2. Extract the first real manifest from the WS-005 bundle's v0-draft
   (steward-side PR: standalone file + pointer from the design artifact).
3. Implementation plan (separate doc, house style): schema validator + loader,
   endpoint/component resolution, finding engine, CLI + JSON, fixtures for
   every finding class (positive + negative + unknown), golden report.
4. Wire `GC-ARCH-CONFORMANCE` on the governance side only after the CLI
   exists (their TODO already gates this on "intended schema v1 согласована").
