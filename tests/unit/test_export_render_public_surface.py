"""M9: render_project's Public surface + Modules sections."""

from prograph.export.render import render_project
from prograph.models import (
    InternalImportRow,
    ModuleRow,
    ProjectDescription,
    PublicSymbolRow,
)


def _make_desc(**overrides) -> ProjectDescription:
    defaults = {
        "project_id": 1,
        "name": "x",
        "slug": "x",
        "kind": "python",
        "root_path": "./x",
        "attrs": {},
        "snapshot_id": 1,
        "snapshot_ts": "2026-05-26T00:00:00Z",
        "mcp_decls": [],
        "contract_files": [],
        "outbound": [],
        "inbound": [],
        "recent_changes": [],
        "modules": [],
        "public_symbols": [],
        "internal_imports": [],
    }
    defaults.update(overrides)
    return ProjectDescription(**defaults)


def test_public_symbols_render_when_present():
    desc = _make_desc(
        public_symbols=[
            PublicSymbolRow(
                module_id=1, rel_path="api.py", name="MaestroAPI", kind="class", line=10
            ),
            PublicSymbolRow(
                module_id=1, rel_path="api.py", name="decide", kind="function", line=20
            ),
        ],
    )
    md = render_project(desc)
    assert "### Public symbols" in md
    assert "`MaestroAPI` (class) — `api.py:10`" in md
    assert "`decide` (function) — `api.py:20`" in md


def test_modules_section_renders_summary():
    desc = _make_desc(
        modules=[
            ModuleRow(id=1, rel_path="api.py", language="python"),
            ModuleRow(id=2, rel_path="helpers.py", language="python"),
        ],
        public_symbols=[
            PublicSymbolRow(module_id=1, rel_path="api.py", name="x", kind="function", line=1)
        ],
        internal_imports=[
            InternalImportRow(module_id=1, rel_path="api.py", target_path="x.util", line=3)
        ],
    )
    md = render_project(desc)
    assert "## Modules" in md
    assert "2 files, 1 public symbols, 1 internal imports" in md
    assert "- `api.py` (python)" in md
    assert "- `helpers.py` (python)" in md


def test_empty_sections_render_none():
    desc = _make_desc()
    md = render_project(desc)
    # Public symbols subsection exists even when empty.
    public_section = md.split("### Public symbols")[1].split("###")[0].split("##")[0]
    assert "_None._" in public_section
    # Modules top-level section exists too.
    modules_section = md.split("## Modules")[1].split("##")[0]
    assert "_None._" in modules_section


def test_render_is_deterministic_with_module_facts():
    desc = _make_desc(
        modules=[ModuleRow(id=1, rel_path="api.py", language="python")],
        public_symbols=[
            PublicSymbolRow(module_id=1, rel_path="api.py", name="x", kind="function", line=1)
        ],
    )
    assert render_project(desc) == render_project(desc)
