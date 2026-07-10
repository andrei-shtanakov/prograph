/* eslint-env browser */
import { buildCytoscape, renderGraph } from './graph.js';
import { el, setChildren, setMessage } from './dom.js';

const cy = buildCytoscape(document.getElementById('graph'));

async function fetchJson(url) {
    const r = await fetch(url);
    if (!r.ok) throw new Error(`${url} → ${r.status}`);
    return r.json();
}

async function loadGraph(since) {
    const url = since
        ? `/api/graph?since=${encodeURIComponent(since)}`
        : '/api/graph';
    const data = await fetchJson(url);
    renderGraph(cy, data);

    const info = document.getElementById('snapshot-info');
    const diffSuffix = since ? ` · diff since #${since}` : '';
    info.textContent = `snapshot #${data.snapshot_id} · ${data.n_projects} projects · ${data.n_contracts} contracts · ${data.n_edges} edges${diffSuffix}`;
}

async function populateSnapshotPicker() {
    try {
        const snapshots = await fetchJson('/api/snapshots?limit=50');
        const select = document.getElementById('diff-since');
        const offOption = select.firstElementChild;
        setChildren(select, [offOption]);
        if (!snapshots.length) return;
        const latestId = snapshots[0].id;
        for (const s of snapshots) {
            if (s.id === latestId) continue;
            const opt = el('option', { value: String(s.id) }, [`#${s.id} (${s.ts})`]);
            select.appendChild(opt);
        }
    } catch (e) {
        console.warn('snapshot picker init failed', e);
    }
}

async function init() {
    await loadGraph(null);
    refreshActivity();
    await populateSnapshotPicker();

    const picker = document.getElementById('diff-since');
    picker.addEventListener('change', () => {
        const v = picker.value;
        loadGraph(v || null).catch((e) => {
            console.error('diff load failed', e);
        });
    });
}

async function refreshActivity() {
    try {
        const events = await fetchJson('/api/changelog?limit=10');
        const list = document.getElementById('activity-list');
        setChildren(list, events.map(renderActivityRow));
    } catch (e) {
        console.warn('activity fetch failed', e);
    }
}

function renderActivityRow(ev) {
    return el('li', {}, [
        el('span', { class: 'ts' }, [ev.ts]),
        ' · ',
        `${ev.entity_kind} ${ev.entity_id}: `,
        el('strong', {}, [ev.change]),
    ]);
}

// ─── Side panel ───────────────────────────────────────────────────────────────

const sidepanel = document.getElementById('sidepanel');

window.addEventListener('prograph:select', async (evt) => {
    const detail = evt.detail;
    setMessage(sidepanel, 'Loading…');
    try {
        if (detail.type === 'node' && detail.node_kind === 'project') {
            const payload = await fetchJson(`/api/projects/by-name/${encodeURIComponent(detail.name)}`);
            setChildren(sidepanel, renderProject(payload));
        } else if (detail.type === 'node' && detail.node_kind === 'contract') {
            const slug = detail.id.replace(/^c:/, '');
            const payload = await fetchJson(`/api/contracts/by-slug/${encodeURIComponent(slug)}`);
            setChildren(sidepanel, renderContract(payload));
        } else if (detail.type === 'edge') {
            const payload = await fetchJson(`/api/edges/${detail.edge_id}`);
            setChildren(sidepanel, renderEdge(payload));
        }
    } catch (e) {
        setMessage(sidepanel, `Error loading: ${e.message}`);
    }
});

window.addEventListener('prograph:deselect', () => {
    setMessage(sidepanel, 'Click a node or edge to see details.', 'placeholder');
});

// Render functions return arrays of DOM nodes; setChildren swaps them into the side panel.

// True only for http(s) URLs — keeps javascript:/data: out of href.
function isHttpUrl(value) {
    return typeof value === 'string' && /^https?:\/\//.test(value);
}

// Short human name for a contract id: URLs collapse to their last path segment.
function contractShortName(id) {
    if (!isHttpUrl(id)) return id;
    const tail = id.replace(/\/+$/, '').split('/').pop();
    return tail || id;
}

// Render a name that may be a URL (contract declared_id): short bold text
// wrapped in a link to the full URL. Non-URLs render as plain <strong>.
function nameOrLink(value) {
    if (isHttpUrl(value)) {
        return el('a', { href: value, target: '_blank', rel: 'noopener noreferrer' }, [
            el('strong', {}, [contractShortName(value)]),
        ]);
    }
    return el('strong', {}, [value]);
}

