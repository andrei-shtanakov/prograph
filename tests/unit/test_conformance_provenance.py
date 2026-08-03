"""Provenance assembly: content hash canon, clock formatting, dataclass shape."""

import datetime as dt
import shutil
from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app
from prograph.conformance.provenance import (
    CANON_VERSION,
    format_ts,
    snapshot_content_hash,
)

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_minimal"


def test_format_ts_utc_second_precision() -> None:
    t = dt.datetime(2026, 8, 3, 12, 0, 7, 123456, tzinfo=dt.UTC)
    assert format_ts(t) == "2026-08-03T12:00:07Z"


def _indexed(tmp_path: Path) -> str:
    dst = tmp_path / "mono"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    assert runner.invoke(app, ["init", "--monorepo", str(dst)]).exit_code == 0
    assert runner.invoke(app, ["index", "--monorepo", str(dst)]).exit_code == 0
    return str(dst / ".prograph" / "graph.db")


def test_content_hash_versioned_and_deterministic(tmp_path: Path) -> None:
    db = _indexed(tmp_path)
    h1 = snapshot_content_hash(db)
    h2 = snapshot_content_hash(db)
    assert h1 == h2
    assert h1.startswith(f"{CANON_VERSION}+sha256:")
    assert len(h1.split(":", 1)[1]) == 64


def test_content_hash_same_content_different_snapshot_ids(tmp_path: Path) -> None:
    db = _indexed(tmp_path)
    h1 = snapshot_content_hash(db)
    # Re-index the unchanged tree: new snapshot id, identical structure -> same hash.
    mono = str(Path(db).parent.parent)
    assert runner.invoke(app, ["index", "--monorepo", mono]).exit_code == 0
    assert snapshot_content_hash(db) == h1
