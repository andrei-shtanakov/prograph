"""Filename slugification — mirror of Rust's slugify for filename + wiki-link generation."""

from __future__ import annotations


def slugify(s: str) -> str:
    """Replace any character that isn't ASCII alphanumeric, dash, or underscore with `-`.

    Preserves case. Empty input returns "_unnamed".
    """
    if not s:
        return "_unnamed"
    return "".join(c if (c.isalnum() and c.isascii()) or c in "-_" else "-" for c in s)


def contract_slug(declared_id: str | None, content_hash: str) -> str:
    """Slug for a contract — declared_id if present, else first 12 chars of content_hash."""
    if declared_id:
        return slugify(declared_id)
    return f"hash-{content_hash[:12]}"
