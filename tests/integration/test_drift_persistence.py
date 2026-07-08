"""M11: drift_findings persists across snapshots; first_seen stays stable."""

import shutil
import sqlite3
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph import _core
from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_drift"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "md"
    shutil.copytree(FIXTURE, dst)
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def test_declarer_missing_public_symbol(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    drifts = _core.drifts_for_project(db, "declarer")
    missing_symbols = [
        d for d in drifts if d.kind == "missing" and d.entity_kind == "public_symbol"
    ]
    names = {d.entity_name for d in missing_symbols}
    assert "Declared" in names
    assert "Implemented" not in names


def test_declarer_missing_mcp_tool(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    drifts = _core.drifts_for_project(db, "declarer")
    missing_tools = [d for d in drifts if d.kind == "missing" and d.entity_kind == "mcp_tool"]
    names = {d.entity_name for d in missing_tools}
    assert "tool_phantom" in names


def test_declarer_extra_public_symbol(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    drifts = _core.drifts_for_project(db, "declarer")
    extras = [d for d in drifts if d.kind == "extra" and d.entity_kind == "public_symbol"]
    names = {d.entity_name for d in extras}
    assert "undocumented_extra_fn" in names


def test_cleaner_has_no_drift(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    drifts = _core.drifts_for_project(db, "cleaner")
    assert not drifts


def test_nointent_skipped_for_extra(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    drifts = _core.drifts_for_project(db, "nointent")
    assert not [d for d in drifts if d.kind == "extra"]


def test_drift_first_seen_stable_across_reindex(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)

    conn = sqlite3.connect(db)
    try:
        before = dict(
            conn.execute(
                """SELECT df.entity_name, df.first_seen
               FROM drift_findings df
               JOIN projects p ON p.id = df.project_id
               WHERE p.name = 'declarer'
            """
            ).fetchall()
        )
    finally:
        conn.close()
    assert before

    runner.invoke(app, ["index", "--monorepo", str(indexed)])

    conn = sqlite3.connect(db)
    try:
        after = dict(
            conn.execute(
                """SELECT df.entity_name, df.first_seen
               FROM drift_findings df
               JOIN projects p ON p.id = df.project_id
               WHERE p.name = 'declarer'
                 AND df.last_seen = (SELECT MAX(id) FROM snapshots)
            """
            ).fetchall()
        )
    finally:
        conn.close()
    for name, fs in before.items():
        if name in after:
            assert after[name] == fs, f"first_seen advanced for {name}: {fs} -> {after[name]}"


def test_drift_findings_filtered_by_kind(indexed: Path):
    db = str(PrographPaths(monorepo_root=indexed).db_path)
    only_missing = _core.find_drifts_filtered(db, "missing")
    assert only_missing
    assert all(d.kind == "missing" for d in only_missing)
