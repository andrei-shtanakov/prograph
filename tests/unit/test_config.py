"""Tests for prograph.config — reading .prograph/config.toml settings."""

from pathlib import Path

from prograph.config import read_auto_export, read_export_root


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
