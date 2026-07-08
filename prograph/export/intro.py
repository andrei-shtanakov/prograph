"""Extract a one-line intro from a project's README/CLAUDE/TODO."""

from __future__ import annotations

import re
from pathlib import Path

_PROBES = ("README.md", "CLAUDE.md", "TODO.md")
_MAX_LEN = 200


def extract_intro(project_root: Path) -> str | None:
    """Return a one-line intro for `project_root`, or None if no probe file usable.

    Algorithm:
    1. For each probe file in order, read up to ~4KB from the start.
    2. Skip leading blank lines and the first `# Heading` line if any.
    3. Take the next non-blank paragraph.
    4. Strip Markdown emphasis markers (`*`, `_`, `**`); collapse whitespace to single spaces.
    5. Truncate at `_MAX_LEN` characters (split on word boundary if possible).
    """
    for probe in _PROBES:
        path = project_root / probe
        if not path.is_file():
            continue
        try:
            raw = path.read_text(encoding="utf-8")[:4096]
        except OSError:
            continue

        intro = _extract_from_text(raw)
        if intro:
            return intro
    return None


def _extract_from_text(raw: str) -> str | None:
    lines = raw.splitlines()
    i = 0
    while i < len(lines) and not lines[i].strip():
        i += 1
    if i < len(lines) and lines[i].startswith("#"):
        i += 1
    while i < len(lines) and not lines[i].strip():
        i += 1
    paragraph_lines: list[str] = []
    while i < len(lines) and lines[i].strip():
        paragraph_lines.append(lines[i].strip())
        i += 1
    if not paragraph_lines:
        return None

    text = " ".join(paragraph_lines)
    text = re.sub(r"\*\*|__|\*|_", "", text)
    text = re.sub(r"\s+", " ", text).strip()
    if not text:
        return None
    return _truncate(text)


def _truncate(text: str) -> str:
    if len(text) <= _MAX_LEN:
        return text
    cut = text[:_MAX_LEN]
    last_space = cut.rfind(" ")
    if last_space > _MAX_LEN - 40:
        cut = cut[:last_space]
    return cut.rstrip(",.;:") + "…"


_BODY_MAX_CHARS = 2000


def extract_readme_body(project_root: Path) -> str | None:
    """Return the body of a project's README.md — content AFTER the H1 title,
    truncated to ~2000 chars at a heading or paragraph boundary. Used for
    workspace-root project cards so the Obsidian view of "atp-platform" shows
    the actual README, not just a one-line blockquote.

    Returns None if no README.md, or the file is empty after the title.
    """
    readme = project_root / "README.md"
    if not readme.is_file():
        return None
    try:
        raw = readme.read_text(encoding="utf-8")
    except OSError:
        return None

    lines = raw.splitlines()
    # Skip leading blanks.
    i = 0
    while i < len(lines) and not lines[i].strip():
        i += 1
    # Skip the H1 (or first heading of any level on line i).
    if i < len(lines) and lines[i].lstrip().startswith("#"):
        i += 1
    # Skip blanks after the title.
    while i < len(lines) and not lines[i].strip():
        i += 1

    if i >= len(lines):
        return None

    body = "\n".join(lines[i:]).strip()
    if not body:
        return None

    if len(body) <= _BODY_MAX_CHARS:
        return body

    # Truncate at a heading or paragraph boundary near the limit.
    cut = body[:_BODY_MAX_CHARS]
    # Prefer cutting before a heading.
    last_heading = cut.rfind("\n##")
    if last_heading > _BODY_MAX_CHARS - 600:
        return cut[:last_heading].rstrip() + "\n\n_(README truncated — see the full file)_"
    # Else cut at a paragraph break.
    last_para = cut.rfind("\n\n")
    if last_para > _BODY_MAX_CHARS - 600:
        return cut[:last_para].rstrip() + "\n\n_(README truncated — see the full file)_"
    # Fallback: just truncate.
    return cut.rstrip() + "…"
