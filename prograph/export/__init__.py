"""prograph.export — Markdown rendering for snapshot data."""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from pathlib import Path

from prograph import _core
from prograph.export.intro import extract_intro, extract_readme_body
from prograph.export.render import render_contract, render_index, render_project
from prograph.models import (
    ContractDescription,
    MonorepoOverview,
    ProjectDescription,
)
from prograph.paths import PrographPaths


def _compute_parent_map(
    projects: list[ProjectDescription],
) -> dict[str, str]:
    """For each project, find its workspace-root parent by longest-prefix match
    on `root_path`. Returns {member_slug: parent_slug}; projects without a
    parent are absent from the dict.
    """
    # Normalise: strip the "./" prefix the model uses.
    def norm(rp: str) -> str:
        return rp[2:] if rp.startswith("./") else rp

    by_root: list[tuple[str, ProjectDescription]] = [(norm(p.root_path), p) for p in projects]
    # Sort by root length DESC so the longest prefix wins (handles nested workspaces).
    by_root_sorted = sorted(by_root, key=lambda t: len(t[0]), reverse=True)

    out: dict[str, str] = {}
    for path, proj in by_root:
        for other_path, other_proj in by_root_sorted:
            if other_proj.slug == proj.slug:
                continue
            if path == other_path:
                continue
            if path.startswith(other_path + "/"):
                out[proj.slug] = other_proj.slug
                break
    return out


def _cleanup_stale_project_mds(projects_dir: Path, valid_paths: set[Path]) -> None:
    """Delete any prograph-generated MD under `projects_dir` that isn't in
    `valid_paths`. Removes flat *.md files from previous layouts, empty
    subdirs, and any MDs whose corresponding project no longer exists.

    Safety: only deletes files whose first line is the `<!-- prograph:generated -->`
    marker — never touches files a user wrote by hand.
    """
    if not projects_dir.is_dir():
        return
    for md in projects_dir.rglob("*.md"):
        if md in valid_paths:
            continue
        try:
            first = md.read_text(encoding="utf-8", errors="replace").splitlines()[:1]
        except OSError:
            continue
        if first and "<!-- prograph:generated -->" in first[0]:
            md.unlink()
    # Remove empty subdirs.
    for sub in sorted(projects_dir.glob("*"), reverse=True):
        if sub.is_dir() and not any(sub.iterdir()):
            sub.rmdir()


@dataclass(frozen=True)
class ExportReport:
    monorepo_root: Path
    n_projects: int
    n_contracts: int
    wrote_index: bool


def export_snapshot(monorepo_root: Path) -> ExportReport:
    """Render MD files from the latest snapshot in `<monorepo_root>/.prograph/graph.db`
    into `<monorepo_root>/.prograph/{projects,contracts}/*.md` + `index.md`.

    Idempotent — repeated calls produce the same bytes for the same snapshot.
    """
    paths = PrographPaths(monorepo_root=monorepo_root)
    paths.ensure_dirs()

    db_path = paths.db_path
    if not db_path.is_file():
        return ExportReport(
            monorepo_root=monorepo_root,
            n_projects=0,
            n_contracts=0,
            wrote_index=False,
        )

    overview_raw = _core.monorepo_overview(str(db_path))
    if overview_raw is None:
        return ExportReport(
            monorepo_root=monorepo_root,
            n_projects=0,
            n_contracts=0,
            wrote_index=False,
        )
    overview = MonorepoOverview.from_core(overview_raw)

    project_ids = _project_ids_in_latest_snapshot(db_path)
    contract_ids = _contract_ids_in_latest_snapshot(db_path)

    # First pass: load all ProjectDescriptions so we can compute the parent map
    # (workspace-member → workspace-root) before rendering. The parent goes into
    # frontmatter for Obsidian Graph view grouping AND drives MD output layout.
    descs: list[ProjectDescription] = []
    for pid in project_ids:
        raw = _core.describe_project(str(db_path), pid)
        if raw is None:
            continue
        descs.append(ProjectDescription.from_core(raw))

    parent_map = _compute_parent_map(descs)
    # A workspace root = any project that's a parent of another. Used to decide
    # whether to embed the README body in its MD card.
    workspace_roots = set(parent_map.values())

    # Track every MD path we write so cleanup can delete stale ones.
    written: set[Path] = set()

    n_projects = 0
    for desc in descs:
        rel = desc.root_path[2:] if desc.root_path.startswith("./") else desc.root_path
        intro = extract_intro(monorepo_root / rel)
        # Embed full README body only for workspace roots — gives the parent
        # node real context. Members keep just the one-line intro.
        readme_body = None
        if desc.slug in workspace_roots:
            readme_body = extract_readme_body(monorepo_root / rel)

        parent_slug = parent_map.get(desc.slug)
        # Look up parent's display name (not slug) for frontmatter readability.
        parent_name: str | None = None
        if parent_slug:
            parent_name = next((d.name for d in descs if d.slug == parent_slug), parent_slug)

        md = render_project(
            desc,
            intro=intro,
            parent=parent_name,
            readme_body=readme_body,
        )

        # Output path: workspace members go under projects/<parent_slug>/<slug>.md,
        # everything else stays flat at projects/<slug>.md.
        if parent_slug:
            sub = paths.projects_md_dir / parent_slug
            sub.mkdir(parents=True, exist_ok=True)
            out_path = sub / f"{desc.slug}.md"
        else:
            out_path = paths.projects_md_dir / f"{desc.slug}.md"

        out_path.write_text(md, encoding="utf-8")
        written.add(out_path)
        n_projects += 1

    _cleanup_stale_project_mds(paths.projects_md_dir, written)

    n_contracts = 0
    for cid in contract_ids:
        raw = _core.describe_contract(str(db_path), cid)
        if raw is None:
            continue
        desc = ContractDescription.from_core(raw)
        md = render_contract(desc)
        (paths.contracts_md_dir / f"{desc.slug}.md").write_text(md, encoding="utf-8")
        n_contracts += 1

    md_index = render_index(overview, parent_map=parent_map)
    paths.index_md_path.write_text(md_index, encoding="utf-8")

    return ExportReport(
        monorepo_root=monorepo_root,
        n_projects=n_projects,
        n_contracts=n_contracts,
        wrote_index=True,
    )


def _project_ids_in_latest_snapshot(db_path: Path) -> list[int]:
    """Bend of the 'Python never touches SQLite' rule: discover IDs to feed
    `_core.describe_project`. The actual data fetch goes through Rust."""
    conn = sqlite3.connect(db_path)
    try:
        rows = conn.execute(
            "SELECT id FROM projects WHERE last_seen = (SELECT MAX(id) FROM snapshots) ORDER BY id"
        ).fetchall()
        return [r[0] for r in rows]
    finally:
        conn.close()


def _contract_ids_in_latest_snapshot(db_path: Path) -> list[int]:
    conn = sqlite3.connect(db_path)
    try:
        rows = conn.execute(
            "SELECT id FROM contracts WHERE last_seen = (SELECT MAX(id) FROM snapshots) ORDER BY id"
        ).fetchall()
        return [r[0] for r in rows]
    finally:
        conn.close()
