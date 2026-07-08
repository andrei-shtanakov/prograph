/* eslint-env browser */

const KIND_COLORS = {
    package_dep: '#888',
    mcp_call: '#0fa3b1',
    contract_link: '#d8862c',
};

const KIND_LINESTYLES = {
    package_dep: 'solid',
    mcp_call: 'solid',
    contract_link: 'dashed',
};

const PROJECT_KIND_COLORS = {
    python: '#3776ab',
    rust: '#dea584',
    js: '#f0db4f',
    docs: '#888',
    mixed: '#9b59b6',
};

function statusOverride(status) {
    if (status === 'added') return '#28a745';
    if (status === 'removed') return '#dc3545';
    return null;
}

function statusLineStyle(status, kind) {
    if (status === 'removed') return 'dotted';
    return KIND_LINESTYLES[kind] || 'solid';
}

function attachCoseBilkent() {
    // cose-bilkent registers itself as a global on script load. Just ensure it's wired.
    if (window.cytoscape && window.cytoscapeCoseBilkent) {
        window.cytoscape.use(window.cytoscapeCoseBilkent);
    }
}

export function buildCytoscape(container) {
    attachCoseBilkent();
    // eslint-disable-next-line no-undef
    const cy = cytoscape({
        container,
        elements: [],
        style: [
            {
                selector: 'node[node_kind = "project"]',
                style: {
                    'shape': 'round-rectangle',
                    'label': 'data(label)',
                    'background-color': (n) => PROJECT_KIND_COLORS[n.data('kind')] || '#888',
                    'color': '#fff',
                    'text-valign': 'center',
                    'text-halign': 'center',
                    'width': 'label',
                    'height': 40,
                    'padding': '12px',
                    'font-size': 12,
                    'font-weight': 600,
                    'border-width': 1,
                    'border-color': '#333',
                },
            },
            {
                selector: 'node[node_kind = "contract"]',
                style: {
                    'shape': 'diamond',
                    'label': 'data(label)',
                    'background-color': '#fff',
                    'color': '#333',
                    'text-valign': 'bottom',
                    'text-halign': 'center',
                    'text-margin-y': 8,
                    'width': 40,
                    'height': 40,
                    'font-size': 11,
                    'font-style': 'italic',
                    'border-width': 2,
                    'border-color': '#d8862c',
                },
            },
            {
                selector: 'edge',
                style: {
                    'curve-style': 'bezier',
                    'target-arrow-shape': 'triangle',
                    'line-color': (e) =>
                        statusOverride(e.data('status'))
                        || KIND_COLORS[e.data('kind')]
                        || '#888',
                    'target-arrow-color': (e) =>
                        statusOverride(e.data('status'))
                        || KIND_COLORS[e.data('kind')]
                        || '#888',
                    'line-style': (e) =>
                        statusLineStyle(e.data('status'), e.data('kind')),
                    'width': (e) =>
                        e.data('status') === 'added' || e.data('status') === 'removed'
                            ? 3
                            : 2,
                    'arrow-scale': 1.2,
                    'opacity': (e) => (e.data('status') === 'removed' ? 0.6 : 1),
                },
            },
            {
                selector: ':selected',
                style: {
                    'border-color': '#222',
                    'border-width': 3,
                    'line-color': '#222',
                    'target-arrow-color': '#222',
                },
            },
        ],
        layout: { name: 'cose-bilkent', animate: false, nodeRepulsion: 4500 },
        wheelSensitivity: 0.2,
    });

    cy.on('tap', 'node', (evt) => {
        const node = evt.target;
        const detail = {
            type: 'node',
            id: node.id(),
            node_kind: node.data('node_kind'),
            name: node.data('name'),
        };
        window.dispatchEvent(new CustomEvent('prograph:select', { detail }));
    });

    cy.on('tap', 'edge', (evt) => {
        const edge = evt.target;
        const detail = {
            type: 'edge',
            edge_id: edge.data('edge_id'),
            kind: edge.data('kind'),
        };
        window.dispatchEvent(new CustomEvent('prograph:select', { detail }));
    });

    cy.on('tap', (evt) => {
        if (evt.target === cy) {
            window.dispatchEvent(new CustomEvent('prograph:deselect'));
        }
    });

    return cy;
}

export function renderGraph(cy, data) {
    cy.elements().remove();
    cy.add(
        data.nodes.map((n) => ({
            data: {
                id: n.id,
                label: n.label,
                name: n.name,
                kind: n.kind,
                node_kind: n.node_kind,
            },
        }))
    );
    cy.add(
        data.edges.map((e) => ({
            data: {
                id: e.id,
                source: e.source,
                target: e.target,
                kind: e.kind,
                edge_id: e.edge_id,
                status: e.status || 'unchanged',
            },
        }))
    );
    cy.layout({ name: 'cose-bilkent', animate: false, nodeRepulsion: 4500 }).run();
}
