"""Integration test: scan_monorepo against the bundled fixture."""

from prograph._core import scan_monorepo


def test_scan_monorepo_minimal_fixture(fixtures_dir):
    root = fixtures_dir / "monorepo_minimal"
    candidates = scan_monorepo(str(root))

    names = sorted(c.name for c in candidates)
    assert names == ["proj_a", "proj_b"]

    for c in candidates:
        assert c.kind.name() == "python"
        assert "pyproject.toml" in c.manifests
        assert c.root_path == f"./{c.name}"


def test_scan_monorepo_errors_on_missing_root():
    import pytest

    with pytest.raises(Exception) as exc:
        scan_monorepo("/path/does/not/exist/prograph-test")
    assert "not a directory" in str(exc.value).lower()
