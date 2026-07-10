"""Sanity-check the bundled static assets without booting a browser."""

from pathlib import Path

STATIC_DIR = Path(__file__).parents[2] / "prograph" / "web_static"


def test_static_dir_exists():
    assert STATIC_DIR.is_dir(), f"missing static dir at {STATIC_DIR}"


def test_index_html_references_expected_ids():
    html = (STATIC_DIR / "index.html").read_text()
    for expected_id in (
        "graph",
        "sidepanel",
        "search-box",
        "search-results",
        "activity-list",
        "snapshot-info",
    ):
        assert f'id="{expected_id}"' in html, f"index.html missing id={expected_id}"


def test_index_html_loads_app_module():
    html = (STATIC_DIR / "index.html").read_text()
    assert "/static/app.js" in html
    assert "/static/styles.css" in html


def test_graph_js_exports_expected_functions():
    js = (STATIC_DIR / "graph.js").read_text()
    assert "export function buildCytoscape" in js
    assert "export function renderGraph" in js


def test_app_js_imports_graph_and_dom():
    js = (STATIC_DIR / "app.js").read_text()
    assert "from './graph.js'" in js
    assert "from './dom.js'" in js


def test_app_js_dispatches_select_event():
    js = (STATIC_DIR / "app.js").read_text()
    assert "addEventListener('prograph:select'" in js


def test_app_js_does_not_use_innerHTML():
    """XSS hardening: all DOM construction must go through dom.js's el() helper."""
    js = (STATIC_DIR / "app.js").read_text()
    assert ".innerHTML" not in js, (
        "app.js must not use innerHTML — use dom.js helpers instead. "
        "See M6 plan §architecture for the XSS-safe DOM construction contract."
    )


def test_graph_js_does_not_use_innerHTML():
    js = (STATIC_DIR / "graph.js").read_text()
    assert ".innerHTML" not in js


def test_dom_helper_exports_el():
    js = (STATIC_DIR / "dom.js").read_text()
    assert "export function el" in js
    assert "export function setChildren" in js


def test_index_html_has_diff_picker():
    html = (STATIC_DIR / "index.html").read_text()
    assert 'id="diff-picker"' in html
    assert 'id="diff-since"' in html


def test_app_js_handles_since_param():
    js = (STATIC_DIR / "app.js").read_text()
    assert "/api/graph?since=" in js
    assert "populateSnapshotPicker" in js


def test_graph_js_tags_edge_status():
    js = (STATIC_DIR / "graph.js").read_text()
    assert "statusOverride" in js
    assert "statusLineStyle" in js
    assert "status: e.status" in js


def test_app_js_renders_public_symbols():
    js = (STATIC_DIR / "app.js").read_text()
    assert "Public symbols" in js
    assert "public_symbols" in js
    # Module summary text uses the structural count fields.
    assert "files," in js


def test_app_js_renders_inbound_outbound_refs():
    js = (STATIC_DIR / "app.js").read_text()
    assert "Inbound references" in js
    assert "Outbound references" in js
    assert "inbound_refs" in js
    assert "outbound_refs" in js


def test_app_js_renders_drift_findings():
    js = (STATIC_DIR / "app.js").read_text()
    assert "Drift findings" in js
    assert "missing" in js and "extra" in js and "stale_todo" in js


def test_contract_label_shortens_urls() -> None:
    """Canvas labels: URL declared_ids collapse to their last path segment."""
    from prograph.web_app import _contract_label

    assert (
        _contract_label(
            "https://github.com/andrei-shtanakov/maestro/benchmark-contract/"
            "report_benchmark-v1.schema.json",
            "slug",
        )
        == "report_benchmark-v1.schema.json"
    )
    assert _contract_label("https://all_ai_orchestrators/observability-contract/v1", "s") == "v1"
    assert _contract_label("contract-real", "s") == "contract-real"
    assert _contract_label(None, "fallback-slug") == "fallback-slug"
    # Trailing slashes are trimmed before taking the last segment.
    assert _contract_label("https://x.test/", "s") == "x.test"


def test_app_js_linkifies_only_http_urls() -> None:
    """The panel link helper must guard href against non-http(s) schemes."""
    app_js = (STATIC_DIR / "app.js").read_text(encoding="utf-8")
    assert "function isHttpUrl" in app_js
    assert "/^https?:\\/\\//" in app_js
    assert "noopener noreferrer" in app_js
