import assert from 'node:assert/strict';
import test from 'node:test';

import { STATUS_PILL_ID, ensureStatusPill, isStatusPillPresent } from '../src/scripts/tauri/status-pill.js';
import { createSaveStatusPill } from '../src/scripts/tauri/save-progress-indicator.js';

function fakeDocument() {
    /** @type {Map<string, any>} */
    const nodes = new Map();
    globalThis.HTMLDivElement = class {
        constructor() { this.id = ''; this.textContent = ''; this.style = {}; }
        remove() { nodes.delete(this.id); }
    };
    return {
        body: { appendChild(node) { nodes.set(node.id, node); } },
        createElement: () => new /** @type {any} */ (globalThis.HTMLDivElement)(),
        getElementById: id => nodes.get(id) ?? null,
    };
}

test('the pill is re-created once startup has removed it', () => {
    globalThis.document = /** @type {any} */ (fakeDocument());

    assert.equal(isStatusPillPresent(), false, 'nothing owns the line yet');

    const pill = ensureStatusPill();
    assert.equal(pill.id, STATUS_PILL_ID);
    assert.equal(isStatusPillPresent(), true);

    // Reuse the live element instead of stacking a second one.
    assert.equal(ensureStatusPill(), pill);

    pill.remove();
    assert.equal(isStatusPillPresent(), false);
});

test('a save status yields to startup and owns the pill afterwards', () => {
    const doc = fakeDocument();
    globalThis.document = /** @type {any} */ (doc);

    // Startup is still running with its pill up: the save status stays silent.
    const startupPill = ensureStatusPill();
    startupPill.textContent = 'Starting: loading core data…';
    const pill = createSaveStatusPill();

    pill.show('savechat 53%');
    assert.equal(startupPill.textContent, 'Starting: loading core data…', 'the startup line is untouched');

    // Startup finishes and removes it.
    startupPill.remove();
    pill.show('savechat 60%');
    assert.equal(doc.getElementById(STATUS_PILL_ID)?.textContent, 'savechat 60%');

    pill.show('savechat 100%');
    assert.equal(doc.getElementById(STATUS_PILL_ID)?.textContent, 'savechat 100%');

    pill.hide();
    assert.equal(isStatusPillPresent(), false);
});
