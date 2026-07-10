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

- Missing file, empty `projects`, or malformed TOML → `None` → track everything
  (backward compatible; all existing fixtures/tests unchanged).
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
| `prograph status` | Full scan (unchanged), each project annotated `tracked`/`untracked`; JSON gains a boolean `tracked` per project. Read-only, no snapshot. |
| `prograph serve` | Runs the audit once at startup, logs untracked/missing to stderr. No periodic re-run inside serve — periodic checks are external (cron/manual `index --discover`). |

## Component changes

### Python (`prograph/`)

- `config.py`: `read_tracked_projects(prograph_dir: Path) -> list[str] | None` — reads
  `tracked.toml`, tolerant of missing file / bad TOML / non-list values (returns `None`),
  mirroring `read_export_root`'s style. Empty list → `None`.
- `cli.py`:
  - `index`: read allowlist, pass to `_core.index_monorepo`; add `--discover` flag that
    afterwards calls `_core.scan_monorepo` and prints the audit.
  - `status`: annotate rows with tracked flag.
  - `serve` (`web_app.py` startup or `cli.py serve`): one-shot audit log.
  - `init`: also create a commented `tracked.toml` template with `projects = []`.
- Audit computation (untracked/missing set arithmetic) lives in Python — Rust knows nothing
  about the audit.

### Rust (`prograph-core/`)

- `indexer::index_monorepo(monorepo_root, store, tracked: Option<Vec<String>>)` — third
  parameter. `None` → current behaviour. `Some(names)`:
  1. From `scan_monorepo` candidates, tracked roots = depth-1 candidates whose `name` is in
     the set.
  2. Keep candidates whose `root_path` is a tracked root or descends from one
     (`starts_with(root + "/")`).
  3. Allowlist names matching nothing → one warning each (into `IndexSummary.n_warnings`).
- `lib.rs` `py_index_monorepo` gains the optional list parameter (PyO3 `Option<Vec<String>>`,
  boundary stays data-only).
- `discovery.rs` unchanged — filtering happens after scan, in the indexer.
- Regenerate `prograph/_core.pyi` by hand for the new signature.

### Diff/change-log interaction

First `index` run with an allowlist will emit `removed` change-log entries for previously
tracked projects that are now filtered out. That is correct and desired (the graph now
reflects the tracked set); noted here so it isn't mistaken for a bug.

## Testing

- **Rust unit (indexer):** subset tracked; workspace members of a tracked root included;
  members of an untracked root excluded; unknown allowlist name → warning; `None` → all.
- **Python unit (config):** missing file → None; valid list → list; empty list → None;
  malformed TOML → None; non-list `projects` → None.
- **Integration (pytest):** fixture monorepo + `tracked.toml` → snapshot contains only the
  tracked closure; `index --discover` prints expected untracked/missing (text and `--json`);
  `status` annotates correctly; no `tracked.toml` → legacy behaviour (existing tests green).

## Out of scope (YAGNI)

- Auto-adding discovered projects to the allowlist, pending files, vault/journal writes,
  non-zero exit codes on untracked, periodic re-scan inside `serve`, glob patterns in
  `projects` (exact names only for now).
