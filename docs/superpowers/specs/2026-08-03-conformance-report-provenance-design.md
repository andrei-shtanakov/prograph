# Conformance report as versioned evidence — provenance + published schemas

> Date: 2026-08-03 · Status: **Draft — awaiting owner review**
> Upstream: intended-graph v1 spec (#22, shipped in #24) and the owner ruling of
> 2026-08-03 accepting «вариант 1» for steward's `GC-ARCH-CONFORMANCE`: the report is
> versioned evidence consumed offline; a green gate over a correct-but-stale report is
> the same defect class WS-005 was built against.
> Consumer: steward `GC-ARCH-{SCHEMA,EVIDENCE,CONFORMANCE}` (their TODO
> `@id:behaviour-arch-gates`, inbox steward#36).

## Problem

`conformance-report/v1` as shipped proves *what the verdicts were*, but not *what they
were about*: provenance carries only the manifest hash and a snapshot id. A consumer
that re-checks «manifest hash unchanged» can still be looking at verdicts computed over
code that has since moved — the manifest is stable precisely because it is authored
governance data. Freshness has **two dimensions**: the manifest and the observed code.
The report attests to neither the code state nor its own completeness, and the
`intended-graph/v1` schema exists only as pydantic models inside prograph — nothing a
neighbour can vendor byte-pinned.

## Decisions

### D1. Extend `conformance-report/v1` in place

No consumer has shipped against the current payload (steward's gates are not built
yet), so the missing provenance is added to **v1** rather than minting v2. After this
spec lands, the payload is frozen: further shape changes bump the version.

### D2. Provenance block (normative)

```yaml
manifest:
  path: spec/intended-graph.yaml     # relative to the monorepo root when possible
  sha256: <hex>                      # of the manifest bytes the run judged
snapshot:
  id: 7
  content_hash: <hex>                # D4 — deterministic hash of snapshot content
  generated_at: 2026-08-03T12:41:07Z # report generation time, UTC ISO-8601
  complete: true                     # D5 — explicit no-truncation attestation
tool:
  name: prograph
  version: 1.4.0                     # package version that produced the report
  schema: intended-graph/v1          # manifest schema the run validated against
projects:                            # D3 — every project named by a manifest component
  steward:   {commit: <hex|null>, dirty: false}
  dispatcher: {commit: <hex|null>, dirty: false}
```

`elements`, `findings`, `exceptions`, `summary` are unchanged (full lists, no
truncation — that is what `complete` attests). JSON stays byte-stable
(`sort_keys`, stable ordering); `generated_at` is the only field that varies between
two runs over identical inputs.

### D3. Per-project provenance is captured at **index time**, not report time

The verdicts derive from the snapshot, so the evidence chain must name the code state
**as indexed** — a check-time `git rev-parse` would happily stamp today's SHA onto
verdicts computed over yesterday's tree, which is exactly the stale-report defect.

- The indexer records, per tracked project: `git_commit` (HEAD at index time,
  `NULL` for non-git directories) and `git_dirty` (worktree had uncommitted changes,
  `NULL` when not a git repo). Schema migration **v11** (additive columns on the
  per-snapshot project rows, consistent with the additive chain v1..v10).
- `prograph conformance` copies these as-indexed values into `projects` for every
  project referenced by a manifest component. A project absent from the snapshot
  (outside-workspace) appears as `{commit: null, dirty: null}` — consistent with its
  `unknown` verdict.
- Consumer semantics (steward PR-gate): «my HEAD == report.projects.steward.commit»
  now proves the verdicts were computed over exactly the tree the PR sees. `null`
  commit or `dirty: true` is *unfreshness the policy must treat as unknown*, never as
  clean.

### D4. `snapshot.content_hash`

SHA-256 over a canonical serialization of the snapshot's node and edge sets (sorted,
attrs included), computed at report time from the store. Two snapshots of identical
observed structure hash identically even if their `id`/`ts` differ; any structural
change flips the hash. This is the report's second freshness anchor for the scheduled
workspace check (D7) and costs one read of data the engine already loads.

### D5. `complete`

An explicit attestation by the tool: **every** intended element appears in `elements`,
**every** computed finding appears in `findings`, and no cap, sampling or truncation
was applied. v1 always emits `true` — the flag exists so any future bounded mode is
forced to declare itself, and so the consumer can fail closed on its absence
(`complete != true` ⇒ instrument failure, not a clean run). Exit-2 paths still produce
no report at all.

### D6. Published contract artifacts (what steward vendors)

Two JSON Schema (draft 2020-12) files become part of the repo, under the
gate-verdicts-style contract layout:

- `contracts/intended-graph/v1/schema.json` — the authored-manifest schema.
  Owner: prograph (semantics live in `conformance/manifest.py`). steward's
  `GC-ARCH-SCHEMA` validates the manifest with a stock JSON Schema engine against its
  **byte-pinned vendored copy** — no second parser, no re-derived rules.
- `contracts/conformance-report/v1/schema.json` — the report schema incl. the D2
  provenance block. steward's `GC-ARCH-CONFORMANCE` step 1 validates the report
  against its pinned copy.

Sync is enforced on prograph's side by tests, not by trust:

- every loader-accepted fixture manifest (monorepo_conformance, ws005_manifest, the
  unit-test VALID document) validates against `intended-graph/v1/schema.json`, and the
  loader-rejection fixtures fail it for the same reason class;
- every `report_payload()` produced in the test suite validates against
  `conformance-report/v1/schema.json` (golden + engine unit reports).

Vendoring by consumers follows the two-guarantees rule (copy-integrity as their
PR-gate, upstream-drift as scheduled observation) — prograph's obligation is only that
the file in `contracts/` **is** the schema the code implements.

### D7. Freshness ownership split (boundary statement)

prograph makes the report *carry the facts*; it does not check freshness policy.

- **PR-gate (steward, offline):** report validates against pinned schema; manifest
  sha256 matches the checked-out manifest; `complete: true`; own repo HEAD equals
  `projects.<self>.commit` and `dirty: false`; report age within stage policy.
- **Scheduled workspace check (umbrella side):** re-derives every listed project's
  HEAD against `projects.*`, and the workspace snapshot against
  `snapshot.content_hash`; refreshes the evidence or opens drift. Its absence or
  expiry is **unknown, never clean** — the second guarantee is observation, not
  assumption.
- Stage policy (`fail_on_findings` / `fail_on_verdicts` / `max_report_age` per
  authoring|release) is steward data; prograph never interprets it.

## Non-goals

- Stage-policy format and gate implementation — steward's (ADR behaviour-lifecycle
  Phase 1, `GC-ARCH-*`).
