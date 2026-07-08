"""Tests for prograph.export.intro."""

from pathlib import Path

from prograph.export.intro import extract_intro


def test_intro_from_readme(tmp_path: Path):
    (tmp_path / "README.md").write_text(
        "# My Project\n\nA tool that does X and Y.\n\nMore details below.\n"
    )
    assert extract_intro(tmp_path) == "A tool that does X and Y."


def test_intro_prefers_readme_over_claude(tmp_path: Path):
    (tmp_path / "README.md").write_text("# X\n\nReadme says hi.\n")
    (tmp_path / "CLAUDE.md").write_text("# X\n\nClaude says hi.\n")
    assert extract_intro(tmp_path) == "Readme says hi."


def test_intro_falls_back_to_claude(tmp_path: Path):
    (tmp_path / "CLAUDE.md").write_text("# X\n\nClaude only.\n")
    assert extract_intro(tmp_path) == "Claude only."


def test_intro_falls_back_to_todo(tmp_path: Path):
    (tmp_path / "TODO.md").write_text("# X\n\nLast resort.\n")
    assert extract_intro(tmp_path) == "Last resort."


def test_intro_returns_none_when_no_probe(tmp_path: Path):
    assert extract_intro(tmp_path) is None


def test_intro_strips_markdown_emphasis(tmp_path: Path):
    (tmp_path / "README.md").write_text("# X\n\n**Bold** and *italic* text here.\n")
    assert extract_intro(tmp_path) == "Bold and italic text here."


def test_intro_collapses_multiline_paragraph(tmp_path: Path):
    (tmp_path / "README.md").write_text("# X\n\nLine one\nline two\nline three.\n\nNext para.\n")
    assert extract_intro(tmp_path) == "Line one line two line three."


def test_intro_truncates_long_text(tmp_path: Path):
    text = "Word " * 100  # > 400 chars
    (tmp_path / "README.md").write_text(f"# X\n\n{text}\n")
    intro = extract_intro(tmp_path)
    assert intro is not None
    assert len(intro) <= 201  # _MAX_LEN + ellipsis room
    assert intro.endswith("…")


def test_intro_skips_blank_after_heading(tmp_path: Path):
    (tmp_path / "README.md").write_text("# X\n\n\n\nFirst real paragraph.\n")
    assert extract_intro(tmp_path) == "First real paragraph."


def test_intro_handles_no_heading(tmp_path: Path):
    (tmp_path / "README.md").write_text("Plain text first paragraph.\n")
    assert extract_intro(tmp_path) == "Plain text first paragraph."
