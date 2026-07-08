"""Tests for prograph.paths — .prograph/ layout constants."""

from pathlib import Path

from prograph.paths import PrographPaths


def test_default_paths_under_monorepo_root(tmp_path: Path):
    p = PrographPaths(monorepo_root=tmp_path)
    assert p.prograph_dir == tmp_path / ".prograph"
    assert p.config_path == tmp_path / ".prograph" / "config.toml"
    assert p.db_path == tmp_path / ".prograph" / "graph.db"
    assert p.lock_path == tmp_path / ".prograph" / "index.lock"
    assert p.log_path == tmp_path / ".prograph" / "index.log"
    assert p.projects_md_dir == tmp_path / ".prograph" / "projects"
    assert p.contracts_md_dir == tmp_path / ".prograph" / "contracts"
    assert p.gitignore_path == tmp_path / ".prograph" / ".gitignore"


def test_ensure_dirs_creates_missing(tmp_path: Path):
    p = PrographPaths(monorepo_root=tmp_path)
    assert not p.prograph_dir.exists()
    p.ensure_dirs()
    assert p.prograph_dir.is_dir()
    assert p.projects_md_dir.is_dir()
    assert p.contracts_md_dir.is_dir()


def test_initialized_false_when_no_prograph_dir(tmp_path: Path):
    p = PrographPaths(monorepo_root=tmp_path)
    assert not p.is_initialized()


def test_initialized_true_after_ensure_dirs_and_config(tmp_path: Path):
    p = PrographPaths(monorepo_root=tmp_path)
    p.ensure_dirs()
    p.config_path.write_text("# config\n")
    assert p.is_initialized()


# ----- export_root: configurable Markdown staging directory -----


def test_export_root_none_identical_to_default(tmp_path: Path):
    """With export_root=None every path is byte-identical to the no-arg default."""
    default = PrographPaths(monorepo_root=tmp_path)
    explicit = PrographPaths(monorepo_root=tmp_path, export_root=None)
    for attr in (
        "prograph_dir",
        "config_path",
        "db_path",
        "lock_path",
        "log_path",
        "projects_md_dir",
        "contracts_md_dir",
        "gitignore_path",
        "index_md_path",
        "mcp_patterns_dir",
    ):
        assert getattr(explicit, attr) == getattr(default, attr)


def test_relative_export_root_resolves_from_monorepo_root(tmp_path: Path):
    p = PrographPaths(monorepo_root=tmp_path, export_root=Path("stage"))
    assert p.projects_md_dir == tmp_path / "stage" / "projects"
    assert p.contracts_md_dir == tmp_path / "stage" / "contracts"
    assert p.index_md_path == tmp_path / "stage" / "index.md"


def test_export_root_does_not_move_db_or_internals(tmp_path: Path):
    p = PrographPaths(monorepo_root=tmp_path, export_root=Path("stage"))
    assert p.db_path == tmp_path / ".prograph" / "graph.db"
    assert p.config_path == tmp_path / ".prograph" / "config.toml"
    assert p.lock_path == tmp_path / ".prograph" / "index.lock"
    assert p.log_path == tmp_path / ".prograph" / "index.log"
    assert p.mcp_patterns_dir == tmp_path / ".prograph" / "mcp_patterns"
    assert p.gitignore_path == tmp_path / ".prograph" / ".gitignore"


def test_absolute_export_root_used_verbatim(tmp_path: Path):
    abs_dir = tmp_path / "elsewhere" / "graph"
    p = PrographPaths(monorepo_root=tmp_path, export_root=abs_dir)
    assert p.projects_md_dir == abs_dir / "projects"
    assert p.index_md_path == abs_dir / "index.md"


def test_ensure_dirs_creates_export_root_md_dirs(tmp_path: Path):
    p = PrographPaths(monorepo_root=tmp_path, export_root=Path("stage"))
    p.ensure_dirs()
    assert p.prograph_dir.is_dir()
    assert p.mcp_patterns_dir.is_dir()
    assert p.projects_md_dir.is_dir()
    assert p.contracts_md_dir.is_dir()
    assert (tmp_path / "stage" / "projects").is_dir()
