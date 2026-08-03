"""Render a ConformanceReport as byte-stable JSON or human-readable text (spec D7)."""

from __future__ import annotations

import json

from prograph.conformance.engine import FINDING_CLASSES, ConformanceReport
from prograph.conformance.provenance import ReportProvenance


def report_payload(
    report: ConformanceReport,
    provenance: ReportProvenance,
) -> dict[str, object]:
    """The JSON-ready dict; every intended element appears, judged or not."""
    verdict_counts = {"conformant": 0, "violation": 0, "unknown": 0}
    for el in report.elements:
        verdict_counts[el.verdict] += 1
    finding_counts = {cls: 0 for cls in FINDING_CLASSES}
    for f in report.findings:
        finding_counts[f.finding_class] += 1
    return {
        "schema": "conformance-report/v1",
        "system": report.system,
        "generated_at": provenance.generated_at,
        "manifest": {
            "project": provenance.manifest_project,
            "path": provenance.manifest_path,
            "sha256": provenance.manifest_sha256,
        },
        "snapshot": {
            "id": provenance.snapshot_id,
            "indexed_at": provenance.snapshot_indexed_at,
            "content_hash": provenance.snapshot_content_hash,
            "complete": provenance.complete,
        },
        "tool": {
            "name": provenance.tool_name,
            "version": provenance.tool_version,
            "schema": provenance.tool_schema,
        },
        "projects": {
            name: {"commit": commit, "dirty": dirty}
            for name, (commit, dirty) in provenance.projects.items()
        },
        "elements": [
            {
                "id": el.id,
                "type": el.element_type,
                "detector": el.detector,
                "verdict": el.verdict,
                "reason": el.reason,
                "waived_by": el.waived_by,
            }
            for el in report.elements
        ],
        "findings": [
            {
                "class": f.finding_class,
                "element": f.element,
                "detail": f.detail,
                "suppressed_by": f.suppressed_by,
            }
            for f in report.findings
        ],
        "exceptions": [
            {"id": e.id, "target": e.target, "expires": e.expires, "status": e.status}
            for e in report.exceptions
        ],
        "summary": {"verdicts": verdict_counts, "findings": finding_counts},
    }


def render_json(
    report: ConformanceReport,
    provenance: ReportProvenance,
) -> str:
    payload = report_payload(report, provenance)
    return json.dumps(payload, indent=2, sort_keys=True) + "\n"


def render_text(
    report: ConformanceReport,
    provenance: ReportProvenance,
) -> str:
    lines: list[str] = [
        f"# Conformance: {report.system}",
        f"manifest: {provenance.manifest_path} (sha256 {provenance.manifest_sha256[:12]}…)"
        + (f" [project {provenance.manifest_project}]" if provenance.manifest_project else ""),
        f"snapshot: {provenance.snapshot_id} (indexed {provenance.snapshot_indexed_at})",
        f"generated: {provenance.generated_at}",
        "",
        "## Elements",
    ]
    for el in report.elements:
        note = f" ({el.reason})" if el.reason else ""
        waived = f" [waived by {el.waived_by}]" if el.waived_by else ""
        lines.append(
            f"  {el.id:<12} {el.element_type}/{el.detector:<16} {el.verdict}{note}{waived}"
        )
    lines += ["", "## Findings"]
    if not report.findings:
        lines.append("  (none)")
    for f in report.findings:
        suppressed = f" [suppressed by {f.suppressed_by}]" if f.suppressed_by else ""
        anchor = f.element or "manifest"
        lines.append(f"  {f.finding_class:<22} {anchor:<12} {f.detail}{suppressed}")
    if report.exceptions:
        lines += ["", "## Exceptions"]
        for e in report.exceptions:
            lines.append(f"  {e.id:<8} target {e.target:<12} expires {e.expires} — {e.status}")
    counts = {"conformant": 0, "violation": 0, "unknown": 0}
    for el in report.elements:
        counts[el.verdict] += 1
    lines += [
        "",
        f"summary: {counts['conformant']} conformant · {counts['violation']} violation · "
        f"{counts['unknown']} unknown · {len(report.findings)} findings",
        "",
    ]
    return "\n".join(lines)
