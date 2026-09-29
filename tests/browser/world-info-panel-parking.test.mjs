import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserRuntime } from './runtime.mjs';

/**
 * Starts the app with Panel Runtime parking installed and the World Info drawer parked.
 */
async function startWithParkedWorldInfo() {
    const runtime = createBrowserRuntime();
    const { window, getModule, load, startHost } = runtime;
    await startHost();
    await load('script.js');
    const { document } = window;
    // happy-dom does not provide the legacy Option() constructor that world-info.js uses.
    window.Option ??= function Option(text = '', value = text) {
        const option = document.createElement('option');
        option.textContent = text;
        option.value = value;
        return option;
    };
    window.fetch = async (url) => url === '/api/settings/get'
        ? new window.Response(JSON.stringify({ world_names: ['Alpha', 'Beta', 'Gamma'] }))
        : new window.Response('{}');

    const { createEmbeddedRuntimeManager } = (await load('tauri/main/services/embedded-runtime/embedded-runtime-manager.js')).namespace;
    const { resolvePanelRuntimeProfile } = (await load('tauri/main/services/panel-runtime/panel-runtime-profiles.js')).namespace;
    const { installTopSettingsPanelParking } = (await load('tauri/main/adapters/panel-runtime/top-settings-panel-parking.js')).namespace;
    const manager = createEmbeddedRuntimeManager({ profile: resolvePanelRuntimeProfile('compat') });
    installTopSettingsPanelParking({ manager });
    assert.equal(document.getElementById('world_info'), null, 'the closed World Info drawer is parked');

    const openWorldInfoDrawer = async () => {
        manager.setVisible('panel:WorldInfo', true);
        manager.reconcile();
        await new Promise(resolve => window.setTimeout(resolve, 0));
    };
    const optionStates = id => Array.from(document.getElementById(id).options)
        .filter(option => option.value !== '')
        .map(option => [option.value, option.textContent, option.selected]);

    return { window, worldInfo: getModule('scripts/world-info.js').namespace, openWorldInfoDrawer, optionStates };
}

test('World Info lists catch up with updates made while their drawer was parked', async () => {
    const { window, worldInfo, openWorldInfoDrawer, optionStates } = await startWithParkedWorldInfo();
    try {
        // A card import links a new lorebook while the drawer is closed.
        worldInfo.selected_world_info.push('Gamma');
        await worldInfo.updateWorldInfoList();

        await openWorldInfoDrawer();

        assert.deepEqual(optionStates('world_info'), [['0', 'Alpha', false], ['1', 'Beta', false], ['2', 'Gamma', true]]);
        assert.deepEqual(optionStates('world_editor_select'), [['0', 'Alpha', false], ['1', 'Beta', false], ['2', 'Gamma', false]]);
    } finally {
        await window.happyDOM.close();
    }
});

test('/world activates a lorebook while the World Info drawer is parked', async () => {
    const { window, worldInfo, openWorldInfoDrawer, optionStates } = await startWithParkedWorldInfo();
    try {
        await worldInfo.updateWorldInfoList();

        worldInfo.onWorldInfoChange({ state: 'on', silent: 'true' }, 'beta');

        assert.deepEqual([...worldInfo.selected_world_info], ['Beta']);
        await openWorldInfoDrawer();
        assert.deepEqual(optionStates('world_info'), [['0', 'Alpha', false], ['1', 'Beta', true], ['2', 'Gamma', false]]);
    } finally {
        await window.happyDOM.close();
    }
});
