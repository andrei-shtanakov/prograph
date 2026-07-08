"""Tests for the assert_md_dir_matches_golden helper in conftest."""

from pathlib import Path

import pytest


def test_golden_helper_passes_on_identical(tmp_path: Path):
    from tests.conftest import assert_md_dir_matches_golden

    produced = tmp_path / "produced"
    golden = tmp_path / "golden"
    produced.mkdir()
    golden.mkdir()
    (produced / "a.md").write_text("hello\n")
    (golden / "a.md").write_text("hello\n")

    assert_md_dir_matches_golden(produced, golden)


def test_golden_helper_raises_on_diff(tmp_path: Path):
    from tests.conftest import assert_md_dir_matches_golden

    produced = tmp_path / "produced"
    golden = tmp_path / "golden"
    produced.mkdir()
    golden.mkdir()
    (produced / "a.md").write_text("hello\n")
    (golden / "a.md").write_text("WORLD\n")

    with pytest.raises(AssertionError, match="differs from golden"):
        assert_md_dir_matches_golden(produced, golden)


def test_golden_helper_raises_on_missing_file(tmp_path: Path):
    from tests.conftest import assert_md_dir_matches_golden

    produced = tmp_path / "produced"
    golden = tmp_path / "golden"
    produced.mkdir()
    golden.mkdir()
    (produced / "a.md").write_text("hi\n")

    with pytest.raises(AssertionError, match="lists differ"):
        assert_md_dir_matches_golden(produced, golden)


def test_golden_helper_normalizes_timestamp(tmp_path: Path):
    from tests.conftest import assert_md_dir_matches_golden

    produced = tmp_path / "produced"
    golden = tmp_path / "golden"
    produced.mkdir()
    golden.mkdir()
    (produced / "a.md").write_text("indexed_at: 2026-05-26T12:34:56Z\nbody\n")
    (golden / "a.md").write_text("indexed_at: 2026-05-26T00:00:00Z\nbody\n")

    # Both timestamps normalize to <ts>, so this must pass.
    assert_md_dir_matches_golden(produced, golden)
