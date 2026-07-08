"""Tests for prograph.export.slug."""

from prograph.export.slug import contract_slug, slugify


def test_slugify_ascii_alphanumeric_preserved():
    assert slugify("alpha-beta_123") == "alpha-beta_123"


def test_slugify_replaces_non_safe_chars():
    assert slugify("foo/bar:baz") == "foo-bar-baz"


def test_slugify_preserves_case():
    assert slugify("Maestro") == "Maestro"


def test_slugify_empty_returns_unnamed():
    assert slugify("") == "_unnamed"


def test_slugify_unicode_replaced():
    # Cyrillic must be replaced; only ASCII alphanumeric counts.
    assert slugify("привет") == "------"


def test_contract_slug_uses_declared_id():
    assert contract_slug("obs-v1", "deadbeef" * 8) == "obs-v1"


def test_contract_slug_falls_back_to_hash():
    h = "abcdef0123456789" + "0" * 48
    assert contract_slug(None, h) == "hash-abcdef012345"


def test_contract_slug_empty_declared_falls_back():
    h = "0123456789ab" + "0" * 52
    assert contract_slug("", h) == "hash-0123456789ab"
