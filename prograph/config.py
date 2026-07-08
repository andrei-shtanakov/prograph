"""Read .prograph/config.toml settings."""

from __future__ import annotations

from pathlib import Path

try:
    import tomllib  # Python 3.11+
except ImportError:
    import tomli as tomllib  # type: ignore[no-redef]


def read_auto_export(config_path: Path) -> bool:
    """Return True if `.prograph/config.toml` sets `[output] auto_export = true`."""
    if not config_path.is_file():
        return False
    try:
        data = tomllib.loads(config_path.read_text(encoding="utf-8"))
    except Exception:
        return False
    output = data.get("output")
    if not isinstance(output, dict):
        return False
    return bool(output.get("auto_export", False))
