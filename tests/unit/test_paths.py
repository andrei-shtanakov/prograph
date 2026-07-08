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
