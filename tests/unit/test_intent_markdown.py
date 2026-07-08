"""M11: smoke that the Rust intent parser is reachable from Python via ProjectDescription."""

import shutil
from pathlib import Path

from typer.testing import CliRunner

from prograph import _core
from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_drift"


def test_intent_extracted_visible_via_describe_project(tmp_path: Path):
    dst = tmp_path / "md"
    shutil.copytree(FIXTURE, dst)
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    runner.invoke(app, ["index", "--monorepo", str(dst)])

    db = str(PrographPaths(monorepo_root=dst).db_path)
    pid = _core.project_by_name(db, "declarer")
    assert pid is not None
    desc = _core.describe_project(db, pid)
    assert desc is not None
    # declarer has multiple drift findings (Declared missing, tool_phantom missing,
    # undocumented_extra_fn extra). At least one of each kind.
    assert any(d.kind == "missing" for d in desc.drifts)
    assert any(d.kind == "extra" for d in desc.drifts)
