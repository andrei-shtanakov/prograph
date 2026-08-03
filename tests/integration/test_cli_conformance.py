"""prograph conformance: end-to-end over the monorepo_conformance fixture."""

import json
import os
import shutil
from pathlib import Path

import pytest
from typer.testing import CliRunner

from prograph.cli import app

runner = CliRunner()
FIXTURE = Path(__file__).resolve().parent.parent / "fixtures" / "monorepo_conformance"
GOLDEN = FIXTURE / "golden" / "conformance.json"


@pytest.fixture(scope="module")
def indexed(tmp_path_factory: pytest.TempPathFactory) -> Path:
    dst = tmp_path_factory.mktemp("conf") / "monorepo_conformance"
    shutil.copytree(FIXTURE, dst, ignore=shutil.ignore_patterns("golden"))
    assert runner.invoke(app, ["init", "--monorepo", str(dst)]).exit_code == 0
    assert runner.invoke(app, ["index", "--monorepo", str(dst)]).exit_code == 0
    return dst


def _json_run(indexed: Path, *extra: str) -> tuple[int, dict]:
    res = runner.invoke(
        app,
        [
            "conformance",
            "--monorepo",
            str(indexed),
            "--project",
            "gamma",
            "--format",
            "json",
            *extra,
        ],
    )
    return res.exit_code, json.loads(res.stdout)


def test_default_run_exits_1_on_violation(indexed: Path) -> None:
    code, payload = _json_run(indexed)
    assert code == 1  # ARCH-C2 violation + expired EX-02
    verdicts = {e["id"]: (e["verdict"], e["reason"]) for e in payload["elements"]}
    assert verdicts == {
        "I-01": ("conformant", None),
        "I-02": ("conformant", None),
        "I-03": ("conformant", None),
        "I-04": ("unknown", "unsupported-resolution"),
        "I-05": ("unknown", None),
        "I-06": ("unknown", "outside-workspace"),
        "I-07": ("unknown", "orphan-component"),
        "ARCH-C1": ("conformant", None),
        "ARCH-C2": ("violation", None),
        "ARCH-C3": ("unknown", "unsupported-resolution"),
        "ARCH-C4": ("unknown", "manual-evidence"),
    }
    classes = sorted(f["class"] for f in payload["findings"])
    assert classes == [
        "expired-waiver",
        "forbidden-edge",
        "manual-obligation",
        "missing-required-edge",
        "orphan-component",
        "orphan-component",
        "undeclared-edge",
    ]
    i05 = next(f for f in payload["findings"] if f["element"] == "I-05")
    assert i05["suppressed_by"] == "EX-01"


def test_all_six_finding_classes_reachable(indexed: Path) -> None:
    _, payload = _json_run(indexed)
    nonzero = {k for k, v in payload["summary"]["findings"].items() if v > 0}
    assert nonzero == {
        "missing-required-edge",
        "forbidden-edge",
        "undeclared-edge",
        "orphan-component",
        "expired-waiver",
        "manual-obligation",
    }


def test_json_matches_golden(indexed: Path) -> None:
    res = runner.invoke(
        app,
        ["conformance", "--monorepo", str(indexed), "--project", "gamma", "--format", "json"],
    )
    if os.environ.get("PROGRAPH_UPDATE_GOLDEN") == "1":
        GOLDEN.parent.mkdir(parents=True, exist_ok=True)
        GOLDEN.write_text(res.stdout, encoding="utf-8")
    assert res.stdout == GOLDEN.read_text(encoding="utf-8")


def test_green_manifest_exits_0(indexed: Path) -> None:
    res = runner.invoke(
        app,
        [
            "conformance",
            "--monorepo",
            str(indexed),
            "--manifest",
            str(indexed / "green-manifest.yaml"),
        ],
    )
    assert res.exit_code == 0, res.stdout


def test_fail_on_escalates(indexed: Path) -> None:
    res = runner.invoke(
        app,
        [
            "conformance",
            "--monorepo",
            str(indexed),
            "--manifest",
            str(indexed / "green-manifest.yaml"),
            "--fail-on-verdict",
            "unknown",
        ],
    )
    assert res.exit_code == 0  # green manifest has no unknowns

    code, _ = _json_run(indexed, "--fail-on", "undeclared-edge")
    assert code == 1


def test_unknown_fail_on_class_is_tool_error(indexed: Path) -> None:
    res = runner.invoke(
        app,
        [
            "conformance",
            "--monorepo",
            str(indexed),
            "--project",
            "gamma",
            "--fail-on",
            "nonsense-class",
        ],
    )
    assert res.exit_code == 2


def test_unreadable_manifest_is_exit_2(indexed: Path, tmp_path: Path) -> None:
    bad = tmp_path / "bad.yaml"
    bad.write_text("schema: intended-graph/v9\nsystem: x\ncomponents: []\n", encoding="utf-8")
    res = runner.invoke(app, ["conformance", "--monorepo", str(indexed), "--manifest", str(bad)])
    assert res.exit_code == 2


def test_no_snapshot_is_exit_2(tmp_path: Path) -> None:
    res = runner.invoke(
        app,
        [
            "conformance",
            "--monorepo",
            str(tmp_path),
            "--manifest",
            str(FIXTURE / "green-manifest.yaml"),
        ],
    )
    assert res.exit_code == 2


def test_manifest_and_project_are_mutually_exclusive(indexed: Path) -> None:
    res = runner.invoke(
        app,
        [
            "conformance",
            "--monorepo",
            str(indexed),
            "--project",
            "gamma",
            "--manifest",
            str(indexed / "green-manifest.yaml"),
        ],
    )
    assert res.exit_code == 2
    res = runner.invoke(app, ["conformance", "--monorepo", str(indexed)])
    assert res.exit_code == 2


def test_text_format_default(indexed: Path) -> None:
    res = runner.invoke(app, ["conformance", "--monorepo", str(indexed), "--project", "gamma"])
    assert res.exit_code == 1
    assert "fixture-feed" in res.stdout and "ARCH-C2" in res.stdout
