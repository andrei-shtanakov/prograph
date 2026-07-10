# prograph — tracked-projects allowlist + discovery audit

**Date:** 2026-07-10
**Status:** Accepted (design approved in session)

## Problem

`prograph index` currently indexes *every* project discovered at the monorepo root (39 as of
snapshot 2). The monorepo contains scratch/experimental directories (`spec-runner-test`,
`spec-runner-test-vscode`, `devtools`, …) that pollute the graph. We want:

1. **Steady-state tracking of a fixed allowlist** of projects on every `prograph index`.
2. **On-demand / startup discovery** that still scans everything and reports projects that
   appeared but are not tracked (or tracked but missing), without indexing them.

## Decisions (made with the user)

- Trigger model: **flags on `index`** (`--discover`), plus a one-shot audit on `serve` startup.
- Allowlist lives in a **separate config file** `.prograph/tracked.toml` (not `config.toml`).
- New/untracked projects: **report only** — no auto-write to any config, no pending file,
  no vault write, exit code stays 0.
- **Workspace members of a tracked root are tracked automatically** (confirmed by user):
  tracking `atp-platform` implies `packages/atp-sdk`, `docs`, etc.; `arbiter` implies
  `arbiter-core/cli/mcp`; `prograph` implies `prograph-core`.

## Config file — `.prograph/tracked.toml`

```toml
# Projects indexed on every `prograph index`.
# Empty list or missing file -> ALL discovered projects are tracked (legacy behaviour).
projects = [
  "arbiter", "atp-platform", "deployer", "dispatcher", "Maestro",
  "open-prose", "proctor", "prograph", "prograph-vault",
  "robin-runtime", "robin-toolkit", "spec-runner", "spec-runner-vscode",
  "steward", "github-checker",
]
```

Semantics:

- Missing file or empty `projects` → `None` → track everything (backward compatible; all
  existing fixtures/tests unchanged).
- **Malformed TOML or non-list `projects` → hard error**: `prograph index` exits 1 with a
  message on stderr. A user who created an allowlist and broke it must not silently fall
  back to indexing everything — that reintroduces the pollution the allowlist exists to
  prevent. (Deliberately stricter than `read_export_root`, whose fail-open only affects an
  output path, not graph content.)
- Names match `ProjectCandidate.name` of **top-level** candidates exactly (case-sensitive,
  as directory names are on disk).
- A candidate is tracked iff its `root_path` equals a tracked root's path or starts with
  `<tracked root_path>/` (workspace-member inclusion).
- A name in the list that matches no discovered project is reported as `missing` by the
  audit; `index` itself only warns (counted in `n_warnings`).

## CLI surface

| Command | Behaviour |
|---|---|
| `prograph index` | Index **only** tracked roots + their workspace members. Snapshot as usual. |
| `prograph index --discover` | Same snapshot, **plus** a full `scan_monorepo` audit printed after: `untracked` (discovered, not in allowlist) and `missing` (in allowlist, not discovered). Untracked projects are NOT indexed. With `--json`, audit is embedded in the output object under `"discover"`. |

### Output discipline (audit)

- `--json`: stdout carries **only** the JSON object (IndexSummary + `"discover"` key);
  no extra text on stdout. Matches the existing CLI contract.
- Without `--json`: the audit is printed to **stderr** (`err_console`), keeping stdout
  reserved for the normal status lines.
- Audit entries are structured — `{name, root_path, kind}` — both in JSON and in the
  human-readable listing (`name` alone is ambiguous for nested workspace members).
| `prograph status` | Full scan (unchanged), each project annotated `tracked`/`untracked`; JSON gains a boolean `tracked` per project. Read-only, no snapshot. |
| `prograph serve` | Runs the audit once at startup, logs untracked/missing to stderr. No periodic re-run inside serve — periodic checks are external (cron/manual `index --discover`). |

## Component changes

### Python (`prograph/`)

- `config.py`: `read_tracked_projects(prograph_dir: Path) -> list[str] | None` — reads
  `tracked.toml`. Missing file or empty list → `None`. Malformed TOML or non-list
  `projects` → raises `TrackedConfigError` (new exception); `cli.py index` catches it,
  prints to stderr, exits 1.
- `cli.py`:
  - `index`: read allowlist, pass to `_core.index_monorepo`; add `--discover` flag that
    afterwards calls `_core.scan_monorepo` and prints the audit.
  - `status`: annotate rows with tracked flag.
  - `serve` (`web_app.py` startup or `cli.py serve`): one-shot audit log.
  - `init`: also create a commented `tracked.toml` template with `projects = []`.
- Audit set arithmetic (untracked/missing) lives in Python, but the **tracked-closure
  decision is NOT reimplemented in Python** — Python calls the same core helper Rust uses
  (see below), so the filter and the audit cannot drift apart.

### Rust (`prograph-core/`)

- New helper `discovery::tracked_closure(candidates: &[ProjectCandidate], names: &[String])
  -> Vec<bool>` — the single source of truth for "is this candidate tracked":
  1. Tracked roots = depth-1 candidates whose `name` is in the set.
  2. A candidate is tracked iff its `root_path` is a tracked root or descends from one
     (`starts_with(root + "/")`).
  Exposed via PyO3 (data-only: list of candidates + list of names → list of bools) so the
  Python audit uses the identical logic.
- `indexer::index_monorepo(monorepo_root, store, tracked: Option<Vec<String>>)` — third
  parameter. `None` → current behaviour. `Some(names)` → filter candidates through
  `tracked_closure`; allowlist names matching nothing → one warning each (into
  `IndexSummary.n_warnings`).
- `lib.rs` `py_index_monorepo` gains the optional list parameter (PyO3 `Option<Vec<String>>`,
  boundary stays data-only).
- `discovery.rs` unchanged — filtering happens after scan, in the indexer.
- Regenerate `prograph/_core.pyi` by hand for the new signature.

### Diff/change-log interaction

First `index` run with an allowlist will emit `removed` change-log entries for previously
tracked projects that are now filtered out. That is correct and desired (the graph now
reflects the tracked set); noted here so it isn't mistaken for a bug. **Add a line to the
release/migration notes** — without warning it reads as a mass project deletion.

### Known compromise: `--discover` rescans

`index --discover` runs `scan_monorepo` a second time (inside `index_monorepo`, then again
for the audit). If files change between the two scans the audit may differ slightly from
the snapshot. Accepted for a local tool; the shared `tracked_closure` helper guarantees the
*logic* is identical even when the scans are not. Revisit only if it bites in practice
(e.g. by returning audit data from `index_monorepo` itself).

## Testing

- **Rust unit (`tracked_closure`):** subset tracked; workspace members of a tracked root
  included; members of an untracked root excluded; name-collision between a nested member
  and a top-level project (only depth-1 names select roots); `None`/empty → all.
- **Rust unit (indexer):** filtering applied; unknown allowlist name → warning.
- **Python unit (config):** missing file → None; valid list → list; empty list → None;
  malformed TOML → `TrackedConfigError`; non-list `projects` → `TrackedConfigError`.
- **Integration (pytest):** fixture monorepo + `tracked.toml` → snapshot contains only the
  tracked closure; `index --discover` prints audit to stderr and clean JSON to stdout with
  `--json`; malformed `tracked.toml` → exit 1; `status` annotates correctly; no
  `tracked.toml` → legacy behaviour (existing tests green).

## Out of scope (YAGNI)

- Auto-adding discovered projects to the allowlist, pending files, vault/journal writes,
  non-zero exit codes on untracked, periodic re-scan inside `serve`, glob patterns in
  `projects` (exact names only for now).