function renderProject(p) {
    const nodes = [
        el('h2', {}, [p.name]),
        renderDl([
            ['kind', p.kind],
            ['root', el('code', {}, [p.root_path])],
            ['snapshot', `#${p.snapshot_id}`],
        ]),
    ];
    if (p.mcp_decls && p.mcp_decls.length) {
        nodes.push(el('h3', {}, ['MCP tools exposed']));
        nodes.push(el('ul', {}, p.mcp_decls.map((d) => (
            el('li', {}, [
                el('code', {}, [d.tool_name]),
                ' — ',
                el('code', {}, [`${d.rel_path}:${d.line}`]),
            ])
        ))));
    }
    if (p.outbound && p.outbound.length) {
        nodes.push(el('h3', {}, ['Outbound']));
        nodes.push(el('ul', {}, p.outbound.map((e) => (
            el('li', {}, [
                '→ ',
                nameOrLink(e.target_name),
                ' ',
                el('em', {}, [e.kind]),
            ])
        ))));
    }
    if (p.inbound && p.inbound.length) {
        nodes.push(el('h3', {}, ['Inbound']));
        nodes.push(el('ul', {}, p.inbound.map((e) => (
            el('li', {}, [
                '← ',
                nameOrLink(e.source_name),
                ' ',
                el('em', {}, [e.kind]),
            ])
        ))));
    }
    if (p.public_symbols && p.public_symbols.length) {
        nodes.push(el('h3', {}, ['Public symbols']));
        const items = p.public_symbols.slice(0, 50).map((s) => (
            el('li', {}, [
                el('code', {}, [s.name]),
                ` (${s.kind}) — `,
                el('code', {}, [`${s.rel_path}:${s.line}`]),
            ])
        ));
        nodes.push(el('ul', {}, items));
        if (p.public_symbols.length > 50) {
            nodes.push(el('p', {}, [
                el('em', {}, [`(${p.public_symbols.length - 50} more)`]),
            ]));
        }
    }
    if (p.modules && p.modules.length) {
        nodes.push(el('h3', {}, ['Modules']));
        const symCount = (p.public_symbols || []).length;
        const impCount = (p.internal_imports || []).length;
        const summary = `${p.modules.length} files, ${symCount} symbols, ${impCount} internal imports`;
        nodes.push(el('p', {}, [el('em', {}, [summary])]));
    }
    if (p.inbound_refs && p.inbound_refs.length) {
        nodes.push(el('h3', {}, ['Inbound references']));
        const items = p.inbound_refs.slice(0, 50).map((r) => {
            const sym = r.to_symbol_name || '(module)';
            const target = r.to_module_path ? `${r.to_module_path}::${sym}` : sym;
            return el('li', {}, [
                'from ',
                el('strong', {}, [r.from_project_name]),
                ' — ',
                el('code', {}, [`${r.from_module_rel_path}:${r.line}`]),
                ' → ',
                el('code', {}, [target]),
            ]);
        });
        nodes.push(el('ul', {}, items));
        if (p.inbound_refs.length > 50) {
            nodes.push(el('p', {}, [
                el('em', {}, [`(${p.inbound_refs.length - 50} more)`]),
            ]));
        }
    }
    if (p.outbound_refs && p.outbound_refs.length) {
        nodes.push(el('h3', {}, ['Outbound references']));
        const items = p.outbound_refs.slice(0, 50).map((r) => {
            const sym = r.to_symbol_name || '(module)';
            const target = r.to_module_path ? `${r.to_module_path}::${sym}` : sym;
            return el('li', {}, [
                'to ',
                el('strong', {}, [r.to_project_name]),
                ' — ',
                el('code', {}, [`${r.from_module_rel_path}:${r.line}`]),
                ' → ',
                el('code', {}, [target]),
            ]);
        });
        nodes.push(el('ul', {}, items));
    }
    if (p.drifts && p.drifts.length) {
        nodes.push(el('h3', {}, ['Drift findings']));
        const groups = { missing: [], extra: [], stale_todo: [] };
        p.drifts.forEach((d) => {
            if (groups[d.kind]) groups[d.kind].push(d);
        });
        const labels = {
            missing: 'Missing (declared but not implemented)',
            extra: 'Extra (implemented but not declared)',
            stale_todo: 'Stale TODOs',
        };
        ['missing', 'extra', 'stale_todo'].forEach((k) => {
            if (!groups[k].length) return;
            nodes.push(el('h4', {}, [labels[k]]));
            const items = groups[k].map((d) => {
                const conf = d.confidence === 'low' ? ' (low confidence)' : '';
                return el('li', {}, [
                    el('code', {}, [d.entity_name]),
                    ' (',
                    d.entity_kind,
                    ') — ',
                    el('code', {}, [`${d.source_path}:${d.source_line}`]),
                    conf,
                ]);
            });
            nodes.push(el('ul', {}, items));
        });
    }
    if (p.recent_changes && p.recent_changes.length) {
        nodes.push(el('h3', {}, ['Recent changes']));
        nodes.push(el('ul', {}, p.recent_changes.map((c) => (
            el('li', {}, [`snapshot #${c.snapshot_id}: ${c.change}`])
        ))));
    }
    return nodes;
}

