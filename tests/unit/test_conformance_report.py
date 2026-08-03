"""Conformance report rendering: byte-stable JSON + human text."""

import json

from prograph.conformance.engine import (
    ConformanceReport,
    ElementResult,
    ExceptionStatus,
    Finding,
)
from prograph.conformance.provenance import ReportProvenance
from prograph.conformance.report import render_json, render_text, report_payload

REPORT = ConformanceReport(
    system="fixture-feed",
    elements=(
        ElementResult(
            id="I-01",
            element_type="interface",
            detector="import",
            verdict="conformant",
            reason=None,
        ),
        ElementResult(
            id="I-05",
            element_type="interface",
            detector="mcp",
            verdict="unknown",
            reason=None,
            waived_by="EX-01",
        ),
        ElementResult(
            id="C-1",
            element_type="constraint",
            detector="import",
            verdict="violation",
            reason=None,
        ),
    ),
    findings=(
        Finding("forbidden-edge", "C-1", "observed gamma -[package_dep]-> alpha"),
        Finding("missing-required-edge", "I-05", "no mcp edge", suppressed_by="EX-01"),
    ),
    exceptions=(ExceptionStatus(id="EX-01", target="I-05", expires="2999-01-01", status="active"),),
)

PROV = ReportProvenance(
    generated_at="2026-08-03T12:00:00Z",
    manifest_project="gamma",
    manifest_path="spec/intended-graph.yaml",
    manifest_sha256="ab" * 32,
    snapshot_id=1,
    snapshot_indexed_at="2026-08-03T00:00:00Z",
    snapshot_content_hash="prograph-snapshot/v1+sha256:" + "cd" * 32,
    complete=True,
    tool_name="prograph",
    tool_version="0.1.0",
    tool_schema="intended-graph/v1",
    projects={"alpha": (None, None), "gamma": ("e" * 40, False)},
)


def test_payload_provenance_block() -> None:
    p = report_payload(REPORT, PROV)
    assert p["generated_at"] == "2026-08-03T12:00:00Z"
    assert p["manifest"] == {
        "project": "gamma",
        "path": "spec/intended-graph.yaml",
        "sha256": "ab" * 32,
    }
    assert p["snapshot"] == {
        "id": 1,
        "indexed_at": "2026-08-03T00:00:00Z",
        "content_hash": "prograph-snapshot/v1+sha256:" + "cd" * 32,
        "complete": True,
    }
    assert p["tool"] == {
        "name": "prograph",
        "version": "0.1.0",
        "schema": "intended-graph/v1",
    }
    assert p["projects"] == {
        "alpha": {"commit": None, "dirty": None},
        "gamma": {"commit": "e" * 40, "dirty": False},
    }


def test_payload_shape() -> None:
    p = report_payload(REPORT, PROV)
    assert p["schema"] == "conformance-report/v1"
    summary = p["summary"]
    assert isinstance(summary, dict)
    verdicts = summary["verdicts"]
    assert isinstance(verdicts, dict)
    assert verdicts == {"conformant": 1, "violation": 1, "unknown": 1}
    findings = summary["findings"]
    assert isinstance(findings, dict)
    assert findings["forbidden-edge"] == 1
    assert findings["undeclared-edge"] == 0
    assert set(findings) == {
        "missing-required-edge",
        "forbidden-edge",
        "undeclared-edge",
        "orphan-component",
        "expired-waiver",
        "manual-obligation",
    }


def test_json_is_byte_stable() -> None:
    a = render_json(REPORT, PROV)
    b = render_json(REPORT, PROV)
    assert a == b
    assert a.endswith("\n")
    parsed = json.loads(a)
    assert parsed == json.loads(json.dumps(parsed, sort_keys=True))
    assert a == json.dumps(parsed, indent=2, sort_keys=True) + "\n"


def test_text_lists_every_element_and_finding() -> None:
    text = render_text(REPORT, PROV)
    for needle in (
        "fixture-feed",
        "I-01",
        "I-05",
        "C-1",
        "conformant",
        "violation",
        "forbidden-edge",
        "EX-01",
        "waived",
        "suppressed",
        "generated",
    ):
        assert needle in text, f"missing {needle!r} in:\n{text}"
