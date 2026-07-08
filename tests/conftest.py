"""Shared pytest fixtures for prograph tests."""

import os
import re
import shutil
from pathlib import Path

import pytest

FIXTURES_DIR = Path(__file__).parent / "fixtures"


@pytest.fixture
def fixtures_dir() -> Path:
    """Path to the tests/fixtures/ directory."""
    return FIXTURES_DIR


def _normalize(raw: bytes) -> bytes:
    """Replace per-run timestamp + tmp-path values for byte-stable golden comparison."""
    text = raw.decode("utf-8", errors="replace")
    # `indexed_at:` frontmatter line — quoted or not.
    text = re.sub(
        r'indexed_at: "?\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z"?',
        "indexed_at: <ts>",
        text,
    )
    # Any inline ISO timestamp in body text (recent_changes etc.).
    text = re.sub(
        r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z",
        "<ts>",
        text,
    )
    # The `# Monorepo: <absolute path>` header in index.md uses tmp_path which differs
    # per test run. Mask it.
    text = re.sub(r"^# Monorepo: .+$", "# Monorepo: <path>", text, flags=re.MULTILINE)
    return text.encode("utf-8")


def assert_md_dir_matches_golden(produced_dir: Path, golden_dir: Path) -> None:
    """Compare every .md file under produced_dir to its peer under golden_dir.

    If PROGRAPH_UPDATE_GOLDEN=1 is set, regenerate the golden directory from produced.
    Timestamps are normalized before comparing — snapshot drift on slow runs is OK.
    """
    if os.environ.get("PROGRAPH_UPDATE_GOLDEN") == "1":
        if golden_dir.exists():
            shutil.rmtree(golden_dir)
        # Copy only .md files (no graph.db, no config.toml, etc.)
        golden_dir.mkdir(parents=True, exist_ok=True)
        for src in produced_dir.rglob("*.md"):
            rel = src.relative_to(produced_dir)
            dst = golden_dir / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            dst.write_bytes(src.read_bytes())
        return

    if not golden_dir.exists():
        raise AssertionError(
            f"golden directory missing: {golden_dir}. "
            f"Run with PROGRAPH_UPDATE_GOLDEN=1 to create it."
        )

    # mcp_patterns/README.md is documentation, not graph state — skip it for golden compare.
    def _relevant(root: Path) -> list[Path]:
        return sorted(
            p.relative_to(root) for p in root.rglob("*.md") if "mcp_patterns" not in p.parts
        )

    produced_files = _relevant(produced_dir)
    golden_files = _relevant(golden_dir)

    if produced_files != golden_files:
        only_in_produced = sorted(set(produced_files) - set(golden_files))
        only_in_golden = sorted(set(golden_files) - set(produced_files))
        msg = ["MD file lists differ between produced and golden:"]
        if only_in_produced:
            msg.append(f"  Only in produced: {only_in_produced}")
        if only_in_golden:
            msg.append(f"  Only in golden: {only_in_golden}")
        msg.append("  Set PROGRAPH_UPDATE_GOLDEN=1 to refresh.")
        raise AssertionError("\n".join(msg))

    for rel in produced_files:
        p_bytes = _normalize((produced_dir / rel).read_bytes())
        g_bytes = _normalize((golden_dir / rel).read_bytes())
        if p_bytes != g_bytes:
            import difflib

            diff = "\n".join(
                difflib.unified_diff(
                    g_bytes.decode("utf-8", errors="replace").splitlines(),
                    p_bytes.decode("utf-8", errors="replace").splitlines(),
                    fromfile=f"golden/{rel}",
                    tofile=f"produced/{rel}",
                    lineterm="",
                )
            )
            raise AssertionError(
                f"MD file differs from golden: {rel}\n{diff}\n"
                f"Set PROGRAPH_UPDATE_GOLDEN=1 to refresh."
            )


@pytest.fixture
def md_matcher():
    """Return the assert_md_dir_matches_golden helper as a callable fixture."""
    return assert_md_dir_matches_golden
