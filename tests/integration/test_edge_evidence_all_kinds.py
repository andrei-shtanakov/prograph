"""M8: edge_evidence persisted for ALL edge kinds (package_dep, mcp_call, contract_link)."""

import shutil
import sqlite3
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

cli_runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_mcp"


@pytest.fixture
def indexed(tmp_path: Path) -> Path:
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(app, ["index", "--monorepo", str(dst)])
    return dst


def _evidence_for_kind(db: Path, kind: str) -> list[tuple]:
    conn = sqlite3.connect(db)
    try:
        return conn.execute(
            """
            SELECT ev.edge_id, ev.rel_path, ev.line
            FROM edge_evidence ev
            JOIN edges e ON e.id = ev.edge_id
            WHERE e.kind = ? AND ev.last_seen = (SELECT MAX(id) FROM snapshots)
            """,
            (kind,),
        ).fetchall()
    finally:
        conn.close()


def test_evidence_persisted_in_monorepo_full(tmp_path: Path):
    """monorepo_full has package_dep edges; each should now have evidence."""
    fixture = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_full"
    dst = tmp_path / "monorepo_full"
    shutil.copytree(fixture, dst, ignore=shutil.ignore_patterns("golden"))
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(app, ["index", "--monorepo", str(dst)])

    db = dst / ".prograph" / "graph.db"
    evidence = _evidence_for_kind(db, "package_dep")
    assert len(evidence) >= 1, f"expected ≥1 package_dep evidence rows, got {evidence}"
    for _eid, rel_path, line in evidence:
        assert rel_path == "pyproject.toml"
        assert line == 1


def test_evidence_persisted_for_mcp_call(indexed: Path):
    """monorepo_mcp has mcp_call edges (existing M7 behaviour) — should remain."""
    db = indexed / ".prograph" / "graph.db"
    evidence = _evidence_for_kind(db, "mcp_call")
    assert len(evidence) >= 1, f"expected ≥1 mcp_call evidence rows, got {len(evidence)}"


def test_evidence_persisted_for_contract_link(indexed: Path):
    """monorepo_mcp has contract_link edges; evidence should exist with the
    consumer's contract file rel_path."""
    db = indexed / ".prograph" / "graph.db"
    evidence = _evidence_for_kind(db, "contract_link")
    assert len(evidence) >= 1, f"expected ≥1 contract_link evidence rows, got {evidence}"
    paths = {e[1] for e in evidence}
    assert any(p.endswith(".json") for p in paths), (
        f"expected at least one .json contract file path in {paths}"
    )
