"""v11: per-project git provenance captured at index time."""

import shutil
import subprocess
from pathlib import Path

from typer.testing import CliRunner

from prograph import _core
from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_minimal"


def _git(cwd: Path, *args: str) -> None:
    subprocess.run(
        [
            "git",
            "-C",
            str(cwd),
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "-c",
            "commit.gpgsign=false",
            *args,
        ],
        check=True,
        capture_output=True,
    )


def test_git_state_captured_at_index_time(tmp_path: Path) -> None:
    dst = tmp_path / "mono"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    projects = sorted(p.name for p in dst.iterdir() if (p / "pyproject.toml").is_file())
    assert projects, "fixture must contain at least one python project"
    repo = dst / projects[0]
    _git(repo, "init", "-q")
    _git(repo, "add", ".")
    _git(repo, "commit", "-q", "-m", "init")

    assert runner.invoke(app, ["init", "--monorepo", str(dst)]).exit_code == 0
    assert runner.invoke(app, ["index", "--monorepo", str(dst)]).exit_code == 0
    db = str(dst / ".prograph" / "graph.db")
    snap = _core.latest_snapshot_info(db)
    assert snap is not None
    states = {s.project_name: s for s in _core.project_git_states(db, snap.id)}

    git_proj = states[projects[0]]
    assert git_proj.git_commit is not None and git_proj.git_dirty is False
    for name, st in states.items():
        if name != projects[0]:
            assert st.git_commit is None and st.git_dirty is None

    # Dirty the repo, reindex: dirty flag flips, commit stays recorded, warning counted.
    (repo / "dirty.txt").write_text("x", encoding="utf-8")
    before = _core.latest_snapshot_info(db)
    assert runner.invoke(app, ["index", "--monorepo", str(dst)]).exit_code == 0
    snap2 = _core.latest_snapshot_info(db)
    assert snap2 is not None and before is not None and snap2.id > before.id
    st2 = {s.project_name: s for s in _core.project_git_states(db, snap2.id)}[projects[0]]
    assert st2.git_dirty is True
    assert st2.git_commit == git_proj.git_commit
