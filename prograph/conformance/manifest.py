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
