import { expect, test } from '@rstest/core';
import { frontendText, importFrontend } from '../helpers/frontend.mjs';

const indexHtml = await frontendText('index.html');
const { createEmbeddedRuntimeManager } = await importFrontend('tauri/main/services/embedded-runtime/embedded-runtime-manager.js');
const { resolvePanelRuntimeProfile } = await importFrontend('tauri/main/services/panel-runtime/panel-runtime-profiles.js');
const { installTopSettingsPanelParking } = await importFrontend('tauri/main/adapters/panel-runtime/top-settings-panel-parking.js');
const { activateApiConnectionsSubtreeGates } = await importFrontend('tauri/main/adapters/panel-runtime/api-connections-subtree-gates.js');

// Assert against the shipped markup: index.html is merged from upstream periodically.
function renderIndexBody() {
    const html = indexHtml.replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '');
    const start = html.indexOf('>', html.indexOf('<body')) + 1;
    document.body.innerHTML = html.slice(start, html.lastIndexOf('</body>'));
}

function installParking(profileName) {
    const manager = createEmbeddedRuntimeManager({ profile: resolvePanelRuntimeProfile(profileName) });
    const { pinnedSelectors } = installTopSettingsPanelParking({ manager });
    return { manager, pinnedSelectors };
}

test('compat parking keeps the OpenAI bounds and third-party anchors reachable', () => {
    renderIndexBody();
    const { manager, pinnedSelectors } = installParking('compat');

    // Drawers ship closed, so installing already parked the left nav.
    expect(document.querySelector('#left-nav-panel .scrollableInner')).toBeNull();
    expect(document.getElementById('openai_max_context')).not.toBeNull();
    expect(document.getElementById('openai_api-presets')).not.toBeNull();
    expect(document.getElementById('completion_prompt_manager')).not.toBeNull();

    manager.setVisible('panel:left-nav-panel', true);
    manager.reconcile();

    const scrollable = document.querySelector('#left-nav-panel .scrollableInner');
    expect(scrollable).not.toBeNull();
    for (const selector of pinnedSelectors) {
        expect(scrollable.contains(document.querySelector(selector)), `${selector} must return to the parked root`).toBe(true);
    }
});

test('aggressive parking keeps the OpenAI bounds but drops the third-party anchors', () => {
    renderIndexBody();
    installParking('aggressive');

    expect(document.querySelector('#left-nav-panel .scrollableInner')).toBeNull();
    expect(document.getElementById('openai_max_context')).not.toBeNull();
    expect(document.getElementById('openai_api-presets')).toBeNull();
});

test('API connections gates keep the custom form for a saved custom source variant', () => {
    renderIndexBody();
    document.getElementById('main_api').value = 'openai';
    document.getElementById('chat_completion_source').value = 'custom_gemini_generate_content';
    activateApiConnectionsSubtreeGates({ manager: createEmbeddedRuntimeManager({ profile: resolvePanelRuntimeProfile('compat') }) });

    expect(document.getElementById('custom_api_url_text')).not.toBeNull();
    expect(document.getElementById('custom_model_id')).not.toBeNull();
    expect(document.getElementById('model_custom_select')).not.toBeNull();
    expect(document.getElementById('openai_form').children).toHaveLength(0);
});
