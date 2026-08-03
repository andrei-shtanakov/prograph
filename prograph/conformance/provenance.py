"""Report provenance (spec 2026-08-03: the conformance report as versioned evidence)."""

from __future__ import annotations

import datetime as dt
import hashlib
import json
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path

from prograph import __version__, _core

CANON_VERSION = "prograph-snapshot/v1"
TOOL_SCHEMA = "intended-graph/v1"


def _utcnow() -> dt.datetime:
    """Injectable clock seam (spec D8) — tests monkeypatch this, production uses it."""
    return dt.datetime.now(dt.UTC)


def format_ts(t: dt.datetime) -> str:
    """RFC3339 second-precision UTC, matching the store's snapshot timestamps."""
    return t.astimezone(dt.UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def snapshot_content_hash(db_path: str) -> str:
    """Versioned canonical hash of the snapshot's node+edge sets (spec D4).

    Identity anchor, not freshness: identical observed structure hashes identically
    across snapshot ids; the canon version prefix keeps hashes from different
    serializations incomparable instead of falsely drifting.
    """
    overview = _core.monorepo_overview(db_path)
    projects = (
        sorted(
            ({"kind": p.kind, "name": p.name, "slug": p.slug} for p in overview.projects),
            key=lambda p: p["name"],
        )
        if overview is not None
        else []
    )
    edges = []
    for e in _core.find_edges_filtered(db_path):
        attrs = json.loads(e.attrs_json) if e.attrs_json else {}
        edges.append(
            {
                "attrs": attrs,
                "from": e.from_name,
                "from_kind": e.from_kind,
                "kind": e.kind,
                "to": e.to_name,
                "to_kind": e.to_kind,
            }
        )
    edges.sort(key=lambda e: json.dumps(e, sort_keys=True))
    canon = json.dumps(
        {"edges": edges, "projects": projects}, sort_keys=True, separators=(",", ":")
    )
    return f"{CANON_VERSION}+sha256:{hashlib.sha256(canon.encode('utf-8')).hexdigest()}"


@dataclass(frozen=True)
class ReportProvenance:
    """Everything the D2 provenance block carries, assembled once in the CLI."""

    generated_at: str
    manifest_project: str | None
    manifest_path: str
    manifest_sha256: str
    snapshot_id: int
    snapshot_indexed_at: str
    snapshot_content_hash: str
    complete: bool
    tool_name: str
    tool_version: str
    tool_schema: str
    projects: Mapping[str, tuple[str | None, bool | None]]


def _resolve_manifest_project(
    db_path: str, monorepo_root: Path, manifest_path: Path
) -> tuple[str | None, str]:
    """(owning project, path relative to that project's root) — spec D2.

    Falls back to (None, path relative to the monorepo root or as given) when the
    manifest lies outside every indexed project root.
    """
    resolved = manifest_path.resolve()
    overview = _core.monorepo_overview(db_path)
    for p in overview.projects if overview is not None else []:
        pid = _core.project_by_name(db_path, p.name)
        desc = _core.describe_project(db_path, pid) if pid is not None else None
        if desc is None:
            continue
        root = (monorepo_root / desc.root_path.removeprefix("./")).resolve()
        try:
            return p.name, str(resolved.relative_to(root))
        except ValueError:
            continue
    try:
        return None, str(resolved.relative_to(monorepo_root.resolve()))
    except ValueError:
        return None, str(manifest_path)


def build_provenance(
    db_path: str,
    monorepo_root: Path,
    manifest_path: Path,
    manifest_projects: Sequence[str],
    *,
    now: dt.datetime | None = None,
) -> ReportProvenance:
    """Assemble the D2 provenance block for the latest snapshot."""
    snap = _core.latest_snapshot_info(db_path)
    if snap is None:
        raise ValueError(f"no snapshot in {db_path}")
    states = {
        s.project_name: (s.git_commit, s.git_dirty)
        for s in _core.project_git_states(db_path, snap.id)
    }
    project, rel_path = _resolve_manifest_project(db_path, monorepo_root, manifest_path)
    return ReportProvenance(
        generated_at=format_ts(now if now is not None else _utcnow()),
        manifest_project=project,
        manifest_path=rel_path,
        manifest_sha256=hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
        snapshot_id=snap.id,
        snapshot_indexed_at=snap.ts,
        snapshot_content_hash=snapshot_content_hash(db_path),
        complete=True,
        tool_name="prograph",
        tool_version=__version__,
        tool_schema=TOOL_SCHEMA,
        projects={name: states.get(name, (None, None)) for name in sorted(manifest_projects)},
    )
