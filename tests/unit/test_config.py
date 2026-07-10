"""Tests for prograph.config — reading .prograph/config.toml settings."""

from pathlib import Path

import pytest

from prograph.config import (
    TrackedConfigError,
    read_auto_export,
    read_export_root,
    read_tracked_projects,
)


def _write(config_path: Path, text: str) -> Path:
    config_path.write_text(text, encoding="utf-8")
    return config_path


def test_read_export_root_returns_value(tmp_path: Path):
    cfg = _write(
        tmp_path / "config.toml",
        '[output]\nexport_root = ".prograph/graph"\n',
    )
    assert read_export_root(cfg) == ".prograph/graph"


def test_read_export_root_missing_key_is_none(tmp_path: Path):
    cfg = _write(tmp_path / "config.toml", "[output]\nauto_export = true\n")
    assert read_export_root(cfg) is None


def test_read_export_root_missing_section_is_none(tmp_path: Path):
    cfg = _write(tmp_path / "config.toml", "[monorepo]\ninclude = []\n")
    assert read_export_root(cfg) is None


def test_read_export_root_missing_file_is_none(tmp_path: Path):
    assert read_export_root(tmp_path / "does_not_exist.toml") is None


def test_read_export_root_broken_toml_is_none(tmp_path: Path):
    cfg = _write(tmp_path / "config.toml", "[output\nexport_root = oops")
    assert read_export_root(cfg) is None


def test_read_export_root_non_string_is_none(tmp_path: Path):
    cfg = _write(tmp_path / "config.toml", "[output]\nexport_root = 42\n")
    assert read_export_root(cfg) is None


def test_auto_export_still_works(tmp_path: Path):
    """Sanity: the existing reader is unaffected."""
    cfg = _write(tmp_path / "config.toml", "[output]\nauto_export = true\n")
    assert read_auto_export(cfg) is True


def test_read_tracked_missing_file_is_none(tmp_path: Path) -> None:
    assert read_tracked_projects(tmp_path) is None


def test_read_tracked_valid_list(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text('projects = ["a", "b"]\n')
    assert read_tracked_projects(tmp_path) == ["a", "b"]


def test_read_tracked_empty_list_is_none(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text("projects = []\n")
    assert read_tracked_projects(tmp_path) is None


def test_read_tracked_missing_key_is_none(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text("# nothing here\n")
    assert read_tracked_projects(tmp_path) is None


def test_read_tracked_malformed_toml_raises(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text("projects = [broken\n")
    with pytest.raises(TrackedConfigError):
        read_tracked_projects(tmp_path)


def test_read_tracked_non_list_raises(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text('projects = "a"\n')
    with pytest.raises(TrackedConfigError):
        read_tracked_projects(tmp_path)


def test_read_tracked_non_string_items_raise(tmp_path: Path) -> None:
    (tmp_path / "tracked.toml").write_text("projects = [1, 2]\n")
    with pytest.raises(TrackedConfigError):
        read_tracked_projects(tmp_path)


def test_paths_tracked_path() -> None:
    from prograph.paths import PrographPaths

    p = PrographPaths(monorepo_root=Path("/x"))
    assert p.tracked_path == Path("/x/.prograph/tracked.toml")
