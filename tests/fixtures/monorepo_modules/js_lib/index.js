import { normalize } from './helpers.js';
import { Store } from './storage.js';

export class PublicAPI {
    constructor() {
        this.store = new Store();
    }
    query(q) {
        return normalize(q);
    }
}

export function publicFn(x) {
    return x * 2;
}

export const PUBLIC_CONST = 'v1';
