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