- The scheduled workspace checker — umbrella/devtools tooling, not shipped code in
  either repo.
- Report signing/attestation cryptography — out of scope for v1 evidence.

## Open questions (owner)

1. **v11 migration vs check-time git.** D3 argues index-time capture is the only
   honest chain and takes the additive migration. Confirm the migration is acceptable
   now (alternative — check-time capture — is cheaper but reintroduces the stale-SHA
   hole this spec exists to close).
2. **Dirty trees at index time.** D3 records `dirty: true` and leaves refusal to
   policy. Should `prograph index` additionally *warn* on dirty tracked projects, or
   stay silent and let the report carry the fact?
3. **`generated_at` and the golden.** The golden test will normalize `generated_at`
   (and, for the fixture's non-git projects, `commit: null`) before comparison —
   byte-stability claim then reads «byte-stable modulo `generated_at`». Acceptable, or
   should the CLI grow a test-only clock override to keep the golden literally
   byte-exact?

## Rollout

1. Owner review of this spec → resolve the three questions.
2. Implementation plan (house style): v11 migration + indexer git capture → report
   provenance block → contract schema files + sync tests → golden refresh.
3. steward side (their repo, after this ships): vendor both schemas pinned, implement
   `GC-ARCH-SCHEMA` / `GC-ARCH-EVIDENCE` / `GC-ARCH-CONFORMANCE` as offline consumers,
   wire stage policy — per the owner ruling recorded above.
