# Conformance report as versioned evidence — provenance + published schemas

> Date: 2026-08-03 · Status: **Revised after owner review (2026-08-03) — awaiting approval**
> (review verdict: pattern confirmed; two mandatory fixes applied — report/snapshot time
> split, JSON-Schema-vs-loader boundary — plus the minor clarifications; all three open
> questions resolved by the owner, see «Resolved questions»)
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
generated_at: 2026-08-03T12:41:07Z   # REPORT time: when this JSON was formed (UTC ISO-8601)
manifest:
  project: steward                   # tracked project owning the manifest
  path: spec/intended-graph.yaml     # relative to THAT project's root (unambiguous cross-repo)
  sha256: <hex>                      # of the manifest bytes the run judged
snapshot:
  id: 7
  indexed_at: 2026-08-02T09:15:44Z   # SNAPSHOT time: when code + git provenance were captured
  content_hash: "prograph-snapshot/v1+sha256:<hex>"   # D4 — versioned canonicalization
  complete: true                     # D5 — producer's no-truncation assertion
tool:
  name: prograph
  version: 1.4.0                     # package version that produced the report
  schema: intended-graph/v1          # manifest schema the run validated against
projects:                            # D3 — every project named by a manifest component
  steward:   {commit: <hex|null>, dirty: false}
  dispatcher: {commit: <hex|null>, dirty: false}
```

**Report age and snapshot age are distinct freshness dimensions.** `generated_at`
(top-level) dates the JSON itself; `snapshot.indexed_at` (from the store's snapshot
timestamp) dates the code state the verdicts describe. A fresh report over a
month-old snapshot must be catchable: consumer freshness policy MUST check at least
the **snapshot** age; report age is an optional additional bound where it carries
operational meaning. (`content_hash` proves snapshot *identity*, never its
*freshness*.)

`elements`, `findings`, `exceptions`, `summary` are unchanged (full lists, no
truncation — that is what `complete` asserts). JSON stays byte-stable
(`sort_keys`, stable ordering); with a fixed clock and fixed inputs the payload is
byte-exact (D8).

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

### D4. `snapshot.content_hash` — versioned canonicalization

SHA-256 over a canonical serialization of the snapshot's node and edge sets (sorted,
attrs included), computed at report time from the store, and **prefixed with the
canonicalization version**: `"prograph-snapshot/v1+sha256:<hex>"`. Two snapshots of
identical observed structure hash identically even if their `id`/`indexed_at` differ;
any structural change flips the hash. The version prefix exists so a future change to
the serialization cannot masquerade as graph drift: a consumer comparing hashes under
different canonicalization versions knows it is comparing incomparables. This is the
report's snapshot-identity anchor for the scheduled workspace check (D7) — identity,
not freshness.

### D5. `complete`

A **producer's assertion**, not an independently verifiable proof: prograph states
that every intended element appears in `elements`, every computed finding appears in
`findings`, and no cap, sampling or truncation was applied. v1 always emits `true` —
the flag exists so any future bounded mode is forced to declare itself, and so the
consumer can fail closed on its absence (`complete != true` ⇒ instrument failure, not
a clean run). Exit-2 paths still produce no report at all.

### D6. Published contract artifacts (what steward vendors)

Two JSON Schema (draft 2020-12) files become part of the repo, under the
gate-verdicts-style contract layout:

- `contracts/intended-graph/v1/schema.json` — the authored-manifest **structural**
  schema. Owner: prograph (semantics live in `conformance/manifest.py`). steward's
  `GC-ARCH-SCHEMA` validates the manifest with a stock JSON Schema engine against its
  **byte-pinned vendored copy** — no second parser, no re-derived rules.
- `contracts/conformance-report/v1/schema.json` — the report schema incl. the D2
  provenance block. steward's `GC-ARCH-CONFORMANCE` step 1 validates the report
  against its pinned copy.

**The JSON Schema is NOT equivalent to prograph's loader, and does not claim to be.**
The loader additionally enforces cross-object integrity a JSON Schema cannot honestly
express: global id uniqueness across collections, endpoint-component existence, the
two-file-endpoint ban, exception-target resolvability, and the constraint rule
grammar (`manifest.py::_check_integrity`). The guarantee split is explicit:

> **`GC-ARCH-SCHEMA` proves structural conformance** (shape, types, required fields,
> closed enums, no unknown keys) via the pinned schema.
> **Semantic/integrity validity is proven by the successfully produced report** — a
> `conformance-report/v1` document exists only if prograph's strict loader accepted
> the manifest (exit 2 writes no report).

Sync is enforced on prograph's side by tests, not by trust — scoped to the
structural layer:

- every loader-accepted fixture manifest (monorepo_conformance, ws005_manifest, the
  unit-test VALID document) validates against `intended-graph/v1/schema.json`;
- fixtures rejected by the loader for **structural** reasons (unknown keys, missing
  required fields, bad enum values, wrong types) fail the schema for the same reason
  class — integrity-only rejections (duplicate ids, dangling refs, rule grammar) are
  expected to PASS the schema and are asserted as such, documenting the boundary;
- every `report_payload()` produced in the test suite validates against
  `conformance-report/v1/schema.json` (golden + engine unit reports).

Vendoring by consumers follows the two-guarantees rule (copy-integrity as their
PR-gate, upstream-drift as scheduled observation) — prograph's obligation is only that
the file in `contracts/` **is** the schema the code implements.

### D7. Freshness ownership split (boundary statement)

prograph makes the report *carry the facts*; it does not check freshness policy.

- **PR-gate (steward, offline):** report validates against pinned schema; manifest
  sha256 matches the checked-out manifest; `complete: true`; own repo HEAD equals
  `projects.<self>.commit` and `dirty: false`; **`snapshot.indexed_at` age within
  stage policy** (mandatory freshness dimension), report `generated_at` age as an
  optional additional bound.
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
- Report signing/attestation cryptography — out of scope for v1: the committed report
  is **reviewable evidence**, not a cryptographically protected attestation, and is
  named as such.

### D8. Injectable clock; the golden stays literally byte-exact

Report timestamps come from an injectable clock inside the code (a `now` parameter /
clock dependency threaded to `report_payload`), **not** a public test-only CLI flag,
and the production payload is never normalized. Tests inject a frozen clock, so the
golden is byte-stable under fixed inputs **and** a fixed clock — the byte-exactness
claim holds literally. (Fixture projects are not git repos, so their
`{commit: null, dirty: null}` provenance is deterministic without masking.)

## Resolved questions (owner review, 2026-08-03)

1. **v11 migration — accepted.** Check-time git capture would falsify provenance; the
   additive migration is the honest chain (D3 stands).
2. **Dirty trees at index time — warn.** `prograph index` warns on dirty tracked
   projects, immediately and machine-readably (counted in `IndexSummary.n_warnings`
   like other index warnings); indexing is not blocked — refusal stays policy.
3. **Time handling — injectable clock** (now D8). No public test-only CLI flag, no
   normalization of production payloads.

## Rollout

1. Owner review of this spec → resolve the three questions.
2. Implementation plan (house style): v11 migration + indexer git capture → report
   provenance block → contract schema files + sync tests → golden refresh.
3. steward side (their repo, after this ships): vendor both schemas pinned, implement
   `GC-ARCH-SCHEMA` / `GC-ARCH-EVIDENCE` / `GC-ARCH-CONFORMANCE` as offline consumers,
   wire stage policy — per the owner ruling recorded above.
