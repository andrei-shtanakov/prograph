"""Tests for `prograph index --export-md` and `prograph export-md`."""

import json
from pathlib import Path

from typer.testing import CliRunner

from prograph.cli import app
from prograph.paths import PrographPaths

runner = CliRunner()


def _setup(root: Path) -> None:
    (root / "alpha").mkdir()
    (root / "alpha" / "pyproject.toml").write_text("[project]\nname='alpha'\n")
    (root / "alpha" / "README.md").write_text("# alpha\n\nThe alpha project.\n")
    (root / "beta").mkdir()
    (root / "beta" / "pyproject.toml").write_text(
        "[project]\nname='beta'\ndependencies=['alpha']\n"
    )


def test_index_with_export_md_writes_files(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--export-md"])
    assert result.exit_code == 0, result.stdout

    paths = PrographPaths(monorepo_root=tmp_path)
    assert (paths.projects_md_dir / "alpha.md").is_file()
    assert (paths.projects_md_dir / "beta.md").is_file()
    assert paths.index_md_path.is_file()

    alpha_md = (paths.projects_md_dir / "alpha.md").read_text()
    assert "# alpha" in alpha_md
    assert "> The alpha project." in alpha_md


def test_export_md_standalone(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    result = runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.stdout

    paths = PrographPaths(monorepo_root=tmp_path)
    assert (paths.projects_md_dir / "alpha.md").is_file()


def test_export_md_leaves_journal_untouched(tmp_path: Path):
    """kb-save journals live under `derived/journal/` (a sibling of `projects/`)
    and are append-only, not regenerable. The stale-MD cleanup is scoped to
    `projects/`, so a journal file — even one carrying the generated marker —
    must survive an export refresh."""
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])

    paths = PrographPaths(monorepo_root=tmp_path)
    export_root = paths.projects_md_dir.parent
    journal = export_root / "journal" / "prograph" / "journal.md"
    journal.parent.mkdir(parents=True, exist_ok=True)
    # Worst case: even a file that carries the generated marker must be spared,
    # because the cleanup only rglobs projects/.
    journal.write_text(
        "<!-- prograph:generated -->\n# journal\nentry-must-survive\n",
        encoding="utf-8",
    )

    result = runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0, result.stdout
    assert journal.is_file()
    assert "entry-must-survive" in journal.read_text(encoding="utf-8")


def test_export_md_idempotent_byte_stable(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])

    paths = PrographPaths(monorepo_root=tmp_path)
    first = (paths.projects_md_dir / "alpha.md").read_bytes()

    runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])
    second = (paths.projects_md_dir / "alpha.md").read_bytes()

    assert first == second, "two export-md runs on the same snapshot must produce identical bytes"


def test_export_md_requires_init(tmp_path: Path):
    result = runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1


def test_export_md_requires_snapshot(tmp_path: Path):
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    # No `index` run — no snapshot.
    result = runner.invoke(app, ["export-md", "--monorepo", str(tmp_path)])
    assert result.exit_code == 1
    assert "no snapshot" in (result.stdout + result.stderr).lower()


def test_auto_export_in_config_triggers_md(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])

    paths = PrographPaths(monorepo_root=tmp_path)
    # Flip auto_export = true in config.toml.
    config = paths.config_path.read_text()
    paths.config_path.write_text(config.replace("auto_export = false", "auto_export = true"))

    # Now `prograph index` (without --export-md) should still write MD.
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path)])
    assert result.exit_code == 0
    assert (paths.projects_md_dir / "alpha.md").is_file()


def test_auto_export_false_skips_md(tmp_path: Path):
    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    # auto_export defaults to false; index without flag should NOT write MD.
    runner.invoke(app, ["index", "--monorepo", str(tmp_path)])

    paths = PrographPaths(monorepo_root=tmp_path)
    assert not (paths.projects_md_dir / "alpha.md").is_file()


# ----- Golden tests against checked-in fixtures (Task 12) -----

import shutil  # noqa: E402


