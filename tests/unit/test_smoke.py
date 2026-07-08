"""Smoke test: PyO3 extension builds and imports."""

import prograph


def test_python_package_version():
    assert prograph.__version__ == "0.1.0"


def test_rust_core_version_matches():
    assert prograph.core_version() == "0.1.0"
