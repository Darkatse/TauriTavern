import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserRuntime } from './runtime.mjs';

test('World Info lists catch up with updates made while their drawer was parked', async () => {
    const { window, getModule, load, startHost } = createBrowserRuntime();
    try {
        await startHost();
        await load('script.js');
        const worldInfo = getModule('scripts/world-info.js').namespace;
        const { document } = window;
        // happy-dom does not provide the legacy Option() constructor that world-info.js uses.
        window.Option ??= function Option(text = '', value = text) {
            const option = document.createElement('option');
            option.textContent = text;
            option.value = value;
            return option;
        };
        window.fetch = async (url) => {
            assert.equal(url, '/api/settings/get');
            return new window.Response(JSON.stringify({ world_names: ['Alpha', 'Beta', 'Gamma'] }));
        };
        const optionStates = id => Array.from(document.getElementById(id).options)
            .filter(option => option.value !== '')
            .map(option => [option.value, option.textContent, option.selected]);

        const { createEmbeddedRuntimeManager } = (await load('tauri/main/services/embedded-runtime/embedded-runtime-manager.js')).namespace;
        const { resolvePanelRuntimeProfile } = (await load('tauri/main/services/panel-runtime/panel-runtime-profiles.js')).namespace;
        const { installTopSettingsPanelParking } = (await load('tauri/main/adapters/panel-runtime/top-settings-panel-parking.js')).namespace;
        const manager = createEmbeddedRuntimeManager({ profile: resolvePanelRuntimeProfile('compat') });
        installTopSettingsPanelParking({ manager });
        assert.equal(document.getElementById('world_info'), null, 'the closed World Info drawer is parked');

        // A card import links a new lorebook while the drawer is closed.
        worldInfo.selected_world_info.push('Gamma');
        await worldInfo.updateWorldInfoList();

        manager.setVisible('panel:WorldInfo', true);
        manager.reconcile();
        await new Promise(resolve => window.setTimeout(resolve, 0));

        assert.deepEqual(optionStates('world_info'), [['0', 'Alpha', false], ['1', 'Beta', false], ['2', 'Gamma', true]]);
        assert.deepEqual(optionStates('world_editor_select'), [['0', 'Alpha', false], ['1', 'Beta', false], ['2', 'Gamma', false]]);
    } finally {
        await window.happyDOM.close();
    }
});
