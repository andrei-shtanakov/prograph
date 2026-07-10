"""prograph CLI — typer entry point exposed as `prograph` console script."""

from __future__ import annotations

import json as _json
import sys
from pathlib import Path

import typer
from rich.console import Console
from rich.table import Table

from prograph import __version__, _core, core_version
from prograph.config import (
    TrackedConfigError,
    read_auto_export,
    read_export_root,
    read_tracked_projects,
)
from prograph.models import IndexSummary, ProjectCandidate, SnapshotInfo
from prograph.paths import PrographPaths

console = Console()
err_console = Console(stderr=True)

app = typer.Typer(
    name="prograph",
    help="Cross-project structure mapper for monorepos.",
    no_args_is_help=True,
    add_completion=False,
)


def _print_version(value: bool) -> None:
    if value:
        console.print(f"prograph {__version__} (core {core_version()})")
        raise typer.Exit()


@app.callback()
def main(
    version: bool = typer.Option(
        False,
        "--version",
        callback=_print_version,
        is_eager=True,
        help="Print version and exit.",
    ),
) -> None:
    """Cross-project structure mapper for monorepos."""


DEFAULT_CONFIG_TOML = """\
# prograph configuration — edit by hand. Re-running `prograph init` will not overwrite this file.

[monorepo]
# `include` / `exclude` accept glob patterns relative to the monorepo root. If `include` is empty,
# all first-level subdirs are scanned (modulo the exclude list).
include = []
exclude = ["target", "node_modules", "dist", "build", "__pycache__"]

[output]
# When true, `prograph index` automatically writes the MD export after indexing — same effect as
# passing `--export-md` to every invocation. Files land under `export_root` (see below) if set,
# otherwise .prograph/{projects,contracts}/ + .prograph/index.md.
auto_export = false
# export_root: where MD export (projects/, contracts/, index.md) is written. Relative to the
# monorepo root; the database and internal artefacts stay in .prograph/ regardless. Overridden
# by `--out-dir`. Unset means write under .prograph/.
# export_root = ".prograph/graph"   # staging for an external promoter

# Override classification or rename projects whose directory name differs from the package name.
# Example:
#   [[project]]
#   path = "./atp-platform"
#   name = "atp_platform"
#   kind = "python"
"""

DEFAULT_GITIGNORE = """\
# prograph runtime artefacts — these change every index run and should not be committed.
graph.db
graph.db-wal
graph.db-shm
index.log
index.lock

# Committed artefacts (kept under version control by default):
#   projects/*.md
#   contracts/*.md
#   index.md
#   config.toml
"""


def _resolve_monorepo(monorepo: Path | None) -> Path:
    return monorepo.resolve() if monorepo is not None else Path.cwd().resolve()


def _resolve_export_root(cli_out_dir: Path | None, config_path: Path) -> Path | None:
    """Pick the Markdown export root: CLI `--out-dir` > config `[output] export_root` > None.

    A returned value may be relative; `PrographPaths` resolves it against the
    monorepo root. None means "default to `.prograph/`".
    """
    if cli_out_dir is not None:
        return cli_out_dir
    from_config = read_export_root(config_path)
    return Path(from_config) if from_config is not None else None


def _read_tracked_or_exit(paths: PrographPaths) -> list[str] | None:
    """Read the allowlist; malformed tracked.toml is a hard error (exit 1).

    Uniform across index/status/serve — a silently-ignored broken allowlist
    would present a wrong picture (spec 2026-07-10-prograph-tracked-projects).
    """
    try:
        return read_tracked_projects(paths.prograph_dir)
    except TrackedConfigError as exc:
        err_console.print(f"[red]error:[/red] {exc}")
        raise typer.Exit(code=1) from exc


def _compute_audit(root: Path, tracked: list[str]) -> dict[str, object]:
    """Full-scan audit vs the allowlist: untracked candidates + missing names.

    Uses the same Rust helpers the indexer filters with — the audit cannot
    drift from the filter.
    """
    raw_candidates = _core.scan_monorepo(str(root))
    flags = _core.tracked_closure(raw_candidates, tracked)
    mirrors = [ProjectCandidate.from_core(c) for c in raw_candidates]
    untracked = [
        {"name": m.name, "root_path": m.root_path, "kind": m.kind.value}
        for m, keep in zip(mirrors, flags, strict=True)
        if not keep
    ]
    missing = list(_core.missing_names(raw_candidates, tracked))
    return {"untracked": untracked, "missing": missing}