function renderContract(c) {
    const displayId = c.declared_id || c.slug;
    const dl = [
        ['kind', c.kind],
        ['content hash', el('code', {}, [`${c.content_hash.slice(0, 16)}…`])],
    ];
    if (isHttpUrl(c.declared_id)) {
        dl.unshift(['declared id', el('a', {
            href: c.declared_id, target: '_blank', rel: 'noopener noreferrer',
        }, [c.declared_id])]);
    } else if (c.declared_id) {
        dl.unshift(['declared id', el('code', {}, [c.declared_id])]);
    }
    const nodes = [
        el('h2', {}, [`Contract: ${contractShortName(displayId)}`]),
        renderDl(dl),
        el('h3', {}, ['Owners']),
    ];
    nodes.push(el('ul', {}, (c.owners || []).map((o) => (
        el('li', {}, [
            el('strong', {}, [o.project_name]),
            ' — ',
            el('code', {}, [o.rel_path]),
        ])
    ))));
    return nodes;
}

function renderEdge(e) {
    const nodes = [
        el('h2', {}, [`Edge: ${e.kind}`]),
        renderDl([
            ['from', el('span', {}, [
                nameOrLink(e.from_name),
                ` (${e.from_kind})`,
            ])],
            ['to', el('span', {}, [
                nameOrLink(e.to_name),
                ` (${e.to_kind})`,
            ])],
            ['attrs', el('code', {}, [JSON.stringify(e.attrs)])],
        ]),
    ];
    if (e.evidence && e.evidence.length) {
        nodes.push(el('h3', {}, ['Evidence']));
        nodes.push(el('ul', {}, e.evidence.map((ev) => (
            el('li', {}, [el('code', {}, [`${ev.rel_path}:${ev.line}`])])
        ))));
    } else {
        nodes.push(el('p', {}, [el('em', {}, ['No evidence rows for this edge.'])]));
    }
    return nodes;
}

function renderDl(pairs) {
    const dl = el('dl', {}, []);
    for (const [label, value] of pairs) {
        dl.appendChild(el('dt', {}, [label]));
        if (typeof value === 'string') {
            dl.appendChild(el('dd', {}, [value]));
        } else {
            dl.appendChild(el('dd', {}, [value]));
        }
    }
    return dl;
}

// ─── Search ──────────────────────────────────────────────────────────────────

const searchBox = document.getElementById('search-box');
const searchResults = document.getElementById('search-results');
let searchDebounce;

searchBox.addEventListener('input', () => {
    clearTimeout(searchDebounce);
    const q = searchBox.value.trim();
    if (!q) {
        searchResults.classList.remove('visible');
        setChildren(searchResults, []);
        return;
    }
    searchDebounce = setTimeout(() => doSearch(q), 250);
});

async function doSearch(q) {
    try {
        const hits = await fetchJson(`/api/search?q=${encodeURIComponent(q)}&limit=10`);
        if (!hits.length) {
            setChildren(searchResults, [el('div', { class: 'hit' }, [el('em', {}, ['No matches'])])]);
        } else {
            const items = hits.map((h) => {
                const div = el('div', {
                    class: 'hit',
                    dataset: {
                        entityKind: h.entity_kind,
                        entityId: String(h.entity_id),
                        name: h.name,
                    },
                }, [
                    el('strong', {}, [h.name]),
                    ' ',
                    el('span', { class: 'hit-kind' }, [h.entity_kind]),
                ]);
                div.addEventListener('click', () => onSearchHitClick(div));
                return div;
            });
            setChildren(searchResults, items);
        }
        searchResults.classList.add('visible');
    } catch (e) {
        setChildren(searchResults, [el('div', { class: 'hit' }, [`Error: ${e.message}`])]);
        searchResults.classList.add('visible');
    }
}

function onSearchHitClick(div) {
    const kind = div.dataset.entityKind;
    const name = div.dataset.name;
    searchResults.classList.remove('visible');
    searchBox.value = '';
    const slug = slugify(name);
    const cyId = kind === 'project' ? `p:${slug}` : `c:${slug}`;
    const node = cy.getElementById(cyId);
    if (node.length) {
        cy.elements().unselect();
        node.select();
        cy.animate({ center: { eles: node }, zoom: 1.2 }, { duration: 400 });
        window.dispatchEvent(new CustomEvent('prograph:select', {
            detail: { type: 'node', node_kind: kind, name, id: node.id() },
        }));
    }
}

function slugify(s) {
    return String(s).split('').map((c) => /[A-Za-z0-9_-]/.test(c) ? c : '-').join('');
}

document.addEventListener('click', (e) => {
    if (!searchBox.contains(e.target) && !searchResults.contains(e.target)) {
        searchResults.classList.remove('visible');
    }
});

init().catch((e) => {
    console.error('init failed', e);
    setMessage(document.getElementById('graph'), `Failed to load graph: ${e.message}`);
});