def _run_full_export(fixture_name: str, tmp_path: Path) -> Path:
    """Copy fixture into tmp_path, init + index --export-md, return the produced .prograph dir."""
    src = Path(__file__).resolve().parents[1] / "fixtures" / fixture_name
    dst = tmp_path / fixture_name
    shutil.copytree(src, dst, ignore=shutil.ignore_patterns("golden"))
    runner.invoke(app, ["init", "--monorepo", str(dst)])
    result = runner.invoke(app, ["index", "--monorepo", str(dst), "--export-md"])
    assert result.exit_code == 0, result.stdout
    return dst / ".prograph"


def test_golden_monorepo_full(tmp_path: Path, md_matcher):
    produced = _run_full_export("monorepo_full", tmp_path)
    golden = Path(__file__).resolve().parents[1] / "fixtures" / "monorepo_full" / "golden"
    md_matcher(produced, golden)


def test_golden_monorepo_multilang(tmp_path: Path, md_matcher):
    produced = _run_full_export("monorepo_multilang", tmp_path)
    golden = Path(__file__).resolve().parents[1] / "fixtures" / "monorepo_multilang" / "golden"
    md_matcher(produced, golden)


def test_golden_monorepo_mcp(tmp_path: Path, md_matcher):
    produced = _run_full_export("monorepo_mcp", tmp_path)
    golden = Path(__file__).resolve().parents[1] / "fixtures" / "monorepo_mcp" / "golden"
    md_matcher(produced, golden)


def test_export_md_survives_case_only_project_rename(tmp_path: Path):
    """Inbox #30 regression: on a case-insensitive FS (macOS APFS, Windows) a
    case-only rename (`Maestro/` -> `maestro/`) used to lose the page — the
    exporter wrote `projects/maestro.md` into the *same on-disk file* as the
    stale `Maestro.md` (case-preserving FS keeps the old entry name), then the
    case-sensitive stale cleanup unlinked the freshly written page. On a
    case-sensitive FS the two names are distinct files and the old page is
    simply removed as stale — every assertion below must hold on both."""
    (tmp_path / "Maestro").mkdir()
    (tmp_path / "Maestro" / "pyproject.toml").write_text("[project]\nname='maestro'\n")
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--export-md"])
    assert result.exit_code == 0, result.stdout

    paths = PrographPaths(monorepo_root=tmp_path)
    assert (paths.projects_md_dir / "Maestro.md").is_file()

    (tmp_path / "Maestro").rename(tmp_path / "maestro")
    result = runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--export-md", "--json"])
    assert result.exit_code == 0, result.stdout
    payload = json.loads(result.stdout)

    # Page survives under the new exact case; the old-cased entry is gone.
    on_disk = sorted(p.name for p in paths.projects_md_dir.rglob("*.md"))
    assert "maestro.md" in on_disk
    assert "Maestro.md" not in on_disk
    # The index links the new name, and every reported project has a page.
    assert "[[maestro]]" in paths.index_md_path.read_text(encoding="utf-8")
    assert len(on_disk) == payload["n_projects"]


def test_reindex_md_stable_modulo_timestamps(tmp_path: Path):
    """Re-indexing same state produces MD files identical except for ts/snapshot fields."""
    import re

    _setup(tmp_path)
    runner.invoke(app, ["init", "--monorepo", str(tmp_path)])
    runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--export-md"])

    paths = PrographPaths(monorepo_root=tmp_path)
    first = (paths.projects_md_dir / "alpha.md").read_text()

    # Second index — same state, fresh snapshot row.
    runner.invoke(app, ["index", "--monorepo", str(tmp_path), "--export-md"])
    second = (paths.projects_md_dir / "alpha.md").read_text()

    def normalize(t: str) -> str:
        t = re.sub(r"^indexed_at: .*$", "indexed_at: <ts>", t, flags=re.MULTILINE)
        t = re.sub(r"^snapshot: \d+$", "snapshot: <n>", t, flags=re.MULTILINE)
        # recent_changes also has the ts inline.
        t = re.sub(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", "<ts>", t)
        # `snapshot N` in body lines.
        t = re.sub(r"snapshot \d+ ", "snapshot <n> ", t)
        return t

    assert normalize(first) == normalize(second), (
        "MD bytes must be stable across reindexes of identical source"
    )
