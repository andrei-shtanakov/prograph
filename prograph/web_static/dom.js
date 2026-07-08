/* eslint-env browser */

/**
 * Create a DOM element safely.
 *
 *   el('div', {class: 'card', dataset: {id: '7'}}, [
 *       el('strong', {}, ['Title']),
 *       ' — body text',
 *   ])
 *
 * All string children are inserted via createTextNode — they CANNOT inject HTML.
 * To insert another element, pass it as a child directly.
 *
 * NEVER pass user-controlled strings as the tag name or attribute name; only
 * attribute values are safe under this contract.
 */
export function el(tag, attrs, children) {
    const node = document.createElement(tag);
    if (attrs) {
        for (const key of Object.keys(attrs)) {
            const val = attrs[key];
            if (val === null || val === undefined) continue;
            if (key === 'class') {
                node.className = String(val);
            } else if (key === 'dataset') {
                for (const dk of Object.keys(val)) {
                    node.dataset[dk] = String(val[dk]);
                }
            } else if (key === 'onclick' && typeof val === 'function') {
                node.addEventListener('click', val);
            } else {
                node.setAttribute(key, String(val));
            }
        }
    }
    if (children) {
        for (const child of children) {
            if (child === null || child === undefined) continue;
            if (typeof child === 'string' || typeof child === 'number') {
                node.appendChild(document.createTextNode(String(child)));
            } else if (child instanceof Node) {
                node.appendChild(child);
            }
        }
    }
    return node;
}

/** Replace all children of `parent` with the given nodes. */
export function setChildren(parent, children) {
    parent.replaceChildren();
    for (const child of children) {
        if (child === null || child === undefined) continue;
        if (typeof child === 'string' || typeof child === 'number') {
            parent.appendChild(document.createTextNode(String(child)));
        } else if (child instanceof Node) {
            parent.appendChild(child);
        }
    }
}

/** Replace all children with a single message node. */
export function setMessage(parent, message, cls) {
    setChildren(parent, [el('p', cls ? {class: cls} : {}, [message])]);
}
