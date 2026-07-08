"""M8: performance baselines for hot paths. Excluded from default test runs.

Invoke with: ``uv run pytest -m bench -v``
Compare runs with: ``uv run pytest -m bench --benchmark-compare``

CI guards against >2x regression on the previous run for each benchmark.
"""

from __future__ import annotations

import shutil
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph import _core
from prograph.cli import app
from prograph.paths import PrographPaths

cli_runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_mcp"


pytestmark = pytest.mark.bench


@pytest.fixture(scope="module")
def indexed_db(tmp_path_factory) -> str:
    tmp_path = tmp_path_factory.mktemp("bench")
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(app, ["index", "--monorepo", str(dst)])
    paths = PrographPaths(monorepo_root=dst)
    return str(paths.db_path)


def test_bench_monorepo_overview(benchmark, indexed_db):
    benchmark(lambda: _core.monorepo_overview(indexed_db))


def test_bench_describe_project(benchmark, indexed_db):
    pid = _core.project_by_name(indexed_db, "py_server")
    assert pid is not None
    benchmark(lambda: _core.describe_project(indexed_db, pid))


def test_bench_find_edges(benchmark, indexed_db):
    benchmark(lambda: _core.find_edges_filtered(indexed_db, None, None, None, None))


def test_bench_search_fts(benchmark, indexed_db):
    benchmark(lambda: _core.search_fts(indexed_db, "py_server", None, 10))


def test_bench_reindex_no_changes(benchmark, tmp_path: Path):
    """Re-indexing the same state — exercises the no-change fast path."""
    dst = tmp_path / "monorepo_mcp"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    cli_runner.invoke(app, ["init", "--monorepo", str(dst)])
    cli_runner.invoke(app, ["index", "--monorepo", str(dst)])
    paths = PrographPaths(monorepo_root=dst)
    benchmark(lambda: _core.index_monorepo(str(dst), str(paths.db_path)))