def _print_audit_stderr(audit: dict[str, object]) -> None:
    untracked = audit["untracked"]
    missing = audit["missing"]
    assert isinstance(untracked, list) and isinstance(missing, list)  # narrow for pyrefly
    if untracked:
        err_console.print(f"[yellow]discover:[/yellow] {len(untracked)} untracked project(s):")
        for entry in untracked:
            err_console.print(f"  - {entry['name']} ({entry['root_path']}, {entry['kind']})")
    if missing:
        err_console.print(
            "[yellow]discover:[/yellow] allowlisted but not found: " + ", ".join(missing)
        )
    if not untracked and not missing:
        err_console.print("[green]discover:[/green] allowlist matches discovery — no drift.")


@app.command()
def init(
    monorepo: Path = typer.Option(  # noqa: B008 — standard typer DSL
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
) -> None:
    """Create the `.prograph/` skeleton under the monorepo root. Idempotent."""

    root = _resolve_monorepo(monorepo)
    if not root.is_dir():
        err_console.print(f"[red]error:[/red] monorepo root {root} is not a directory")
        raise typer.Exit(code=1)

    paths = PrographPaths(monorepo_root=root)
    paths.ensure_dirs()

    if not paths.config_path.exists():
        paths.config_path.write_text(DEFAULT_CONFIG_TOML)
    if not paths.gitignore_path.exists():
        paths.gitignore_path.write_text(DEFAULT_GITIGNORE)

    # M7: document the MCP-patterns override mechanism alongside the (empty) dir.
    patterns_readme = paths.mcp_patterns_dir / "README.md"
    if not patterns_readme.exists():
        patterns_readme.write_text(
            "# MCP detection pattern overrides\n\n"
            "Drop `python.scm` or `rust.scm` files here to extend the bundled\n"
            "tree-sitter queries used by `detectors/mcp`. They are appended to the\n"
            "built-in queries; queries are run with the same capture-name conventions\n"
            "(`tool_name`, `tool_name_literal`, `tool_use_call`, `tool_use_method`).\n"
        )

    console.print(f"[green]initialized[/green] {paths.prograph_dir}")


@app.command()
def index(
    monorepo: Path = typer.Option(  # noqa: B008 — standard typer DSL
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
    json: bool = typer.Option(
        False,
        "--json",
        help="Emit IndexSummary as JSON instead of a status line.",
    ),
    export_md: bool = typer.Option(
        False,
        "--export-md",
        help="Also write MD files after indexing.",
    ),
    out_dir: Path = typer.Option(  # noqa: B008 — standard typer DSL
        None,
        "--out-dir",
        help="Markdown export root (default: config [output] export_root, else .prograph/). "
        "Relative paths resolve against the monorepo root. The database stays in .prograph/.",
        file_okay=False,
        dir_okay=True,
    ),
    discover: bool = typer.Option(
        False,
        "--discover",
        help="After indexing, run a full scan and report untracked/missing projects "
        "(report only — untracked projects are not indexed).",
    ),
) -> None:
    """Run a full index of the monorepo: discover, parse, detect edges, diff, persist."""

    root = _resolve_monorepo(monorepo)
    paths = PrographPaths(monorepo_root=root)
    if not paths.is_initialized():
        err_console.print(
            f"[red]error:[/red] not initialized at {paths.prograph_dir}. Run `prograph init` first."
        )
        raise typer.Exit(code=1)

    export_root = _resolve_export_root(out_dir, paths.config_path)
    paths = PrographPaths(monorepo_root=root, export_root=export_root)

    tracked = _read_tracked_or_exit(paths)

    try:
        raw = _core.index_monorepo(str(root), str(paths.db_path), tracked)
    except Exception as exc:  # PrographError surfaces as PyRuntimeError / PyIOError / PyValueError
        message = str(exc).lower()
        if "lock" in message:
            err_console.print(f"[red]error:[/red] another prograph index is running ({exc})")
        else:
            err_console.print(f"[red]error:[/red] {exc}")
        raise typer.Exit(code=1) from exc

    summary = IndexSummary.from_core(raw)

    auto = read_auto_export(paths.config_path)
    if export_md or auto:
        from prograph.export import export_snapshot

        export_snapshot(root, export_root)

    audit: dict[str, object] | None = None
    if discover:
        # tracked is None -> everything is tracked; audit trivially empty.
        audit = (
            _compute_audit(root, tracked)
            if tracked is not None
            else {"untracked": [], "missing": []}
        )

    if json:
        payload = summary.model_dump(mode="json")
        if audit is not None:
            payload["discover"] = audit
        sys.stdout.write(_json.dumps(payload, indent=2) + "\n")
        return

    console.print(
        f"[green]snapshot #{summary.snapshot_id}[/green] written in "
        f"[bold]{summary.duration_ms}ms[/bold]"
    )
    console.print(
        f"  [cyan]{summary.n_projects}[/cyan] projects, "
        f"[cyan]{summary.n_edges}[/cyan] edges, "
        f"[cyan]{summary.n_changes}[/cyan] changes"
        + (f", [yellow]{summary.n_warnings}[/yellow] warnings" if summary.n_warnings else "")
    )

    if audit is not None:
        _print_audit_stderr(audit)


@app.command()
def status(
    monorepo: Path = typer.Option(  # noqa: B008 — standard typer DSL
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
    json: bool = typer.Option(False, "--json", help="Emit JSON to stdout instead of a table."),
) -> None:
    """Show monorepo state: project candidates from discovery + latest snapshot info."""

    root = _resolve_monorepo(monorepo)
    paths = PrographPaths(monorepo_root=root)
    if not paths.is_initialized():
        err_console.print(
            f"[red]error:[/red] not initialized at {paths.prograph_dir}. Run `prograph init` first."
        )
        raise typer.Exit(code=1)

    raw_candidates = _core.scan_monorepo(str(root))
    candidates = [ProjectCandidate.from_core(c) for c in raw_candidates]

    # Try to read snapshot info — opens the DB read-only via the Rust helper.
    snapshot: SnapshotInfo | None = None
    if paths.db_path.exists():
        raw_snap = _core.latest_snapshot_info(str(paths.db_path))
        snapshot = SnapshotInfo.from_core(raw_snap) if raw_snap is not None else None

    if json:
        payload = {
            "monorepo_root": str(root),
            "snapshot": snapshot.model_dump(mode="json") if snapshot else None,
            "projects": [c.model_dump(mode="json") for c in candidates],
        }
        # Bypass rich wrapping/styling: write raw JSON to stdout.
        sys.stdout.write(_json.dumps(payload, indent=2) + "\n")
        return

    table = Table(title=f"prograph status — {root}")
    table.add_column("name", style="cyan")
    table.add_column("kind", style="magenta")
    table.add_column("root", style="dim")
    table.add_column("manifests")

    for c in candidates:
        table.add_row(
            c.name,
            c.kind.value,
            c.root_path,
            ", ".join(c.manifests),
        )

    console.print(table)
    console.print(f"[dim]{len(candidates)} projects discovered.[/dim]")

    if snapshot:
        console.print(
            f"[dim]Last snapshot #{snapshot.id} at {snapshot.ts} — "
            f"{snapshot.n_projects} projects, {snapshot.n_edges} edges, "
            f"{snapshot.n_changes} changes.[/dim]"
        )
    else:
        console.print("[dim]No snapshot yet — run `prograph index` to create one.[/dim]")


@app.command("export-md")
def export_md(
    monorepo: Path = typer.Option(  # noqa: B008
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
    out_dir: Path = typer.Option(  # noqa: B008 — standard typer DSL
        None,
        "--out-dir",
        help="Markdown export root (default: config [output] export_root, else .prograph/). "
        "Relative paths resolve against the monorepo root. The database stays in .prograph/.",
        file_okay=False,
        dir_okay=True,
    ),
) -> None:
    """Render Markdown files from the latest snapshot — no reindex."""
    from prograph.export import export_snapshot

    root = _resolve_monorepo(monorepo)
    paths = PrographPaths(monorepo_root=root)
    if not paths.is_initialized():
        err_console.print(
            f"[red]error:[/red] not initialized at {paths.prograph_dir}. Run `prograph init` first."
        )
        raise typer.Exit(code=1)

    if not paths.db_path.exists():
        err_console.print("[red]error:[/red] no snapshot to export. Run `prograph index` first.")
        raise typer.Exit(code=1)

    export_root = _resolve_export_root(out_dir, paths.config_path)
    report = export_snapshot(root, export_root)
    console.print(
        f"[green]exported[/green] {report.n_projects} projects, "
        f"{report.n_contracts} contracts, "
        f"index{'.md' if report.wrote_index else ' skipped'}"
    )


@app.command()
def mcp(
    monorepo: Path = typer.Option(  # noqa: B008
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
) -> None:
    """Run the MCP stdio server. Communicates with the AI client via stdin/stdout."""
    from prograph.mcp_server import main as mcp_main

    root = _resolve_monorepo(monorepo)
    paths = PrographPaths(monorepo_root=root)
    if not paths.is_initialized() or not paths.db_path.exists():
        err_console.print(
            f"[red]error:[/red] no snapshot at {paths.db_path}. "
            "Run `prograph init && prograph index` first."
        )
        raise typer.Exit(code=1)

    mcp_main(root)


@app.command()
def drift(
    monorepo: Path = typer.Option(  # noqa: B008
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
    kind: str = typer.Option(
        None,
        "--kind",
        help="Filter: missing | extra | stale_todo",
    ),
    json_out: bool = typer.Option(
        False,
        "--json",
        help="Emit JSON instead of formatted output.",
    ),
) -> None:
    """Print drift findings from the latest snapshot."""
    from prograph.models import DriftFinding

    paths = PrographPaths(monorepo_root=_resolve_monorepo(monorepo))
    if not paths.db_path.exists():
        err_console.print("[red]error:[/red] no graph.db found — run `prograph index` first.")
        raise typer.Exit(code=1)

    rows = _core.find_drifts_filtered(str(paths.db_path), kind)
    findings = [DriftFinding.from_core(r) for r in rows]

    if json_out:
        sys.stdout.write(
            _json.dumps([f.model_dump(mode="json") for f in findings], indent=2) + "\n"
        )
        return

    if not findings:
        console.print("No drift findings.")
        return

    from collections import defaultdict

    by_project: dict[str, list[DriftFinding]] = defaultdict(list)
    for f in findings:
        by_project[f.project_name].append(f)

    for project in sorted(by_project):
        console.print(f"\n## {project}")
        by_kind: dict[str, list[DriftFinding]] = defaultdict(list)
        for f in by_project[project]:
            by_kind[f.kind].append(f)
        for k in ("missing", "extra", "stale_todo"):
            if not by_kind.get(k):
                continue
            console.print(f"  [{k}]")
            for f in by_kind[k]:
                conf = " (low)" if f.confidence == "low" else ""
                console.print(
                    f"    - {f.entity_name} ({f.entity_kind}) "
                    f"— {f.source_path}:{f.source_line}{conf}"
                )


@app.command()
def serve(
    monorepo: Path = typer.Option(  # noqa: B008
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
    host: str = typer.Option(
        "127.0.0.1",
        "--host",
        help="Bind address. Use 0.0.0.0 to expose on all interfaces (warning printed).",
    ),
    port: int = typer.Option(7700, "--port", help="Bind port."),
) -> None:
    """Start the local web UI + REST API at http://<host>:<port>."""
    import uvicorn

    from prograph.web_app import build_app

    root = _resolve_monorepo(monorepo)
    paths = PrographPaths(monorepo_root=root)
    if not paths.is_initialized():
        err_console.print(
            f"[red]error:[/red] not initialized at {paths.prograph_dir}. Run `prograph init` first."
        )
        raise typer.Exit(code=1)
    if not paths.db_path.exists():
        err_console.print(
            f"[red]error:[/red] no snapshot at {paths.db_path}. Run `prograph index` first."
        )
        raise typer.Exit(code=1)

    if host == "0.0.0.0":
        err_console.print(
            "[yellow]warning:[/yellow] binding to 0.0.0.0 exposes the API on all "
            "network interfaces with NO authentication. Use only on trusted networks."
        )

    console.print(f"[green]prograph serve[/green] at http://{host}:{port} (monorepo: {root})")

    app_instance = build_app(root)
    uvicorn.run(app_instance, host=host, port=port, log_level="info")


if __name__ == "__main__":
    sys.exit(app())
