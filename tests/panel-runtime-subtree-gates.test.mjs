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

test('gate activation with a saved custom source variant keeps the custom form connected', async () => {
    const dom = installFakeDom();
    try {
        renderIndexBody(dom.document);

        const { createEmbeddedRuntimeManager } = await import('../src/tauri/main/services/embedded-runtime/embedded-runtime-manager.js');
        const { resolvePanelRuntimeProfile } = await import('../src/tauri/main/services/panel-runtime/panel-runtime-profiles.js');
        const { activateApiConnectionsSubtreeGates } = await import('../src/tauri/main/adapters/panel-runtime/api-connections-subtree-gates.js');

        const manager = createEmbeddedRuntimeManager({ profile: resolvePanelRuntimeProfile('compat') });

        const mainApi = dom.document.getElementById('main_api');
        mainApi.value = 'openai';
        // Saved variant values (custom_openai_responses / custom_claude_messages /
        // custom_gemini_generate_content / custom_gemini_interactions) must gate like 'custom'.
        const source = dom.document.getElementById('chat_completion_source');
        source.value = 'custom_gemini_generate_content';

        activateApiConnectionsSubtreeGates({ manager });

        // The Custom Endpoint form must keep its children: the URL input, the model
        // id input and the Available Models select stay in the document.
        const customForm = dom.document.getElementById('custom_form');
        assert.ok(customForm, '#custom_form must exist');
        assert.ok(customForm.children.length > 0, 'the custom form must not be parked on activation');
        assert.ok(dom.document.getElementById('custom_api_url_text'), 'custom URL input must stay connected');
        assert.ok(dom.document.getElementById('custom_model_id'), 'custom model id input must stay connected');
        assert.ok(dom.document.getElementById('model_custom_select'), 'custom model select must stay connected');

        // The gate must still park hosts that do not match the normalized source:
        // the plain OpenAI form (data-source="openai") is parked while 'custom' is active.
        const openaiForm = dom.document.getElementById('openai_form');
        assert.ok(openaiForm, '#openai_form must exist');
        assert.equal(openaiForm.children.length, 0, 'the plain OpenAI form must stay parked for a custom source');
    } finally {
        dom.cleanup();
    }
});
