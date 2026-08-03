# Intended Graph v1 + `prograph conformance` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the prescriptive plane from the approved spec
`docs/superpowers/specs/2026-08-03-prograph-intended-graph-design.md`: a strict loader for
`intended-graph/v1` manifests, a three-valued verdict engine over the existing edge store, and a
`prograph conformance` CLI with byte-stable JSON output and fail-closed exit codes.

**Architecture:** Pure Python feature — no Rust changes, no schema migration (spec D8: the
manifest is read at check time, never written to `graph.db`). New package
`prograph/conformance/` with three modules: `manifest.py` (pydantic schema + strict YAML
loader), `engine.py` (data-only verdict/finding engine over an `ObservedGraph` snapshot
adapter), `report.py` (text + byte-stable JSON rendering). One new CLI command in
`prograph/cli.py`. The engine is deliberately decoupled from the DB: `evaluate()` takes plain
dataclasses so unit tests need no index; a thin `load_observed()` adapter wraps the existing
`_core` query helpers (`monorepo_overview`, `find_edges_filtered`, `describe_project`).

**Tech Stack:** Python 3.11+, pydantic v2 (already a dep), PyYAML (new dep), typer CLI
(existing pattern), pytest + CliRunner (existing pattern).

## Global Constraints

- Package management: **uv only**, never pip. New deps: `uv add "pyyaml>=6,<7"` and
  `uv add --dev types-pyyaml`.
- Ruff line length is **100** in this repo (`pyproject.toml [tool.ruff]`), not 88.
- Type hints everywhere; after every change run
  `uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'`
  (never bare `pyrefly check` — project-mode excludes collide with `.gitignore`).
- Format/lint gate before each commit: `uv run ruff format .` then `uv run ruff check .`.
- Local checks ARE CI: there is no remote workflow running pytest/ruff/pyrefly — run
  `uv run pytest -v` locally before claiming green.
- No Rust edits anywhere in this plan → **no `maturin develop` needed**.
- Git: branch `feat/conformance-v1`, commits per task, PR at the end; **no direct commits to
  master, do not merge** (owner merges).
- Spec vocabulary is normative and closed: verdicts `conformant | violation | unknown`;
  unknown reasons `manual-evidence | unsupported-resolution | outside-workspace |
  orphan-component`; finding classes `missing-required-edge | forbidden-edge |
  undeclared-edge | orphan-component | expired-waiver | manual-obligation`. Do not invent
  additions.
- Existing edge-kind strings (DB + `find_edges_filtered` filter values): `package_dep`,
  `mcp_call`, `contract_link`, `declared`. Node kinds: `project`, `contract`. Declared-edge
  `attrs_json` = `{"mode": "read"|"write", "path": "<workspace-relative, e.g.
  proctor/data/state.db>"}`.

## Semantics locked by this plan (decisions the spec left to implementation)

These four rules are referenced by multiple tasks; they are the plan's contract.

