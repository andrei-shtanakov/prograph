"""D6 sync tests: the published contracts ARE what the code implements — structural layer."""

import json
from pathlib import Path

import jsonschema
import pytest
import yaml

from prograph.conformance.manifest import ManifestError, load_manifest
from prograph.conformance.report import report_payload

REPO = Path(__file__).resolve().parent.parent.parent
MANIFEST_SCHEMA = json.loads(
    (REPO / "contracts" / "intended-graph" / "v1" / "schema.json").read_text(encoding="utf-8")
)
REPORT_SCHEMA = json.loads(
    (REPO / "contracts" / "conformance-report" / "v1" / "schema.json").read_text(encoding="utf-8")
)


def _yaml_as_json(path: Path) -> object:
    """YAML -> JSON-types canonicalization (dates become ISO strings)."""
    raw = yaml.safe_load(path.read_text(encoding="utf-8"))
    return json.loads(json.dumps(raw, default=str))


ACCEPTED_MANIFESTS = [
    REPO / "tests" / "fixtures" / "monorepo_conformance" / "gamma" / "spec" / "intended-graph.yaml",
    REPO / "tests" / "fixtures" / "monorepo_conformance" / "green-manifest.yaml",
    REPO / "tests" / "fixtures" / "ws005_manifest" / "intended-graph.yaml",
]


@pytest.mark.parametrize("path", ACCEPTED_MANIFESTS, ids=lambda p: p.parent.name)
def test_loader_accepted_manifests_validate_structurally(path: Path) -> None:
    load_manifest(path)  # loader accepts (raises otherwise)
    jsonschema.validate(_yaml_as_json(path), MANIFEST_SCHEMA)


def test_structural_rejections_agree(tmp_path: Path) -> None:
    """Structural defects: loader rejects AND schema rejects."""
    base = _yaml_as_json(ACCEPTED_MANIFESTS[1])
    assert isinstance(base, dict)
    for mutate in (
        lambda d: d.update(extra_key="boom"),
        lambda d: d.update(schema="intended-graph/v2"),
        lambda d: d["components"][0].update(kind="banana"),
        lambda d: d["components"][0].pop("owner"),
    ):
        doc = json.loads(json.dumps(base))
        mutate(doc)
        with pytest.raises(jsonschema.ValidationError):
            jsonschema.validate(doc, MANIFEST_SCHEMA)
        p = tmp_path / "m.yaml"
        p.write_text(yaml.safe_dump(doc), encoding="utf-8")
        with pytest.raises(ManifestError):
            load_manifest(p)


def test_integrity_rejections_pass_schema_documenting_the_boundary(
    tmp_path: Path,
) -> None:
    """Integrity-only defects: loader rejects, schema PASSES — the documented D6 split."""
    base = _yaml_as_json(ACCEPTED_MANIFESTS[1])
    assert isinstance(base, dict)
    dup = json.loads(json.dumps(base))
    dup["components"].append(dict(dup["components"][0]))  # duplicate id
    dangling = json.loads(json.dumps(base))
    dangling["interfaces"] = [
        {
            "id": "I-90",
            "producer": "no.such",
            "consumer": dangling["components"][0]["id"],
            "detector": "import",
        }
    ]
    for doc in (dup, dangling):
        jsonschema.validate(doc, MANIFEST_SCHEMA)  # structurally fine
        p = tmp_path / "m.yaml"
        p.write_text(yaml.safe_dump(doc), encoding="utf-8")
        with pytest.raises(ManifestError):
            load_manifest(p)


def test_report_payloads_validate() -> None:
    from tests.unit.test_conformance_report import PROV, REPORT

    jsonschema.validate(report_payload(REPORT, PROV), REPORT_SCHEMA)


def test_golden_report_validates() -> None:
    golden = REPO / "tests" / "fixtures" / "monorepo_conformance" / "golden" / ("conformance.json")
    jsonschema.validate(json.loads(golden.read_text(encoding="utf-8")), REPORT_SCHEMA)
