"""Intended-graph/v1 manifest: schema validation + strict loader."""

from pathlib import Path

import pytest

from prograph.conformance.manifest import (
    ForbiddenRule,
    ManifestError,
    load_manifest,
    parse_rule,
)

VALID = """\
schema: intended-graph/v1
system: fixture-feed
components:
  - id: alpha.api
    project: alpha
    kind: service
    owner: architects
    responsibility: "serves the feed API"
  - id: beta.lib
    project: beta
    kind: module
    owner: architects
    scope: src/beta
    responsibility: "shared library"
    evidence: [BEH-01]
interfaces:
  - id: I-01
    producer: beta.lib
    consumer: alpha.api
    protocol: "python package"
    detector: import
  - id: I-02
    producer: "file:beta/data/feed.txt"
    consumer: alpha.api
    protocol: "text file"
    detector: declared
constraints:
  - id: ARCH-C1
    rule: "forbidden: alpha -> beta"
    detector: import
    evidence: [FR-02]
  - id: ARCH-C2
    rule: "forbidden: панель мутирует данные — только чтение"
    detector: manual-evidence
resources:
  - "runtime: nothing new"
exceptions:
  - id: EX-01
    target: I-01
    reason: "grandfathered"
    owner: architects
    expires: 2026-09-01
"""


def _write(tmp_path: Path, text: str) -> Path:
    p = tmp_path / "intended-graph.yaml"
    p.write_text(text, encoding="utf-8")
    return p


def test_valid_manifest_loads(tmp_path: Path) -> None:
    m = load_manifest(_write(tmp_path, VALID))
    assert m.schema_ == "intended-graph/v1"
    assert m.system == "fixture-feed"
    assert [c.id for c in m.components] == ["alpha.api", "beta.lib"]
    assert m.components[1].scope == "src/beta"
    assert m.interfaces[1].producer == "file:beta/data/feed.txt"
    assert m.constraints[0].detector == "import"
    assert m.exceptions[0].expires.isoformat() == "2026-09-01"


def test_wrong_schema_id_rejected(tmp_path: Path) -> None:
    bad = VALID.replace("intended-graph/v1", "intended-graph/v2")
    with pytest.raises(ManifestError, match="unknown schema"):
        load_manifest(_write(tmp_path, bad))


def test_unknown_top_level_key_rejected(tmp_path: Path) -> None:
    with pytest.raises(ManifestError):
        load_manifest(_write(tmp_path, VALID + "extra_key: boom\n"))


def test_unknown_nested_key_rejected(tmp_path: Path) -> None:
    bad = VALID.replace("kind: service", "kind: service\n    color: red")
    with pytest.raises(ManifestError):
        load_manifest(_write(tmp_path, bad))


def test_duplicate_id_rejected(tmp_path: Path) -> None:
    bad = VALID.replace("id: beta.lib", "id: alpha.api")
    with pytest.raises(ManifestError, match="duplicate id"):
        load_manifest(_write(tmp_path, bad))


def test_dangling_interface_endpoint_rejected(tmp_path: Path) -> None:
    bad = VALID.replace(
        'consumer: alpha.api\n    protocol: "python package"',
        'consumer: no.such\n    protocol: "python package"',
    )
    with pytest.raises(ManifestError, match="unknown component"):
        load_manifest(_write(tmp_path, bad))


def test_interface_with_two_file_endpoints_rejected(tmp_path: Path) -> None:
    bad = VALID.replace("producer: beta.lib", 'producer: "file:a.txt"').replace(
        'consumer: alpha.api\n    protocol: "python package"',
        'consumer: "file:b.txt"\n    protocol: "python package"',
    )
    with pytest.raises(ManifestError, match="at least one component"):
        load_manifest(_write(tmp_path, bad))


def test_dangling_exception_target_rejected(tmp_path: Path) -> None:
    bad = VALID.replace("target: I-01", "target: I-99")
    with pytest.raises(ManifestError, match="unknown element"):
        load_manifest(_write(tmp_path, bad))


def test_expires_required(tmp_path: Path) -> None:
    bad = VALID.replace("    expires: 2026-09-01\n", "")
    with pytest.raises(ManifestError):
        load_manifest(_write(tmp_path, bad))


def test_unparseable_rule_on_mechanical_detector_rejected(tmp_path: Path) -> None:
    bad = VALID.replace('rule: "forbidden: alpha -> beta"', 'rule: "no arrow here"')
    with pytest.raises(ManifestError, match="unparseable"):
        load_manifest(_write(tmp_path, bad))


def test_manual_evidence_rule_is_prose_not_parsed(tmp_path: Path) -> None:
    # ARCH-C2 above carries prose with no '->': must load fine.
    m = load_manifest(_write(tmp_path, VALID))
    assert m.constraints[1].detector == "manual-evidence"


def test_parse_rule_grammar() -> None:
    r = parse_rule("forbidden: dispatcher.governance-panel -> file:.steward/x.jsonl")
    assert r == ForbiddenRule(src="dispatcher.governance-panel", dst="file:.steward/x.jsonl")
    with pytest.raises(ManifestError):
        parse_rule("layering: a -> b -> c")


def test_missing_file_is_manifest_error(tmp_path: Path) -> None:
    with pytest.raises(ManifestError, match="cannot read"):
        load_manifest(tmp_path / "nope.yaml")


def test_non_mapping_root_rejected(tmp_path: Path) -> None:
    with pytest.raises(ManifestError, match="mapping"):
        load_manifest(_write(tmp_path, "- just\n- a list\n"))


def test_read_intended_path(tmp_path: Path) -> None:
    from prograph.config import read_intended_path

    py = tmp_path / "pyproject.toml"
    py.write_text(
        '[project]\nname = "x"\n\n[tool.prograph]\nintended = "arch/graph.yaml"\n',
        encoding="utf-8",
    )
    assert read_intended_path(py) == "arch/graph.yaml"


def test_read_intended_path_absent(tmp_path: Path) -> None:
    from prograph.config import read_intended_path

    py = tmp_path / "pyproject.toml"
    py.write_text('[project]\nname = "x"\n', encoding="utf-8")
    assert read_intended_path(py) is None
    assert read_intended_path(tmp_path / "missing.toml") is None


def test_read_intended_path_malformed(tmp_path: Path) -> None:
    from prograph.config import read_intended_path

    py = tmp_path / "pyproject.toml"
    py.write_text("not [ toml", encoding="utf-8")
    assert read_intended_path(py) is None
