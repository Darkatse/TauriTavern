import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { installFakeDom } from './helpers/fake-dom.mjs';

const INDEX_HTML = fileURLToPath(new URL('../src/index.html', import.meta.url));

// Assert against the shipped markup: index.html is merged from upstream periodically.
function renderIndexBody(document) {
    const html = readFileSync(INDEX_HTML, 'utf8').replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '');
    const start = html.indexOf('>', html.indexOf('<body')) + 1;
    document.body.innerHTML = html.slice(start, html.lastIndexOf('</body>'));
}

async function installParking(profileName) {
    const { createEmbeddedRuntimeManager } = await import('../src/tauri/main/services/embedded-runtime/embedded-runtime-manager.js');
    const { resolvePanelRuntimeProfile } = await import('../src/tauri/main/services/panel-runtime/panel-runtime-profiles.js');
    const { installTopSettingsPanelParking } = await import('../src/tauri/main/adapters/panel-runtime/top-settings-panel-parking.js');

    const manager = createEmbeddedRuntimeManager({ profile: resolvePanelRuntimeProfile(profileName) });
    const { pinnedSelectors } = installTopSettingsPanelParking({ manager });
    return { manager, pinnedSelectors };
}

test('compat parking keeps the OpenAI bounds and third-party anchors reachable', async () => {
    const dom = installFakeDom();
    try {
        renderIndexBody(dom.document);
        const { manager, pinnedSelectors } = await installParking('compat');

        // Drawers ship closed, so installing already parked the left nav.
        assert.equal(dom.document.querySelector('#left-nav-panel .scrollableInner'), null);

        assert.ok(dom.document.getElementById('openai_max_context'));
        assert.ok(dom.document.getElementById('openai_api-presets'));
        assert.ok(dom.document.getElementById('completion_prompt_manager'));

        manager.setVisible('panel:left-nav-panel', true);
        manager.reconcile();

        const scrollable = dom.document.querySelector('#left-nav-panel .scrollableInner');
        assert.ok(scrollable);
        for (const selector of pinnedSelectors) {
            assert.ok(scrollable.contains(dom.document.querySelector(selector)), `${selector} must return to the parked root`);
        }
    } finally {
        dom.cleanup();
    }
});

test('aggressive parking keeps the OpenAI bounds but drops the third-party anchors', async () => {
    const dom = installFakeDom();
    try {
        renderIndexBody(dom.document);
        await installParking('aggressive');

        assert.equal(dom.document.querySelector('#left-nav-panel .scrollableInner'), null);
        assert.ok(dom.document.getElementById('openai_max_context'));
        assert.equal(dom.document.getElementById('openai_api-presets'), null);
    } finally {
        dom.cleanup();
    }
});