1. **File-endpoint path matching** (`file:<p>` vs a Declared edge's `attrs.path`): normalize a
   leading `./` off `<p>`, then match **exact** or **segment-suffix** (`attrs.path ==
   p or attrs.path.endswith("/" + p)`). Rationale: M12 declared paths are workspace-relative
   with a project-name prefix (`steward/.steward/gate_verdicts.jsonl`) while real manifests
   (WS-005) write repo-generic paths (`.steward/gate_verdicts.jsonl`).
2. **Forbidden-constraint attribution**: a constraint endpoint that is a *component id* is
   attributable at v1's project granularity **only if that component is the sole modelled
   component of its project**. If its project hosts ≥2 modelled components, the constraint's
   verdict is unconditionally `unknown` / `unsupported-resolution` (matches the WS-005
   ARCH-C4 expectation "unknown — постоянно до module-level v1.1"). Project-name endpoints
   are always exact at project granularity.
3. **Interfaces stay optimistic at project granularity** (spec D2: "v1 checks at project
   granularity"): a required edge observed at project level ⇒ `conformant`, even when the
   endpoint project hosts several modelled components. Only *same-project* endpoint pairs are
   `unknown` / `unsupported-resolution`.
4. **`undeclared-edge` scope in v1**: only project→project edges (`package_dep`, `mcp_call`,
   `declared`) are checked; edges to contract *nodes* are skipped (contract-sharing
   undeclared detection is v1.1). Both endpoint projects must host ≥1 modelled component
   (spec D5). Self-edges never fire.

Two more report-shape decisions:

- An element whose expected edge is machine-checkable but absent has verdict `unknown` with
  `reason: null` — the actionable detail lives in the `missing-required-edge` finding (spec
  D4 keeps the reason list for cannot-be-observed cases only).
- Active exceptions do not delete findings; findings carry `suppressed_by: <EX-id>` and the
  element carries `waived_by: <EX-id>`. Suppressed findings and waived elements are excluded
  from exit-code computation but stay in the report ("no silent truncation"). An
  `expired-waiver` finding always contributes exit 1 (spec D5: violation on the exception).

## File Structure

- Create: `prograph/conformance/__init__.py` — public re-exports.
- Create: `prograph/conformance/manifest.py` — schema models, `load_manifest`, `parse_rule`,
  `ManifestError`.
- Create: `prograph/conformance/engine.py` — `ObservedEdge`, `ObservedGraph`,
  `load_observed`, `evaluate`, `exit_code`, result dataclasses.
- Create: `prograph/conformance/report.py` — `report_payload`, `render_json`, `render_text`.
- Modify: `prograph/config.py` — add `read_intended_path`.
- Modify: `prograph/cli.py` — add `conformance` command.
- Create: `tests/unit/test_conformance_manifest.py`, `tests/unit/test_conformance_engine.py`,
  `tests/unit/test_conformance_report.py`.
- Create: `tests/fixtures/monorepo_conformance/` (+ `golden/conformance.json`),
  `tests/fixtures/ws005_manifest/` (vendored pinned copy).
- Create: `tests/integration/test_cli_conformance.py`.
- Modify: `CLAUDE.md`, `TODO.md` (final task).

---

### Task 1: Manifest schema + strict YAML loader

**Files:**
- Create: `prograph/conformance/__init__.py`
- Create: `prograph/conformance/manifest.py`
- Test: `tests/unit/test_conformance_manifest.py`

**Interfaces:**
- Consumes: nothing (leaf module).
- Produces (used by Tasks 3–9):
  - `SCHEMA_ID = "intended-graph/v1"`, `FILE_PREFIX = "file:"`
  - `class ManifestError(Exception)`
  - pydantic models `Component`, `Interface`, `Constraint`, `ExceptionEntry`,
    `IntendedManifest` (fields exactly as coded below; `IntendedManifest.schema_` aliases
    YAML key `schema`)
  - `def load_manifest(path: Path) -> IntendedManifest` — raises `ManifestError` on any
    defect (unreadable, bad YAML, wrong `schema:`, extra keys at any level, duplicate ids,
    dangling references, both-file interfaces, unparseable rules)
  - `@dataclass(frozen=True) class ForbiddenRule: src: str; dst: str`
  - `def parse_rule(rule: str) -> ForbiddenRule` — raises `ManifestError`

- [ ] **Step 1: Add dependencies**

```sh
uv add "pyyaml>=6,<7"
uv add --dev types-pyyaml
```

- [ ] **Step 2: Write the failing tests**

`tests/unit/test_conformance_manifest.py`:

```python
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
    bad = VALID.replace("consumer: alpha.api\n    protocol: \"python package\"",
                        "consumer: no.such\n    protocol: \"python package\"")
    with pytest.raises(ManifestError, match="unknown component"):
        load_manifest(_write(tmp_path, bad))


def test_interface_with_two_file_endpoints_rejected(tmp_path: Path) -> None:
    bad = VALID.replace("producer: beta.lib", 'producer: "file:a.txt"').replace(
        "consumer: alpha.api\n    protocol: \"python package\"",
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `uv run pytest tests/unit/test_conformance_manifest.py -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'prograph.conformance'`

- [ ] **Step 4: Implement**

`prograph/conformance/__init__.py`:

```python
"""Intended-graph v1 conformance checking (spec 2026-08-03)."""

from prograph.conformance.manifest import (
    FILE_PREFIX,
    SCHEMA_ID,
    Component,
    Constraint,
    ExceptionEntry,
    ForbiddenRule,
    IntendedManifest,
    Interface,
    ManifestError,
    load_manifest,
    parse_rule,
)

__all__ = [
    "FILE_PREFIX",
    "SCHEMA_ID",
    "Component",
    "Constraint",
    "ExceptionEntry",
    "ForbiddenRule",
    "IntendedManifest",
    "Interface",
    "ManifestError",
    "load_manifest",
    "parse_rule",
]
```

`prograph/conformance/manifest.py`:

```python
"""Intended-graph/v1 manifest: pydantic schema + strict YAML loader (spec D1, D6, Schema v1)."""

from __future__ import annotations

import datetime as dt
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Literal

import yaml
from pydantic import BaseModel, ConfigDict, Field, ValidationError

SCHEMA_ID = "intended-graph/v1"
FILE_PREFIX = "file:"

Detector = Literal["import", "mcp", "contract", "declared", "manual-evidence"]

_RULE_RE = re.compile(r"forbidden:\s*(\S+)\s*->\s*(\S+)")


class ManifestError(Exception):
    """Manifest unreadable, malformed, or violating intended-graph/v1."""


class Component(BaseModel):
    model_config = ConfigDict(extra="forbid")

    id: str
    project: str
    kind: Literal["service", "module", "cli", "ui", "contract", "store"]
    owner: str
    responsibility: str
    scope: str | None = None
    evidence: list[str] = Field(default_factory=list)


class Interface(BaseModel):
    model_config = ConfigDict(extra="forbid")

    id: str
    producer: str
    consumer: str
    detector: Detector
    protocol: str | None = None
    evidence: list[str] = Field(default_factory=list)


class Constraint(BaseModel):
    model_config = ConfigDict(extra="forbid")

    id: str
    rule: str
    detector: Detector
    evidence: list[str] = Field(default_factory=list)


class ExceptionEntry(BaseModel):
    model_config = ConfigDict(extra="forbid")

    id: str
    target: str
    reason: str
    owner: str
    expires: dt.date


class IntendedManifest(BaseModel):
    model_config = ConfigDict(extra="forbid")

    schema_: str = Field(alias="schema")
    system: str
    components: list[Component] = Field(min_length=1)
    interfaces: list[Interface] = Field(default_factory=list)
    constraints: list[Constraint] = Field(default_factory=list)
    resources: list[str] = Field(default_factory=list)
    exceptions: list[ExceptionEntry] = Field(default_factory=list)


@dataclass(frozen=True)
class ForbiddenRule:
    """Parsed `forbidden: <src> -> <dst>` rule (spec D6)."""

    src: str
    dst: str


def parse_rule(rule: str) -> ForbiddenRule:
    """Parse a mechanical constraint rule. Raises ManifestError on any other shape."""
    m = _RULE_RE.fullmatch(rule.strip())
    if m is None:
        raise ManifestError(f"unparseable constraint rule: {rule!r}")
    return ForbiddenRule(src=m.group(1), dst=m.group(2))


def load_manifest(path: Path) -> IntendedManifest:
    """Load + strictly validate an intended-graph/v1 manifest (spec: drift must be loud)."""
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as exc:
        raise ManifestError(f"cannot read manifest {path}: {exc}") from exc
    try:
        raw = yaml.safe_load(text)
    except yaml.YAMLError as exc:
        raise ManifestError(f"invalid YAML in {path}: {exc}") from exc
    if not isinstance(raw, dict):
        raise ManifestError("manifest root must be a mapping")
    if raw.get("schema") != SCHEMA_ID:
        raise ManifestError(f"unknown schema: {raw.get('schema')!r} (expected {SCHEMA_ID!r})")
    try:
        manifest = IntendedManifest.model_validate(raw)
    except ValidationError as exc:
        raise ManifestError(f"schema validation failed: {exc}") from exc
    _check_integrity(manifest)
    return manifest


def _check_integrity(manifest: IntendedManifest) -> None:
    seen: set[str] = set()
    for element_id in (
        [c.id for c in manifest.components]
        + [i.id for i in manifest.interfaces]
        + [c.id for c in manifest.constraints]
        + [e.id for e in manifest.exceptions]
    ):
        if element_id in seen:
            raise ManifestError(f"duplicate id: {element_id!r}")
        seen.add(element_id)

    component_ids = {c.id for c in manifest.components}
    for iface in manifest.interfaces:
        endpoints = (iface.producer, iface.consumer)
        for endpoint in endpoints:
            if not endpoint.startswith(FILE_PREFIX) and endpoint not in component_ids:
                raise ManifestError(f"{iface.id}: unknown component {endpoint!r}")
        if all(e.startswith(FILE_PREFIX) for e in endpoints):
            raise ManifestError(f"{iface.id}: at least one component endpoint required")

    for constraint in manifest.constraints:
        if constraint.detector != "manual-evidence":
            parse_rule(constraint.rule)  # raises ManifestError when unparseable

    element_ids = {i.id for i in manifest.interfaces} | {c.id for c in manifest.constraints}
    for exc_entry in manifest.exceptions:
        if exc_entry.target not in element_ids:
            raise ManifestError(f"{exc_entry.id}: unknown element {exc_entry.target!r}")
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `uv run pytest tests/unit/test_conformance_manifest.py -v`
Expected: all PASS

- [ ] **Step 6: Format, lint, typecheck, commit**

```sh
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/conformance tests/unit/test_conformance_manifest.py pyproject.toml uv.lock
git commit -m "feat(conformance): intended-graph/v1 schema + strict loader"
```

---

### Task 2: `[tool.prograph] intended` reader in config.py

**Files:**
- Modify: `prograph/config.py`
- Test: `tests/unit/test_conformance_manifest.py` (append)

**Interfaces:**
- Produces (used by Task 8): `def read_intended_path(pyproject_path: Path) -> str | None` —
  returns `[tool.prograph] intended` from a *project's* `pyproject.toml`, or `None` on
  missing file / section / key / non-string value / malformed TOML (tolerant, like the other
  readers in `config.py`; the caller falls back to `spec/intended-graph.yaml`).

- [ ] **Step 1: Write the failing tests** (append to `tests/unit/test_conformance_manifest.py`)

```python
from prograph.config import read_intended_path


def test_read_intended_path(tmp_path: Path) -> None:
    py = tmp_path / "pyproject.toml"
    py.write_text(
        '[project]\nname = "x"\n\n[tool.prograph]\nintended = "arch/graph.yaml"\n',
        encoding="utf-8",
    )
    assert read_intended_path(py) == "arch/graph.yaml"


def test_read_intended_path_absent(tmp_path: Path) -> None:
    py = tmp_path / "pyproject.toml"
    py.write_text('[project]\nname = "x"\n', encoding="utf-8")
    assert read_intended_path(py) is None
    assert read_intended_path(tmp_path / "missing.toml") is None


def test_read_intended_path_malformed(tmp_path: Path) -> None:
    py = tmp_path / "pyproject.toml"
    py.write_text("not [ toml", encoding="utf-8")
    assert read_intended_path(py) is None
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/unit/test_conformance_manifest.py -k intended_path -v`
Expected: FAIL — `ImportError: cannot import name 'read_intended_path'`

- [ ] **Step 3: Implement** (append to `prograph/config.py`, matching its house style)

```python
def read_intended_path(pyproject_path: Path) -> str | None:
    """Return `[tool.prograph] intended` from a project's pyproject.toml, or None.

    Tolerant of a missing file, missing section/key, malformed TOML, or a
    non-string value — all yield None so callers fall back to the default
    manifest path (`spec/intended-graph.yaml`, spec D1).
    """
    if not pyproject_path.is_file():
        return None
    try:
        data = tomllib.loads(pyproject_path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return None
    tool = data.get("tool")
    if not isinstance(tool, dict):
        return None
    section = tool.get("prograph")
    if not isinstance(section, dict):
        return None
    value = section.get("intended")
    return value if isinstance(value, str) else None
```

- [ ] **Step 4: Run tests, format, typecheck, commit**

```sh
uv run pytest tests/unit/test_conformance_manifest.py -v
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/config.py tests/unit/test_conformance_manifest.py
git commit -m "feat(conformance): read [tool.prograph] intended manifest override"
```

---

### Task 3: Engine data types + observed-graph adapter

**Files:**
- Create: `prograph/conformance/engine.py`
- Test: `tests/unit/test_conformance_engine.py`

**Interfaces:**
- Consumes: `prograph._core` query helpers (`monorepo_overview`, `find_edges_filtered`,
  `project_by_name`, `describe_project`); Task 1 models.
- Produces (used by Tasks 4–8):

```python
@dataclass(frozen=True)
class ObservedEdge:
    kind: str            # "package_dep" | "mcp_call" | "contract_link" | "declared"
    from_name: str       # always a project name
    to_kind: str         # "project" | "contract"
    to_name: str         # project name or contract node name
    path: str | None     # declared edges: attrs["path"]; else None
    mode: str | None     # declared edges: attrs["mode"] ("read"|"write"); else None

@dataclass(frozen=True)
class ObservedGraph:
    projects: frozenset[str]                     # indexed project names
    edges: tuple[ObservedEdge, ...]
    project_paths: Mapping[str, frozenset[str]]  # known rel paths per project;
                                                 # empty set = nothing known (skip scope check)

def load_observed(db_path: str) -> ObservedGraph | None   # None when no snapshot exists
```

- [ ] **Step 1: Write the failing tests**

`tests/unit/test_conformance_engine.py` (this file grows through Tasks 4–6; start it with the
shared builders):

```python
"""Conformance engine: verdicts + findings over a synthetic observed graph."""

import datetime as dt

from prograph.conformance.engine import ObservedEdge, ObservedGraph

TODAY = dt.date(2026, 8, 3)


def graph(
    *edges: ObservedEdge,
    projects: tuple[str, ...] = ("alpha", "beta", "gamma"),
    project_paths: dict[str, frozenset[str]] | None = None,
) -> ObservedGraph:
    return ObservedGraph(
        projects=frozenset(projects),
        edges=edges,
        project_paths=project_paths or {},
    )


def dep(src: str, dst: str) -> ObservedEdge:
    return ObservedEdge(
        kind="package_dep", from_name=src, to_kind="project", to_name=dst, path=None, mode=None
    )


def mcp(src: str, dst: str) -> ObservedEdge:
    return ObservedEdge(
        kind="mcp_call", from_name=src, to_kind="project", to_name=dst, path=None, mode=None
    )


def contract(src: str, node: str) -> ObservedEdge:
    return ObservedEdge(
        kind="contract_link", from_name=src, to_kind="contract", to_name=node,
        path=None, mode=None,
    )


def declared(src: str, dst: str, path: str, mode: str = "read") -> ObservedEdge:
    return ObservedEdge(
        kind="declared", from_name=src, to_kind="project", to_name=dst, path=path, mode=mode
    )


def test_observed_edge_is_hashable() -> None:
    assert len({dep("a", "b"), dep("a", "b"), dep("a", "c")}) == 2
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/unit/test_conformance_engine.py -v`
Expected: FAIL — no module `prograph.conformance.engine`

- [ ] **Step 3: Implement the data types + adapter**

`prograph/conformance/engine.py` (start; `evaluate` arrives in Tasks 4–6):

```python
"""Conformance engine: three-valued verdicts over the observed edge store (spec D2–D5)."""

from __future__ import annotations

import json
from collections.abc import Mapping
from dataclasses import dataclass

from prograph import _core

# Spec D4: closed verdict set.
VERDICT_CONFORMANT = "conformant"
VERDICT_VIOLATION = "violation"
VERDICT_UNKNOWN = "unknown"

# Spec D4: machine-readable unknown reasons.
REASON_MANUAL = "manual-evidence"
REASON_UNSUPPORTED = "unsupported-resolution"
REASON_OUTSIDE = "outside-workspace"
REASON_ORPHAN = "orphan-component"

# Spec D5: finding taxonomy v1 — exact identifiers, also the --fail-on vocabulary (D7).
FINDING_CLASSES = (
    "missing-required-edge",
    "forbidden-edge",
    "undeclared-edge",
    "orphan-component",
    "expired-waiver",
    "manual-obligation",
)

# Spec D3: detector vocabulary maps onto existing EdgeKind strings.
DETECTOR_TO_KIND = {
    "import": "package_dep",
    "mcp": "mcp_call",
    "contract": "contract_link",
    "declared": "declared",
}


@dataclass(frozen=True)
class ObservedEdge:
    """One edge from the descriptive plane, flattened for matching."""

    kind: str
    from_name: str
    to_kind: str
    to_name: str
    path: str | None
    mode: str | None


@dataclass(frozen=True)
class ObservedGraph:
    """Everything the engine needs from a snapshot — data only, no DB handle."""

    projects: frozenset[str]
    edges: tuple[ObservedEdge, ...]
    project_paths: Mapping[str, frozenset[str]]


def load_observed(db_path: str) -> ObservedGraph | None:
    """Flatten the latest snapshot into an ObservedGraph. None when no snapshot."""
    overview = _core.monorepo_overview(db_path)
    if overview is None:
        return None
    names = [p.name for p in overview.projects]

    edges: list[ObservedEdge] = []
    for row in _core.find_edges_filtered(db_path):
        path: str | None = None
        mode: str | None = None
        if row.kind == "declared":
            try:
                attrs = json.loads(row.attrs_json)
            except json.JSONDecodeError:
                attrs = {}
            raw_path = attrs.get("path")
            raw_mode = attrs.get("mode")
            path = raw_path if isinstance(raw_path, str) else None
            mode = raw_mode if isinstance(raw_mode, str) else None
        edges.append(
            ObservedEdge(
                kind=row.kind,
                from_name=row.from_name,
                to_kind=row.to_kind,
                to_name=row.to_name,
                path=path,
                mode=mode,
            )
        )

    project_paths: dict[str, frozenset[str]] = {}
    for name in names:
        pid = _core.project_by_name(db_path, name)
        desc = _core.describe_project(db_path, pid) if pid is not None else None
        if desc is None:
            project_paths[name] = frozenset()
            continue
        paths = {m.rel_path for m in desc.modules}
        paths.update(c.rel_path for c in desc.contract_files)
        project_paths[name] = frozenset(paths)

    return ObservedGraph(
        projects=frozenset(names), edges=tuple(edges), project_paths=project_paths
    )
```

- [ ] **Step 4: Run tests, format, typecheck, commit**

```sh
uv run pytest tests/unit/test_conformance_engine.py -v
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/conformance/engine.py tests/unit/test_conformance_engine.py
git commit -m "feat(conformance): observed-graph adapter over the edge store"
```

---

### Task 4: Interface verdicts

**Files:**
- Modify: `prograph/conformance/engine.py`
- Test: `tests/unit/test_conformance_engine.py` (append)

**Interfaces:**
- Consumes: Task 1 models, Task 3 types.
- Produces (used by Tasks 6–8):

```python
@dataclass(frozen=True)
class Finding:
    finding_class: str        # one of FINDING_CLASSES
    element: str | None       # element id; None for undeclared-edge (attaches to manifest)
    detail: str
    suppressed_by: str | None = None

@dataclass(frozen=True)
class ElementResult:
    id: str
    element_type: str         # "interface" | "constraint"
    detector: str
    verdict: str
    reason: str | None
    waived_by: str | None = None

# internal, used by evaluate() in Task 6:
def _interface_result(
    iface: Interface,
    comp_by_id: dict[str, Component],
    observed: ObservedGraph,
) -> tuple[ElementResult, Finding | None, ObservedEdge | None]
```

The third return element is the matched edge (for Task 6's undeclared-edge coverage set).

- [ ] **Step 1: Write the failing tests** (append; imports extend the existing header)

```python
from prograph.conformance.engine import evaluate  # noqa: E402  (single import point)
from prograph.conformance.manifest import IntendedManifest  # noqa: E402


def manifest(**overrides: object) -> IntendedManifest:
    base: dict[str, object] = {
        "schema": "intended-graph/v1",
        "system": "t",
        "components": [
            {"id": "alpha.api", "project": "alpha", "kind": "service",
             "owner": "architects", "responsibility": "api"},
            {"id": "alpha.worker", "project": "alpha", "kind": "module",
             "owner": "architects", "responsibility": "worker"},
            {"id": "beta.lib", "project": "beta", "kind": "module",
             "owner": "architects", "responsibility": "lib"},
            {"id": "gamma.reader", "project": "gamma", "kind": "cli",
             "owner": "architects", "responsibility": "reader"},
        ],
    }
    base.update(overrides)
    return IntendedManifest.model_validate(base)


def _by_id(report, element_id):  # noqa: ANN001, ANN202 — test helper
    return next(e for e in report.elements if e.id == element_id)


def iface(id_: str, producer: str, consumer: str, detector: str) -> dict[str, str]:
    return {"id": id_, "producer": producer, "consumer": consumer, "detector": detector}


def test_import_interface_conformant() -> None:
    m = manifest(interfaces=[iface("I-01", "beta.lib", "alpha.api", "import")])
    report = evaluate(m, graph(dep("alpha", "beta")), TODAY)
    el = _by_id(report, "I-01")
    assert (el.verdict, el.reason) == ("conformant", None)
    assert report.findings == ()


def test_import_interface_missing_edge() -> None:
    m = manifest(interfaces=[iface("I-01", "beta.lib", "alpha.api", "import")])
    report = evaluate(m, graph(), TODAY)
    el = _by_id(report, "I-01")
    assert (el.verdict, el.reason) == ("unknown", None)
    assert [f.finding_class for f in report.findings] == ["missing-required-edge"]
    assert report.findings[0].element == "I-01"


def test_mcp_interface_conformant() -> None:
    m = manifest(interfaces=[iface("I-01", "alpha.api", "gamma.reader", "mcp")])
    report = evaluate(m, graph(mcp("gamma", "alpha")), TODAY)
    assert _by_id(report, "I-01").verdict == "conformant"


def test_contract_interface_needs_shared_node() -> None:
    m = manifest(interfaces=[iface("I-01", "beta.lib", "gamma.reader", "contract")])
    both = graph(contract("beta", "feed-v1"), contract("gamma", "feed-v1"))
    only_one = graph(contract("beta", "feed-v1"), contract("gamma", "other-v1"))
    assert _by_id(evaluate(m, both, TODAY), "I-01").verdict == "conformant"
    assert _by_id(evaluate(m, only_one, TODAY), "I-01").verdict == "unknown"


def test_declared_interface_file_producer() -> None:
    # gamma.reader consumes a file published by beta → gamma declares the read.
    m = manifest(
        interfaces=[iface("I-02", "file:beta/data/feed.txt", "gamma.reader", "declared")]
    )
    ok = graph(declared("gamma", "beta", "beta/data/feed.txt", "read"))
    wrong_mode = graph(declared("gamma", "beta", "beta/data/feed.txt", "write"))
    assert _by_id(evaluate(m, ok, TODAY), "I-02").verdict == "conformant"
    assert _by_id(evaluate(m, wrong_mode, TODAY), "I-02").verdict == "unknown"


def test_declared_file_path_segment_suffix_matches() -> None:
    # Manifest writes a repo-generic path; the edge carries the workspace-relative one.
    m = manifest(
        interfaces=[iface("I-02", "file:.steward/gv.jsonl", "gamma.reader", "declared")]
    )
    ok = graph(declared("gamma", "beta", "beta/.steward/gv.jsonl", "read"))
    assert _by_id(evaluate(m, ok, TODAY), "I-02").verdict == "conformant"


def test_declared_interface_file_consumer_requires_write() -> None:
    m = manifest(
        interfaces=[iface("I-01", "gamma.reader", "file:beta/out.txt", "declared")]
    )
    ok = graph(declared("gamma", "beta", "beta/out.txt", "write"))
    assert _by_id(evaluate(m, ok, TODAY), "I-01").verdict == "conformant"


def test_same_project_pair_is_unsupported_resolution() -> None:
    m = manifest(interfaces=[iface("I-04", "alpha.api", "alpha.worker", "import")])
    report = evaluate(m, graph(dep("alpha", "alpha")), TODAY)
    el = _by_id(report, "I-04")
    assert (el.verdict, el.reason) == ("unknown", "unsupported-resolution")
    assert report.findings == ()


def test_manual_evidence_interface_is_manual_obligation() -> None:
    m = manifest(
        interfaces=[iface("I-09", "alpha.api", "gamma.reader", "manual-evidence")]
    )
    report = evaluate(m, graph(), TODAY)
    el = _by_id(report, "I-09")
    assert (el.verdict, el.reason) == ("unknown", "manual-evidence")
    assert [f.finding_class for f in report.findings] == ["manual-obligation"]


def test_project_outside_workspace() -> None:
    m = manifest(
        components=[
            {"id": "delta.ghost", "project": "delta", "kind": "service",
             "owner": "architects", "responsibility": "ghost"},
            {"id": "beta.lib", "project": "beta", "kind": "module",
             "owner": "architects", "responsibility": "lib"},
        ],
        interfaces=[iface("I-06", "beta.lib", "delta.ghost", "import")],
    )
    report = evaluate(m, graph(), TODAY)
    el = _by_id(report, "I-06")
    assert (el.verdict, el.reason) == ("unknown", "outside-workspace")
    assert [f.finding_class for f in report.findings] == ["orphan-component"]


def test_scope_matching_nothing_is_orphan() -> None:
    m = manifest(
        components=[
            {"id": "beta.ghost", "project": "beta", "kind": "module", "owner": "architects",
             "responsibility": "ghost", "scope": "no/such/dir"},
            {"id": "gamma.reader", "project": "gamma", "kind": "cli",
             "owner": "architects", "responsibility": "reader"},
        ],
        interfaces=[iface("I-07", "beta.ghost", "gamma.reader", "import")],
    )
    paths = {"beta": frozenset({"beta/__init__.py"})}
    report = evaluate(m, graph(project_paths=paths), TODAY)
    el = _by_id(report, "I-07")
    assert (el.verdict, el.reason) == ("unknown", "orphan-component")
    assert [f.finding_class for f in report.findings] == ["orphan-component"]


def test_scope_check_skipped_when_no_paths_known() -> None:
    # No module/contract facts for beta → cannot falsify the scope → not an orphan.
    m = manifest(
        components=[
            {"id": "beta.ghost", "project": "beta", "kind": "module", "owner": "architects",
             "responsibility": "ghost", "scope": "no/such/dir"},
            {"id": "gamma.reader", "project": "gamma", "kind": "cli",
             "owner": "architects", "responsibility": "reader"},
        ],
        interfaces=[iface("I-07", "beta.ghost", "gamma.reader", "import")],
    )
    report = evaluate(m, graph(dep("gamma", "beta")), TODAY)
    assert _by_id(report, "I-07").verdict == "conformant"
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/unit/test_conformance_engine.py -v`
Expected: FAIL — `ImportError: cannot import name 'evaluate'`

- [ ] **Step 3: Implement** (append to `engine.py`; `evaluate` here handles interfaces only —
constraints/undeclared/exceptions raise `NotImplementedError` paths are NOT stubbed, instead
`evaluate` simply processes what exists: manifests without constraints/exceptions work now,
Task 5–6 extend the same function)

```python
@dataclass(frozen=True)
class Finding:
    """One report entry (spec D5). `element` is None for undeclared-edge."""

    finding_class: str
    element: str | None
    detail: str
    suppressed_by: str | None = None


@dataclass(frozen=True)
class ElementResult:
    """Verdict for one intended element (spec D4)."""

    id: str
    element_type: str
    detector: str
    verdict: str
    reason: str | None
    waived_by: str | None = None


@dataclass(frozen=True)
class ExceptionStatus:
    id: str
    target: str
    expires: str
    status: str  # "active" | "expired"


@dataclass(frozen=True)
class ConformanceReport:
    system: str
    elements: tuple[ElementResult, ...]
    findings: tuple[Finding, ...]
    exceptions: tuple[ExceptionStatus, ...]


@dataclass(frozen=True)
class _FileEndpoint:
    path: str


def _path_matches(file_path: str, edge_path: str | None) -> bool:
    """Plan rule 1: exact or segment-suffix match, './' normalized off."""
    if edge_path is None:
        return False
    p = file_path.removeprefix("./")
    return edge_path == p or edge_path.endswith("/" + p)


def _component_gap(
    comp: Component, observed: ObservedGraph
) -> tuple[str, Finding] | None:
    """Orphan checks (spec D4/D5). Returns (unknown-reason, finding) or None."""
    if comp.project not in observed.projects:
        return (
            REASON_OUTSIDE,
            Finding(
                "orphan-component",
                comp.id,
                f"project {comp.project!r} is not in the indexed workspace",
            ),
        )
    known = observed.project_paths.get(comp.project) or frozenset()
    if comp.scope and known:
        prefix = comp.scope.rstrip("/") + "/"
        if not any(p == comp.scope or p.startswith(prefix) for p in known):
            return (
                REASON_ORPHAN,
                Finding(
                    "orphan-component",
                    comp.id,
                    f"scope {comp.scope!r} matches nothing indexed in {comp.project!r}",
                ),
            )
    return None


def _match_interface(
    detector: str,
    producer: Component | _FileEndpoint,
    consumer: Component | _FileEndpoint,
    observed: ObservedGraph,
) -> ObservedEdge | None:
    """Return the first observed edge satisfying the interface, else None."""
    if detector == "contract":
        assert isinstance(producer, Component) and isinstance(consumer, Component)
        producer_nodes = {
            e.to_name
            for e in observed.edges
            if e.kind == "contract_link" and e.from_name == producer.project
        }
        for e in observed.edges:
            if (
                e.kind == "contract_link"
                and e.from_name == consumer.project
                and e.to_name in producer_nodes
            ):
                return e
        return None

    kind = DETECTOR_TO_KIND[detector]
    if detector in ("import", "mcp"):
        assert isinstance(producer, Component) and isinstance(consumer, Component)
        for e in observed.edges:
            if (
                e.kind == kind
                and e.from_name == consumer.project
                and e.to_kind == "project"
                and e.to_name == producer.project
            ):
                return e
        return None

    # declared
    if isinstance(producer, _FileEndpoint):
        assert isinstance(consumer, Component)
        for e in observed.edges:
            if (
                e.kind == "declared"
                and e.mode == "read"
                and e.from_name == consumer.project
                and _path_matches(producer.path, e.path)
            ):
                return e
        return None
    if isinstance(consumer, _FileEndpoint):
        for e in observed.edges:
            if (
                e.kind == "declared"
                and e.mode == "write"
                and e.from_name == producer.project
                and _path_matches(consumer.path, e.path)
            ):
                return e
        return None
    # component -> component: consumer reads producer's files, or producer writes into
    # consumer's; honor the producer's scope as a workspace-relative path prefix when set.
    scope_prefix = (
        f"{producer.project}/{producer.scope.rstrip('/')}/" if producer.scope else None
    )

    def _scope_ok(e: ObservedEdge) -> bool:
        if scope_prefix is None or e.path is None:
            return True
        return e.path.startswith(scope_prefix) or e.path == scope_prefix.rstrip("/")

    for e in observed.edges:
        if e.kind != "declared" or not _scope_ok(e):
            continue
        reads = (
            e.mode == "read"
            and e.from_name == consumer.project
            and e.to_name == producer.project
        )
        writes = (
            e.mode == "write"
            and e.from_name == producer.project
            and e.to_name == consumer.project
        )
        if reads or writes:
            return e
    return None


def _interface_result(
    iface: Interface,
    comp_by_id: dict[str, Component],
    observed: ObservedGraph,
) -> tuple[ElementResult, Finding | None, ObservedEdge | None]:
    def result(verdict: str, reason: str | None) -> ElementResult:
        return ElementResult(
            id=iface.id,
            element_type="interface",
            detector=iface.detector,
            verdict=verdict,
            reason=reason,
        )

    if iface.detector == "manual-evidence":
        finding = Finding(
            "manual-obligation",
            iface.id,
            "manual-evidence element: verify by review, restated in every report",
        )
        return result(VERDICT_UNKNOWN, REASON_MANUAL), finding, None

    endpoints: list[Component | _FileEndpoint] = []
    for raw in (iface.producer, iface.consumer):
        if raw.startswith(FILE_PREFIX):
            endpoints.append(_FileEndpoint(path=raw.removeprefix(FILE_PREFIX)))
        else:
            endpoints.append(comp_by_id[raw])
    producer, consumer = endpoints

    for endpoint in endpoints:
        if isinstance(endpoint, Component):
            gap = _component_gap(endpoint, observed)
            if gap is not None:
                reason, finding = gap
                return result(VERDICT_UNKNOWN, reason), finding, None

    if (
        isinstance(producer, Component)
        and isinstance(consumer, Component)
        and producer.project == consumer.project
    ):
        return result(VERDICT_UNKNOWN, REASON_UNSUPPORTED), None, None

    matched = _match_interface(iface.detector, producer, consumer, observed)
    if matched is not None:
        return result(VERDICT_CONFORMANT, None), None, matched
    finding = Finding(
        "missing-required-edge",
        iface.id,
        f"detector {iface.detector!r} observed no edge for "
        f"{iface.producer} -> {iface.consumer}",
    )
    return result(VERDICT_UNKNOWN, None), finding, None


def evaluate(
    manifest: IntendedManifest, observed: ObservedGraph, today: "dt.date"
) -> ConformanceReport:
    """Evaluate every intended element against the observed graph (spec D4/D5)."""
    comp_by_id = {c.id: c for c in manifest.components}
    elements: list[ElementResult] = []
    findings: list[Finding] = []
    covered: set[ObservedEdge] = set()

    for iface in manifest.interfaces:
        element, finding, matched = _interface_result(iface, comp_by_id, observed)
        elements.append(element)
        if finding is not None:
            findings.append(finding)
        if matched is not None:
            covered.add(matched)

    # Constraints (Task 5), undeclared-edge + exceptions (Task 6) extend this function.

    findings.sort(key=lambda f: (f.finding_class, f.element or "", f.detail))
    return ConformanceReport(
        system=manifest.system,
        elements=tuple(elements),
        findings=tuple(findings),
        exceptions=(),
    )
```

Add the needed imports at the top of `engine.py`:

```python
import datetime as dt

from prograph.conformance.manifest import (
    FILE_PREFIX,
    Component,
    Constraint,
    IntendedManifest,
    Interface,
    parse_rule,
)
```

(`Constraint` and `parse_rule` are consumed in Task 5 — importing them now avoids an
import-shuffle diff later; if ruff flags them unused at this commit, add them in Task 5
instead.)

- [ ] **Step 4: Run tests, format, typecheck, commit**

```sh
uv run pytest tests/unit/test_conformance_engine.py tests/unit/test_conformance_manifest.py -v
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/conformance/engine.py tests/unit/test_conformance_engine.py
git commit -m "feat(conformance): interface verdicts (import/mcp/contract/declared)"
```

---

### Task 5: Constraint verdicts

**Files:**
- Modify: `prograph/conformance/engine.py`
- Test: `tests/unit/test_conformance_engine.py` (append)

**Interfaces:**
- Consumes: Task 4's `ElementResult`/`Finding`, `parse_rule`, plan rule 2 (attribution).
- Produces: `_constraint_result(constraint, comp_by_id, project_component_count, observed)
  -> tuple[ElementResult, Finding | None]`; `evaluate` now processes `manifest.constraints`.

- [ ] **Step 1: Write the failing tests** (append)

```python
def constraint(id_: str, rule: str, detector: str) -> dict[str, str]:
    return {"id": id_, "rule": rule, "detector": detector}


def test_forbidden_project_pair_violation() -> None:
    m = manifest(constraints=[constraint("C-1", "forbidden: gamma -> alpha", "import")])
    report = evaluate(m, graph(dep("gamma", "alpha")), TODAY)
    el = _by_id(report, "C-1")
    assert (el.verdict, el.reason) == ("violation", None)
    assert [f.finding_class for f in report.findings] == ["forbidden-edge"]
    assert report.findings[0].element == "C-1"


def test_forbidden_project_pair_conformant_when_absent() -> None:
    m = manifest(constraints=[constraint("C-1", "forbidden: gamma -> alpha", "import")])
    report = evaluate(m, graph(dep("alpha", "beta")), TODAY)
    assert _by_id(report, "C-1").verdict == "conformant"


def test_forbidden_glob_endpoint() -> None:
    m = manifest(constraints=[constraint("C-2", "forbidden: gamma -> *", "import")])
    report = evaluate(m, graph(dep("gamma", "beta")), TODAY)
    assert _by_id(report, "C-2").verdict == "violation"


def test_forbidden_kind_filter_respected() -> None:
    # The dep edge exists but the constraint watches mcp edges only.
    m = manifest(constraints=[constraint("C-3", "forbidden: gamma -> alpha", "mcp")])
    report = evaluate(m, graph(dep("gamma", "alpha")), TODAY)
    assert _by_id(report, "C-3").verdict == "conformant"


def test_component_endpoint_sole_in_project_is_attributable() -> None:
    # gamma.reader is gamma's only modelled component → project granularity is exact.
    m = manifest(
        constraints=[constraint("C-4", "forbidden: gamma.reader -> beta", "import")]
    )
    report = evaluate(m, graph(dep("gamma", "beta")), TODAY)
    assert _by_id(report, "C-4").verdict == "violation"


def test_component_endpoint_ambiguous_is_unsupported() -> None:
    # alpha hosts alpha.api AND alpha.worker → attribution needs module-level (v1.1).
    m = manifest(
        constraints=[constraint("C-5", "forbidden: alpha.worker -> beta", "import")]
    )
    report = evaluate(m, graph(dep("alpha", "beta")), TODAY)
    el = _by_id(report, "C-5")
    assert (el.verdict, el.reason) == ("unknown", "unsupported-resolution")
    assert report.findings == ()


def test_file_endpoint_in_rule_matches_declared_path() -> None:
    m = manifest(
        constraints=[
            constraint("C-6", "forbidden: gamma.reader -> file:beta/secret.db", "declared")
        ]
    )
    hit = graph(declared("gamma", "beta", "beta/secret.db", "read"))
    miss = graph(declared("gamma", "beta", "beta/other.db", "read"))
    assert _by_id(evaluate(m, hit, TODAY), "C-6").verdict == "violation"
    assert _by_id(evaluate(m, miss, TODAY), "C-6").verdict == "conformant"


def test_manual_evidence_constraint_is_permanent_unknown() -> None:
    m = manifest(
        constraints=[constraint("C-7", "forbidden: только чтение, без мутаций",
                                "manual-evidence")]
    )
    report = evaluate(m, graph(), TODAY)
    el = _by_id(report, "C-7")
    assert (el.verdict, el.reason) == ("unknown", "manual-evidence")
    assert [f.finding_class for f in report.findings] == ["manual-obligation"]


def test_constraint_component_outside_workspace() -> None:
    m = manifest(
        components=[
            {"id": "delta.ghost", "project": "delta", "kind": "service",
             "owner": "architects", "responsibility": "ghost"},
        ],
        constraints=[constraint("C-8", "forbidden: delta.ghost -> beta", "import")],
    )
    report = evaluate(m, graph(), TODAY)
    el = _by_id(report, "C-8")
    assert (el.verdict, el.reason) == ("unknown", "outside-workspace")
    assert [f.finding_class for f in report.findings] == ["orphan-component"]
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/unit/test_conformance_engine.py -k constraint -v` (plus the new
names) — Expected: FAIL (constraints currently ignored by `evaluate`).

- [ ] **Step 3: Implement** (append to `engine.py`; wire into `evaluate` right after the
interface loop, before the `findings.sort`)

```python
from fnmatch import fnmatch  # move to the top imports


@dataclass(frozen=True)
class _RuleSide:
    kind: str                      # "component" | "project" | "file"
    value: str                     # glob pattern or file path (kind != "component")
    component: Component | None = None


def _resolve_side(raw: str, comp_by_id: dict[str, Component]) -> _RuleSide:
    if raw.startswith(FILE_PREFIX):
        return _RuleSide(kind="file", value=raw.removeprefix(FILE_PREFIX))
    comp = comp_by_id.get(raw)
    if comp is not None:
        return _RuleSide(kind="component", value=comp.project, component=comp)
    return _RuleSide(kind="project", value=raw)


def _side_matches(side: _RuleSide, edge: ObservedEdge, from_side: bool) -> bool:
    if side.kind == "file":
        return edge.kind == "declared" and _path_matches(side.value, edge.path)
    name = edge.from_name if from_side else edge.to_name
    if side.kind == "component":
        return name == side.value
    return fnmatch(name, side.value)


def _constraint_result(
    con: Constraint,
    comp_by_id: dict[str, Component],
    project_component_count: dict[str, int],
    observed: ObservedGraph,
) -> tuple[ElementResult, Finding | None]:
    def result(verdict: str, reason: str | None) -> ElementResult:
        return ElementResult(
            id=con.id,
            element_type="constraint",
            detector=con.detector,
            verdict=verdict,
            reason=reason,
        )

    if con.detector == "manual-evidence":
        finding = Finding(
            "manual-obligation",
            con.id,
            "manual-evidence element: verify by review, restated in every report",
        )
        return result(VERDICT_UNKNOWN, REASON_MANUAL), finding

    rule = parse_rule(con.rule)  # load_manifest already guaranteed this parses
    src = _resolve_side(rule.src, comp_by_id)
    dst = _resolve_side(rule.dst, comp_by_id)

    for side in (src, dst):
        if side.component is not None:
            gap = _component_gap(side.component, observed)
            if gap is not None:
                reason, finding = gap
                # re-anchor the finding on the constraint, not the component
                finding = Finding(finding.finding_class, con.id, finding.detail)
                return result(VERDICT_UNKNOWN, reason), finding

    # Plan rule 2: a component endpoint is attributable only when it is the sole
    # modelled component of its project; otherwise project-granularity evidence
    # cannot pin the edge on it — honest unknown until module-level v1.1.
    for side in (src, dst):
        if side.component is not None and project_component_count[side.component.project] > 1:
            return result(VERDICT_UNKNOWN, REASON_UNSUPPORTED), None

    kind = DETECTOR_TO_KIND[con.detector]
    for e in observed.edges:
        if e.kind != kind:
            continue
        if _side_matches(src, e, from_side=True) and _side_matches(dst, e, from_side=False):
            finding = Finding(
                "forbidden-edge",
                con.id,
                f"observed {e.from_name} -[{e.kind}]-> {e.to_name} matches {con.rule!r}",
            )
            return result(VERDICT_VIOLATION, None), finding
    return result(VERDICT_CONFORMANT, None), None
```

In `evaluate`, after the interface loop:

```python
    project_component_count: dict[str, int] = {}
    for comp in manifest.components:
        project_component_count[comp.project] = (
            project_component_count.get(comp.project, 0) + 1
        )

    for con in manifest.constraints:
        element, finding = _constraint_result(
            con, comp_by_id, project_component_count, observed
        )
        elements.append(element)
        if finding is not None:
            findings.append(finding)
```

- [ ] **Step 4: Run tests, format, typecheck, commit**

```sh
uv run pytest tests/unit/test_conformance_engine.py -v
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/conformance/engine.py tests/unit/test_conformance_engine.py
git commit -m "feat(conformance): forbidden-constraint verdicts with honest attribution"
```

---

### Task 6: undeclared-edge, exceptions, exit codes

**Files:**
- Modify: `prograph/conformance/engine.py`
- Test: `tests/unit/test_conformance_engine.py` (append)

**Interfaces:**
- Consumes: everything above.
- Produces (used by Tasks 7–8):
  - `ConformanceReport.exceptions` now populated (`ExceptionStatus` per manifest entry)
  - `def exit_code(report: ConformanceReport, fail_on: frozenset[str],
    fail_on_verdict: frozenset[str]) -> int` — spec D7 policy: `1` on non-waived violation,
    any `expired-waiver`, non-suppressed finding class in `fail_on`, or non-waived verdict in
    `fail_on_verdict`; else `0`.

- [ ] **Step 1: Write the failing tests** (append)

```python
from prograph.conformance.engine import exit_code  # noqa: E402


def test_undeclared_edge_between_modelled_projects() -> None:
    # gamma -> alpha observed; both projects modelled; no interface covers it.
    m = manifest(interfaces=[iface("I-01", "beta.lib", "alpha.api", "import")])
    report = evaluate(m, graph(dep("alpha", "beta"), dep("gamma", "alpha")), TODAY)
    undeclared = [f for f in report.findings if f.finding_class == "undeclared-edge"]
    assert len(undeclared) == 1
    assert undeclared[0].element is None
    assert "gamma" in undeclared[0].detail and "alpha" in undeclared[0].detail


def test_undeclared_edge_ignores_unmodelled_projects() -> None:
    m = manifest(
        components=[
            {"id": "alpha.api", "project": "alpha", "kind": "service",
             "owner": "architects", "responsibility": "api"},
        ]
    )
    # beta is not modelled → its edges never fire the finding.
    report = evaluate(m, graph(dep("alpha", "beta"), dep("beta", "alpha")), TODAY)
    assert report.findings == ()


def test_undeclared_edge_skips_contract_nodes_and_self_edges() -> None:
    m = manifest()
    report = evaluate(
        m, graph(contract("alpha", "feed-v1"), dep("alpha", "alpha")), TODAY
    )
    assert [f for f in report.findings if f.finding_class == "undeclared-edge"] == []


def test_active_exception_suppresses_and_waives() -> None:
    m = manifest(
        interfaces=[iface("I-05", "alpha.api", "gamma.reader", "mcp")],
        exceptions=[{"id": "EX-01", "target": "I-05", "reason": "next milestone",
                     "owner": "architects", "expires": "2999-01-01"}],
    )
    report = evaluate(m, graph(), TODAY)
    finding = next(f for f in report.findings if f.finding_class == "missing-required-edge")
    assert finding.suppressed_by == "EX-01"
    assert _by_id(report, "I-05").waived_by == "EX-01"
    assert report.exceptions[0].status == "active"
    assert exit_code(report, frozenset({"missing-required-edge"}), frozenset()) == 0


def test_expired_exception_is_a_violation_and_stops_suppressing() -> None:
    m = manifest(
        constraints=[constraint("C-1", "forbidden: gamma -> alpha", "import")],
        exceptions=[{"id": "EX-02", "target": "C-1", "reason": "grandfathered",
                     "owner": "architects", "expires": "2020-01-01"}],
    )
    report = evaluate(m, graph(dep("gamma", "alpha")), TODAY)
    classes = sorted(f.finding_class for f in report.findings)
    assert classes == ["expired-waiver", "forbidden-edge", "undeclared-edge"]
    assert _by_id(report, "C-1").waived_by is None
    assert report.exceptions[0].status == "expired"
    assert exit_code(report, frozenset(), frozenset()) == 1


def test_exit_code_default_policy() -> None:
    # unknowns and report-only findings do not fail a default run (spec D7).
    m = manifest(interfaces=[iface("I-05", "alpha.api", "gamma.reader", "mcp")])
    report = evaluate(m, graph(dep("gamma", "alpha")), TODAY)  # + undeclared gamma->alpha
    assert exit_code(report, frozenset(), frozenset()) == 0
    assert exit_code(report, frozenset({"undeclared-edge"}), frozenset()) == 1
    assert exit_code(report, frozenset(), frozenset({"unknown"})) == 1


def test_violation_always_fails() -> None:
    m = manifest(constraints=[constraint("C-1", "forbidden: gamma -> alpha", "import")])
    report = evaluate(m, graph(dep("gamma", "alpha")), TODAY)
    assert exit_code(report, frozenset(), frozenset()) == 1
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/unit/test_conformance_engine.py -v`
Expected: new tests FAIL (no undeclared/exception handling, no `exit_code`).

- [ ] **Step 3: Implement** (extend `evaluate` between the constraint loop and
`findings.sort`, then add `exit_code`)

```python
    # Spec D5 undeclared-edge, bounded by plan rule 4 (project→project kinds only).
    modelled_projects = {c.project for c in manifest.components}
    for e in observed.edges:
        if e.to_kind != "project" or e.from_name == e.to_name:
            continue
        if e.from_name not in modelled_projects or e.to_name not in modelled_projects:
            continue
        if e in covered:
            continue
        detail = f"observed {e.from_name} -[{e.kind}]-> {e.to_name} appears in no interface"
        if e.path is not None:
            detail += f" (path {e.path!r})"
        findings.append(Finding("undeclared-edge", None, detail))

    # Exceptions: active ones suppress findings + waive their element; expired ones are
    # violations on the exception itself (spec D5) and suppress nothing.
    exception_statuses: list[ExceptionStatus] = []
    active_by_target: dict[str, str] = {}
    for entry in manifest.exceptions:
        expired = entry.expires < today
        exception_statuses.append(
            ExceptionStatus(
                id=entry.id,
                target=entry.target,
                expires=entry.expires.isoformat(),
                status="expired" if expired else "active",
            )
        )
        if expired:
            findings.append(
                Finding(
                    "expired-waiver",
                    entry.id,
                    f"exception on {entry.target} expired {entry.expires.isoformat()}",
                )
            )
        else:
            active_by_target[entry.target] = entry.id

    if active_by_target:
        findings = [
            Finding(
                f.finding_class,
                f.element,
                f.detail,
                suppressed_by=active_by_target.get(f.element or ""),
            )
            for f in findings
        ]
        elements = [
            ElementResult(
                id=el.id,
                element_type=el.element_type,
                detector=el.detector,
                verdict=el.verdict,
                reason=el.reason,
                waived_by=active_by_target.get(el.id),
            )
            for el in elements
        ]
```

(the trailing `findings.sort` + `ConformanceReport(...)` from Task 4 stays, now passing
`exceptions=tuple(exception_statuses)`).

```python
def exit_code(
    report: ConformanceReport,
    fail_on: frozenset[str],
    fail_on_verdict: frozenset[str],
) -> int:
    """Spec D7 exit policy for exit 0 vs 1 (exit 2 is the CLI's tool-error path)."""
    violation = any(
        el.verdict == VERDICT_VIOLATION and el.waived_by is None for el in report.elements
    )
    expired = any(f.finding_class == "expired-waiver" for f in report.findings)
    escalated_finding = any(
        f.suppressed_by is None and f.finding_class in fail_on for f in report.findings
    )
    escalated_verdict = any(
        el.waived_by is None and el.verdict in fail_on_verdict for el in report.elements
    )
    return 1 if (violation or expired or escalated_finding or escalated_verdict) else 0
```

- [ ] **Step 4: Run tests, format, typecheck, commit**

```sh
uv run pytest tests/unit -v
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/conformance/engine.py tests/unit/test_conformance_engine.py
git commit -m "feat(conformance): undeclared-edge, waivers, exit-code policy"
```

---

### Task 7: Report rendering (byte-stable JSON + text)

**Files:**
- Create: `prograph/conformance/report.py`
- Test: `tests/unit/test_conformance_report.py`

**Interfaces:**
- Consumes: `ConformanceReport` and friends from Task 4/6.
- Produces (used by Task 8):

```python
def report_payload(
    report: ConformanceReport, *, manifest_path: str, manifest_sha256: str, snapshot_id: int
) -> dict[str, object]
def render_json(...same args...) -> str   # json.dumps(payload, indent=2, sort_keys=True) + "\n"
def render_text(...same args...) -> str
```

Payload shape (keys are final — the golden test and GC-ARCH-CONFORMANCE consume it):

```json
{
  "schema": "conformance-report/v1",
  "system": "...",
  "manifest": {"path": "...", "sha256": "..."},
  "snapshot": {"id": 1},
  "elements": [{"id": "...", "type": "interface", "detector": "import",
                 "verdict": "conformant", "reason": null, "waived_by": null}],
  "findings": [{"class": "...", "element": "...", "detail": "...", "suppressed_by": null}],
  "exceptions": [{"id": "...", "target": "...", "expires": "2026-09-01", "status": "active"}],
  "summary": {"verdicts": {"conformant": 0, "violation": 0, "unknown": 0},
               "findings": {"missing-required-edge": 0, "forbidden-edge": 0,
                             "undeclared-edge": 0, "orphan-component": 0,
                             "expired-waiver": 0, "manual-obligation": 0}}
}
```

All six finding-class keys are always present in `summary.findings` (zero included) — "no
silent truncation" applies to the summary too.

- [ ] **Step 1: Write the failing tests**

`tests/unit/test_conformance_report.py`:

```python
"""Conformance report rendering: byte-stable JSON + human text."""

import json

from prograph.conformance.engine import (
    ConformanceReport,
    ElementResult,
    ExceptionStatus,
    Finding,
)
from prograph.conformance.report import render_json, render_text, report_payload

REPORT = ConformanceReport(
    system="fixture-feed",
    elements=(
        ElementResult(id="I-01", element_type="interface", detector="import",
                      verdict="conformant", reason=None),
        ElementResult(id="I-05", element_type="interface", detector="mcp",
                      verdict="unknown", reason=None, waived_by="EX-01"),
        ElementResult(id="C-1", element_type="constraint", detector="import",
                      verdict="violation", reason=None),
    ),
    findings=(
        Finding("forbidden-edge", "C-1", "observed gamma -[package_dep]-> alpha"),
        Finding("missing-required-edge", "I-05", "no mcp edge", suppressed_by="EX-01"),
    ),
    exceptions=(
        ExceptionStatus(id="EX-01", target="I-05", expires="2999-01-01", status="active"),
    ),
)

ARGS = {"manifest_path": "gamma/spec/intended-graph.yaml", "manifest_sha256": "ab" * 32,
        "snapshot_id": 1}


def test_payload_shape() -> None:
    p = report_payload(REPORT, **ARGS)
    assert p["schema"] == "conformance-report/v1"
    assert p["manifest"] == {"path": ARGS["manifest_path"], "sha256": ARGS["manifest_sha256"]}
    assert p["snapshot"] == {"id": 1}
    assert p["summary"]["verdicts"] == {"conformant": 1, "violation": 1, "unknown": 1}
    assert p["summary"]["findings"]["forbidden-edge"] == 1
    assert p["summary"]["findings"]["undeclared-edge"] == 0
    assert set(p["summary"]["findings"]) == {
        "missing-required-edge", "forbidden-edge", "undeclared-edge",
        "orphan-component", "expired-waiver", "manual-obligation",
    }


def test_json_is_byte_stable() -> None:
    a = render_json(REPORT, **ARGS)
    b = render_json(REPORT, **ARGS)
    assert a == b
    assert a.endswith("\n")
    parsed = json.loads(a)
    assert parsed == json.loads(json.dumps(parsed, sort_keys=True))
    assert a == json.dumps(parsed, indent=2, sort_keys=True) + "\n"


def test_text_lists_every_element_and_finding() -> None:
    text = render_text(REPORT, **ARGS)
    for needle in ("fixture-feed", "I-01", "I-05", "C-1", "conformant", "violation",
                   "forbidden-edge", "EX-01", "waived", "suppressed"):
        assert needle in text, f"missing {needle!r} in:\n{text}"
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run pytest tests/unit/test_conformance_report.py -v`
Expected: FAIL — no module `prograph.conformance.report`

- [ ] **Step 3: Implement**

`prograph/conformance/report.py`:

```python
"""Render a ConformanceReport as byte-stable JSON or human-readable text (spec D7)."""

from __future__ import annotations

import json

from prograph.conformance.engine import FINDING_CLASSES, ConformanceReport


def report_payload(
    report: ConformanceReport,
    *,
    manifest_path: str,
    manifest_sha256: str,
    snapshot_id: int,
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
        "manifest": {"path": manifest_path, "sha256": manifest_sha256},
        "snapshot": {"id": snapshot_id},
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
    *,
    manifest_path: str,
    manifest_sha256: str,
    snapshot_id: int,
) -> str:
    payload = report_payload(
        report,
        manifest_path=manifest_path,
        manifest_sha256=manifest_sha256,
        snapshot_id=snapshot_id,
    )
    return json.dumps(payload, indent=2, sort_keys=True) + "\n"


def render_text(
    report: ConformanceReport,
    *,
    manifest_path: str,
    manifest_sha256: str,
    snapshot_id: int,
) -> str:
    lines: list[str] = [
        f"# Conformance: {report.system}",
        f"manifest: {manifest_path} (sha256 {manifest_sha256[:12]}…)",
        f"snapshot: {snapshot_id}",
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
```

- [ ] **Step 4: Run tests, format, typecheck, commit**

```sh
uv run pytest tests/unit/test_conformance_report.py -v
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/conformance/report.py tests/unit/test_conformance_report.py
git commit -m "feat(conformance): byte-stable JSON + text report rendering"
```

---

### Task 8: CLI command + integration fixture + golden report

**Files:**
- Modify: `prograph/cli.py`
- Create: `tests/fixtures/monorepo_conformance/` (all files below)
- Create: `tests/integration/test_cli_conformance.py`
- Create: `tests/fixtures/monorepo_conformance/golden/conformance.json` (generated in Step 5)

**Interfaces:**
- Consumes: `load_manifest`, `ManifestError`, `load_observed`, `evaluate`, `exit_code`,
  `render_json`, `render_text`, `report_payload`, `read_intended_path`.
- Produces: `prograph conformance` per spec D7.

- [ ] **Step 1: Create the fixture monorepo**

```
tests/fixtures/monorepo_conformance/
├── alpha/
│   ├── pyproject.toml
│   └── alpha/__init__.py          (empty file)
├── beta/
│   ├── pyproject.toml
│   ├── beta/__init__.py           (empty file)
│   ├── data/feed.txt              (content: "feed\n")
│   └── schemas/feed-v1.json
├── gamma/
│   ├── pyproject.toml
│   ├── gamma/__init__.py          (empty file)
│   ├── schemas/feed-v1.json       (byte-identical copy of beta's)
│   └── spec/intended-graph.yaml
└── green-manifest.yaml
```

`alpha/pyproject.toml`:

```toml
[project]
name = "alpha"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = ["beta"]
```

`beta/pyproject.toml`:

```toml
[project]
name = "beta"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = []
```

`gamma/pyproject.toml`:

```toml
[project]
name = "gamma"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = ["alpha"]

[tool.prograph]
reads = ["beta/data/feed.txt"]
intended = "spec/intended-graph.yaml"
```

`beta/schemas/feed-v1.json` and `gamma/schemas/feed-v1.json` (identical — same `$id` links
both projects to one contract node, mirroring `tests/fixtures/monorepo_mcp`):

```json
{
  "$id": "feed-v1",
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "ts": {"type": "string"}
  }
}
```

`gamma/spec/intended-graph.yaml` — exercises **every finding class and unknown reason**:

```yaml
schema: intended-graph/v1
system: fixture-feed
components:
  - id: alpha.api
    project: alpha
    kind: service
    owner: architects
    responsibility: "serves the feed API"
  - id: alpha.worker
    project: alpha
    kind: module
    owner: architects
    responsibility: "background worker (makes alpha ambiguous for constraints)"
  - id: beta.lib
    project: beta
    kind: module
    owner: architects
    responsibility: "shared library"
  - id: beta.contract
    project: beta
    kind: contract
    owner: architects
    scope: schemas
    responsibility: "feed schema v1"
  - id: beta.ghost
    project: beta
    kind: module
    owner: architects
    scope: no/such/dir
    responsibility: "scope that matches nothing indexed"
  - id: gamma.reader
    project: gamma
    kind: cli
    owner: architects
    responsibility: "reads beta's feed file"
  - id: delta.ghost
    project: delta
    kind: service
    owner: architects
    responsibility: "project outside the workspace"
interfaces:
  - id: I-01
    producer: beta.lib
    consumer: alpha.api
    protocol: "python package"
    detector: import
  - id: I-02
    producer: "file:beta/data/feed.txt"
    consumer: gamma.reader
    protocol: "text file"
    detector: declared
  - id: I-03
    producer: beta.contract
    consumer: gamma.reader
    protocol: "json-schema feed-v1"
    detector: contract
  - id: I-04
    producer: alpha.api
    consumer: alpha.worker
    protocol: "in-process"
    detector: import
  - id: I-05
    producer: gamma.reader
    consumer: alpha.api
    protocol: "mcp"
    detector: mcp
  - id: I-06
    producer: beta.lib
    consumer: delta.ghost
    protocol: "python package"
    detector: import
  - id: I-07
    producer: beta.ghost
    consumer: gamma.reader
    protocol: "python package"
    detector: import
constraints:
  - id: ARCH-C1
    rule: "forbidden: gamma -> beta"
    detector: import
  - id: ARCH-C2
    rule: "forbidden: gamma -> alpha"
    detector: import
  - id: ARCH-C3
    rule: "forbidden: alpha.worker -> beta"
    detector: import
  - id: ARCH-C4
    rule: "forbidden: мутировать наблюдаемые данные — только чтение"
    detector: manual-evidence
resources: []
exceptions:
  - id: EX-01
    target: I-05
    reason: "mcp wiring lands next milestone"
    owner: architects
    expires: 2999-01-01
  - id: EX-02
    target: ARCH-C2
    reason: "grandfathered dep, to be removed"
    owner: architects
    expires: 2020-01-01
```

Expected outcome (assert in Step 4's tests):

| Element | Verdict | Reason / finding |
|---|---|---|
| I-01 | conformant | alpha→beta package_dep observed |
| I-02 | conformant | gamma declared read `beta/data/feed.txt` |
| I-03 | conformant | both projects link contract node `feed-v1` |
| I-04 | unknown / unsupported-resolution | same-project pair |
| I-05 | unknown, waived by EX-01 | missing-required-edge suppressed |
| I-06 | unknown / outside-workspace | orphan-component finding |
| I-07 | unknown / orphan-component | orphan-component finding |
| ARCH-C1 | conformant | no gamma→beta dep |
| ARCH-C2 | violation | forbidden-edge (gamma→alpha); EX-02 expired ⇒ expired-waiver |
| ARCH-C3 | unknown / unsupported-resolution | alpha hosts 2 components |
| ARCH-C4 | unknown / manual-evidence | manual-obligation |

Plus one `undeclared-edge` (gamma→alpha package_dep is in no interface). Default exit: **1**.

`green-manifest.yaml` (exit-0 path; placed at the fixture root, discovery ignores it):

```yaml
schema: intended-graph/v1
system: fixture-feed-green
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
    responsibility: "shared library"
interfaces:
  - id: I-01
    producer: beta.lib
    consumer: alpha.api
    protocol: "python package"
    detector: import
```

(gamma is unmodelled here, so its edges cannot fire `undeclared-edge`; the run is clean.)

- [ ] **Step 2: Write the failing integration tests**

`tests/integration/test_cli_conformance.py`:

```python
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
        ["conformance", "--monorepo", str(indexed), "--project", "gamma",
         "--format", "json", *extra],
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
        "expired-waiver", "forbidden-edge", "manual-obligation",
        "missing-required-edge", "orphan-component", "orphan-component",
        "undeclared-edge",
    ]
    i05 = next(f for f in payload["findings"] if f["element"] == "I-05")
    assert i05["suppressed_by"] == "EX-01"


def test_all_six_finding_classes_reachable(indexed: Path) -> None:
    _, payload = _json_run(indexed)
    nonzero = {k for k, v in payload["summary"]["findings"].items() if v > 0}
    assert nonzero == {
        "missing-required-edge", "forbidden-edge", "undeclared-edge",
        "orphan-component", "expired-waiver", "manual-obligation",
    }


def test_json_matches_golden(indexed: Path) -> None:
    res = runner.invoke(
        app,
        ["conformance", "--monorepo", str(indexed), "--project", "gamma",
         "--format", "json"],
    )
    if os.environ.get("PROGRAPH_UPDATE_GOLDEN") == "1":
        GOLDEN.parent.mkdir(parents=True, exist_ok=True)
        GOLDEN.write_text(res.stdout, encoding="utf-8")
    assert res.stdout == GOLDEN.read_text(encoding="utf-8")


def test_green_manifest_exits_0(indexed: Path) -> None:
    res = runner.invoke(
        app,
        ["conformance", "--monorepo", str(indexed),
         "--manifest", str(indexed / "green-manifest.yaml")],
    )
    assert res.exit_code == 0, res.stdout


def test_fail_on_escalates(indexed: Path) -> None:
    res = runner.invoke(
        app,
        ["conformance", "--monorepo", str(indexed),
         "--manifest", str(indexed / "green-manifest.yaml"),
         "--fail-on-verdict", "unknown"],
    )
    assert res.exit_code == 0  # green manifest has no unknowns

    code, _ = _json_run(indexed, "--fail-on", "undeclared-edge")
    assert code == 1


def test_unknown_fail_on_class_is_tool_error(indexed: Path) -> None:
    code, _ = (
        runner.invoke(
            app,
            ["conformance", "--monorepo", str(indexed), "--project", "gamma",
             "--fail-on", "nonsense-class"],
        ).exit_code,
        None,
    )
    assert code == 2


def test_unreadable_manifest_is_exit_2(indexed: Path, tmp_path: Path) -> None:
    bad = tmp_path / "bad.yaml"
    bad.write_text("schema: intended-graph/v9\nsystem: x\ncomponents: []\n", encoding="utf-8")
    res = runner.invoke(
        app, ["conformance", "--monorepo", str(indexed), "--manifest", str(bad)]
    )
    assert res.exit_code == 2


def test_no_snapshot_is_exit_2(tmp_path: Path) -> None:
    res = runner.invoke(
        app,
        ["conformance", "--monorepo", str(tmp_path),
         "--manifest", str(FIXTURE / "green-manifest.yaml")],
    )
    assert res.exit_code == 2


def test_manifest_and_project_are_mutually_exclusive(indexed: Path) -> None:
    res = runner.invoke(
        app,
        ["conformance", "--monorepo", str(indexed), "--project", "gamma",
         "--manifest", str(indexed / "green-manifest.yaml")],
    )
    assert res.exit_code == 2
    res = runner.invoke(app, ["conformance", "--monorepo", str(indexed)])
    assert res.exit_code == 2


def test_text_format_default(indexed: Path) -> None:
    res = runner.invoke(
        app, ["conformance", "--monorepo", str(indexed), "--project", "gamma"]
    )
    assert res.exit_code == 1
    assert "fixture-feed" in res.stdout and "ARCH-C2" in res.stdout
```

- [ ] **Step 3: Run to verify failure**

Run: `uv run pytest tests/integration/test_cli_conformance.py -v`
Expected: FAIL — `conformance` is not a CLI command yet.

- [ ] **Step 4: Implement the CLI command** (append to `prograph/cli.py`, matching the house
style of `drift`; module already imports `sys`, `json as _json`, `PrographPaths`,
`_resolve_monorepo`, `err_console`, `console`)

```python
@app.command()
def conformance(
    monorepo: Path = typer.Option(  # noqa: B008
        None,
        "--monorepo",
        "-m",
        help="Monorepo root (default: current working directory).",
        exists=False,
        file_okay=False,
        dir_okay=True,
    ),
    manifest: Path = typer.Option(  # noqa: B008
        None,
        "--manifest",
        help="Path to an intended-graph/v1 YAML manifest.",
    ),
    project: str = typer.Option(
        None,
        "--project",
        help="Tracked project whose manifest to check ([tool.prograph] intended "
        "or spec/intended-graph.yaml).",
    ),
    format_: str = typer.Option("text", "--format", help="Output format: text | json."),
    fail_on: str = typer.Option(
        None,
        "--fail-on",
        help="Comma-separated finding classes escalated to exit 1 "
        "(exact spec D5 identifiers).",
    ),
    fail_on_verdict: str = typer.Option(
        None,
        "--fail-on-verdict",
        help="Comma-separated verdicts escalated to exit 1 (e.g. unknown).",
    ),
) -> None:
    """Check an intended-graph manifest against the latest snapshot (exit 0/1/2)."""
    import hashlib

    from prograph.config import read_intended_path
    from prograph.conformance.engine import (
        FINDING_CLASSES,
        VERDICT_CONFORMANT,
        VERDICT_UNKNOWN,
        VERDICT_VIOLATION,
        evaluate,
        exit_code,
        load_observed,
    )
    from prograph.conformance.manifest import ManifestError, load_manifest
    from prograph.conformance.report import render_json, render_text

    def tool_error(message: str) -> None:
        err_console.print(f"[red]error:[/red] {message}")
        raise typer.Exit(code=2)

    if format_ not in ("text", "json"):
        tool_error(f"unknown --format {format_!r} (expected text or json)")
    if (manifest is None) == (project is None):
        tool_error("exactly one of --manifest or --project is required")

    fail_on_set = frozenset(s.strip() for s in fail_on.split(",")) if fail_on else frozenset()
    unknown_classes = fail_on_set - set(FINDING_CLASSES)
    if unknown_classes:
        tool_error(f"unknown --fail-on classes: {sorted(unknown_classes)}")
    verdict_set = (
        frozenset(s.strip() for s in fail_on_verdict.split(","))
        if fail_on_verdict
        else frozenset()
    )
    unknown_verdicts = verdict_set - {VERDICT_CONFORMANT, VERDICT_VIOLATION, VERDICT_UNKNOWN}
    if unknown_verdicts:
        tool_error(f"unknown --fail-on-verdict values: {sorted(unknown_verdicts)}")

    root = _resolve_monorepo(monorepo)
    paths = PrographPaths(monorepo_root=root)
    if not paths.db_path.exists():
        tool_error(f"no snapshot at {paths.db_path} — run `prograph index` first")
    db = str(paths.db_path)

    if project is not None:
        pid = _core.project_by_name(db, project)
        desc = _core.describe_project(db, pid) if pid is not None else None
        if desc is None:
            tool_error(f"project {project!r} is not in the latest snapshot")
            return  # unreachable; narrows Optional for the type checker
        project_root = root / desc.root_path.removeprefix("./")
        intended = read_intended_path(project_root / "pyproject.toml")
        manifest_path = project_root / (intended or "spec/intended-graph.yaml")
    else:
        manifest_path = manifest

    try:
        loaded = load_manifest(manifest_path)
    except ManifestError as exc:
        tool_error(str(exc))
        return  # unreachable

    observed = load_observed(db)
    if observed is None:
        tool_error(f"no snapshot data in {paths.db_path}")
        return  # unreachable

    import datetime as _dt

    report = evaluate(loaded, observed, _dt.date.today())

    raw_snap = _core.latest_snapshot_info(db)
    snapshot_id = raw_snap.id if raw_snap is not None else 0
    sha256 = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
    try:
        display_path = str(manifest_path.resolve().relative_to(root.resolve()))
    except ValueError:
        display_path = str(manifest_path)

    render = render_json if format_ == "json" else render_text
    sys.stdout.write(
        render(
            report,
            manifest_path=display_path,
            manifest_sha256=sha256,
            snapshot_id=snapshot_id,
        )
    )
    raise typer.Exit(code=exit_code(report, fail_on_set, verdict_set))
```

(`SnapshotInfo.id` verified against `prograph/_core.pyi:112`.)

- [ ] **Step 5: Generate the golden file, run everything**

```sh
PROGRAPH_UPDATE_GOLDEN=1 uv run pytest \
  tests/integration/test_cli_conformance.py::test_json_matches_golden -v
git diff --stat  # review the new golden file content by eye before committing
uv run pytest tests/integration/test_cli_conformance.py tests/unit -v
```

Expected: all PASS; golden JSON exists and matches spec D7's byte-stable shape.

- [ ] **Step 6: Format, lint, typecheck, commit**

```sh
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add prograph/cli.py tests/integration/test_cli_conformance.py tests/fixtures/monorepo_conformance
git commit -m "feat(conformance): prograph conformance CLI with golden e2e fixture"
```

---

### Task 9: WS-005 real-manifest acceptance fixture (vendored pinned copy)

The first real manifest already exists — `steward@727a28d`
`workstreams/WS-005-gate-verdicts/spec/intended-graph.yaml` (6 components, 4 interfaces,
4 constraints, spanning steward + dispatcher). Per repo-boundaries we vendor a pinned copy;
it proves the loader accepts the real thing (not just our fixtures) and locks the schema
against accidental narrowing.

**External prerequisite note:** the *cross-repo* golden/integration slice — running
`prograph conformance --project steward` against the live umbrella workspace index — depends
on workspace state (steward + dispatcher indexed, allowlist current) and is an operational
acceptance step after merge, NOT a pytest in this repo. Only the loader-level acceptance is
in-tree.

**Files:**
- Create: `tests/fixtures/ws005_manifest/intended-graph.yaml` (byte-exact copy)
- Create: `tests/fixtures/ws005_manifest/PIN` (provenance record)
- Test: `tests/unit/test_conformance_manifest.py` (append)

- [ ] **Step 1: Vendor the pinned copy**

```sh
mkdir -p tests/fixtures/ws005_manifest
git -C ../steward show 727a28d:workstreams/WS-005-gate-verdicts/spec/intended-graph.yaml \
  > tests/fixtures/ws005_manifest/intended-graph.yaml
printf 'source: steward@727a28d workstreams/WS-005-gate-verdicts/spec/intended-graph.yaml\nvendored: 2026-08-03\npurpose: loader acceptance fixture for the first real intended-graph/v1 manifest\n' \
  > tests/fixtures/ws005_manifest/PIN
```

- [ ] **Step 2: Write the failing test** (append to `tests/unit/test_conformance_manifest.py`)

```python
WS005 = Path(__file__).resolve().parent.parent / "fixtures" / "ws005_manifest"


def test_ws005_real_manifest_loads() -> None:
    """The first real manifest (steward@727a28d) must pass the strict loader as-is."""
    m = load_manifest(WS005 / "intended-graph.yaml")
    assert m.system == "ws005-governance-panel"
    assert len(m.components) == 6
    assert [i.id for i in m.interfaces] == ["I-01", "I-02", "I-03", "I-04"]
    assert [c.id for c in m.constraints] == ["ARCH-C1", "ARCH-C2", "ARCH-C3", "ARCH-C4"]
    assert m.exceptions == []
    # Mechanical rules parse; manual-evidence prose is not parsed.
    parsed = parse_rule(m.constraints[0].rule)
    assert parsed == ForbiddenRule(src="dispatcher", dst="steward")
    arch_c4 = parse_rule(m.constraints[3].rule)
    assert arch_c4.dst == "file:.steward/gate_verdicts.jsonl"
```

- [ ] **Step 3: Run, fix any schema mismatch surfaced (that is the point of this task),
commit**

Run: `uv run pytest tests/unit/test_conformance_manifest.py::test_ws005_real_manifest_loads -v`
Expected: PASS. If it fails, the schema models are narrower than the approved real manifest —
fix the models (never the vendored copy) and note the correction in the commit message.

```sh
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
git add tests/fixtures/ws005_manifest tests/unit/test_conformance_manifest.py
git commit -m "test(conformance): vendor WS-005 manifest (steward@727a28d) as acceptance fixture"
```

---

### Task 10: Docs + TODO closure + PR

**Files:**
- Modify: `CLAUDE.md` (commands block, architecture section, plans list)
- Modify: `TODO.md` (mark the intended-graph item done)

- [ ] **Step 1: Update `CLAUDE.md`**

1. Commands block — add after the `serve` line:

```sh
uv run prograph conformance [--monorepo PATH] (--manifest PATH | --project NAME) \
    [--format text|json] [--fail-on <classes>] [--fail-on-verdict <verdicts>]  # exit 0/1/2
```

2. "Design and plans" — add under Plans:
   `- Conformance v1 plan: docs/superpowers/plans/2026-08-03-prograph-conformance-v1.md`
3. Architecture section — extend the `prograph` (Python package) list with:
   `- conformance/ — intended-graph/v1 loader (manifest.py), verdict engine (engine.py),
   byte-stable report (report.py); manifest is read at check time, never stored (spec D8)`
   and add `conformance` to the `cli.py` command list.
4. "Current deferrals" — add: module-level constraint attribution + `--since` comparisons +
   layering sugar (intended-graph v1.1); contract-sharing `undeclared-edge` detection.

- [ ] **Step 2: Update `TODO.md`**

Flip the intended-graph item to `[x]` and record what shipped, keeping the existing item's
inline tags:

```markdown
- [x] **Intended graph v1 + `prograph conformance`** — shipped: strict
  `intended-graph/v1` loader, three-valued verdict engine (honest
  `unsupported-resolution` per D2), finding taxonomy v1, CLI with byte-stable
  JSON and 0/1/2 exit codes; WS-005 manifest (steward@727a28d) vendored as
  acceptance fixture. Spec:
  `docs/superpowers/specs/2026-08-03-prograph-intended-graph-design.md`; plan:
  `docs/superpowers/plans/2026-08-03-prograph-conformance-v1.md`. Follow-ups
  live in the spec's v1.1 list (module-level resolution, `--since`, layering
  sugar). Consumer: steward `GC-ARCH-CONFORMANCE` (@trigger there is
  "prograph conformance реализован" — теперь выполнен). @owner:andrei
  @id:intended-graph-v1
```

- [ ] **Step 3: Full local gate (this IS CI), then commit and open the PR**

```sh
uv run ruff format . && uv run ruff check .
uv run pyrefly check 'prograph/**/*.py' 'tests/unit/**/*.py' 'tests/integration/**/*.py'
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings  # untouched, cheap
uv run pytest -v
git add CLAUDE.md TODO.md
git commit -m "docs: conformance v1 shipped — CLAUDE.md + TODO closure"
git push -u origin feat/conformance-v1
gh pr create --title "feat: intended graph v1 + prograph conformance (spec 2026-08-03)" \
  --body "Implements docs/superpowers/specs/2026-08-03-prograph-intended-graph-design.md ..."
```

Then: action the GitHub Copilot review (fix valid points, answer invalid ones with
reasoning); do NOT merge — the owner merges. After merge: notify the steward side that its
`@trigger:"prograph conformance реализован"` has fired (inbox issue per ADR-ECO-006 if a
plan change is requested there).

---

## Self-Review (performed while writing)

- **Spec coverage:** D1 (Task 2 + Task 8 path resolution, default `spec/intended-graph.yaml`),
  D2 (Task 4: project granularity, scope in declared matching, same-project ⇒
  `unsupported-resolution`), D3 (DETECTOR_TO_KIND, Task 3), D4 (closed verdict set, reasons,
  absence ⇒ unknown + finding — Tasks 4–6), D5 (all six classes — Tasks 4–6, fixture reaches
  each one, Task 8 asserts it), D6 (rule grammar — Tasks 1, 5), D7 (CLI surface, split
  namespaces, exit codes, byte-stable JSON, full-report no-truncation — Tasks 7–8), D8 (no DB
  writes anywhere; manifest parsed per invocation), Schema v1 (Task 1 strict models; Task 9
  proves against the real manifest). Rollout step 3 = this plan; step 2 already merged in
  steward (`727a28d`); step 4 is steward-side after this ships.
- **Placeholder scan:** none; every step carries executable content.
- **Type consistency:** `Finding.finding_class` (JSON key `"class"`), `ElementResult.
  element_type` (JSON key `"type"`) — renamed only at the payload boundary in `report.py`;
  `evaluate(manifest, observed, today)` signature consistent across Tasks 4–6 and CLI;
  `SnapshotInfo.id` verified against the stub.
