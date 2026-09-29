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
    // These tests cover which lorebook the editor shows, not how its entries are paged.
    window.jQuery.fn.pagination = function () { return this; };
    const backend = { worldNames: ['Alpha', 'Beta', 'Gamma'] };
    window.fetch = async (url, init) => {
        switch (url) {
            case '/api/settings/get':
                return new window.Response(JSON.stringify({ world_names: backend.worldNames }));
            case '/api/worldinfo/sanitize-name':
                return new window.Response(JSON.stringify({ name: JSON.parse(init.body).name }));
            default:
                return new window.Response(JSON.stringify({ entries: {} }));
        }
    };

    // Detaching an ancestor makes happy-dom point <option>.parentNode at the raw <select> behind
    // its Proxy, so jQuery stops finding pre-existing options once Panel Runtime restores the
    // drawer. Browsers keep one identity; re-append them under the select to match.
    const insertBefore = window.Node.prototype.insertBefore;
    window.Node.prototype.insertBefore = function (node, child) {
        const inserted = insertBefore.call(this, node, child);
        if (node.id === 'wi-holder') {
            for (const select of node.querySelectorAll('select')) {
                const selected = Array.from(select.options, option => option.selected);
                select.append(...select.childNodes);
                Array.from(select.options).forEach((option, index) => { option.selected = selected[index]; });
            }
        }
        return inserted;
    };

    const worldInfo = getModule('scripts/world-info.js').namespace;
    worldInfo.initWorldInfo();
    const editorSelect = document.getElementById('world_editor_select');

    const { createEmbeddedRuntimeManager } = (await load('tauri/main/services/embedded-runtime/embedded-runtime-manager.js')).namespace;
    const { resolvePanelRuntimeProfile } = (await load('tauri/main/services/panel-runtime/panel-runtime-profiles.js')).namespace;
    const { installTopSettingsPanelParking } = (await load('tauri/main/adapters/panel-runtime/top-settings-panel-parking.js')).namespace;
    const manager = createEmbeddedRuntimeManager({ profile: resolvePanelRuntimeProfile('compat') });
    installTopSettingsPanelParking({ manager });
    assert.equal(document.getElementById('world_info'), null, 'the closed World Info drawer is parked');

    // Leaves the parked editor as an earlier render would have: these books, one of them picked.
    const seedParkedEditor = (names, picked) => {
        for (const [index, name] of names.entries()) {
            const option = document.createElement('option');
            option.value = String(index);
            option.textContent = name;
            option.selected = name === picked;
            editorSelect.append(option);
        }
    };
    const openWorldInfoDrawer = async () => {
        manager.setVisible('panel:WorldInfo', true);
        manager.reconcile();
        await new Promise(resolve => window.setTimeout(resolve, 0));
    };
    const optionStates = id => Array.from(document.getElementById(id).options)
        .filter(option => option.value !== '')
        .map(option => [option.value, option.textContent, option.selected]);
    const editorWorld = () => {
        const option = editorSelect.options[editorSelect.selectedIndex];
        return option?.value ? option.textContent : '';
    };
    const editorReloads = [];
    window.jQuery(editorSelect).on('change', () => editorReloads.push(editorWorld()));

    return { window, backend, worldInfo, seedParkedEditor, openWorldInfoDrawer, optionStates, editorWorld, editorReloads };
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

test('opening a lorebook from outside the parked drawer selects that lorebook', async () => {
    const { window, worldInfo, seedParkedEditor, editorWorld } = await startWithParkedWorldInfo();
    try {
        // Index 1 still belongs to Gamma in the parked options when Beta is added.
        seedParkedEditor(['Alpha', 'Gamma'], null);
        await worldInfo.updateWorldInfoList();
        // Served from cache, so the editor does not read a response after the window closes.
        worldInfo.worldInfoCache.set('Beta', { entries: {} });

        // What openWorldInfoEditor() and importEmbeddedWorldInfo() do: open the drawer, then pick by index.
        window.jQuery('#WIDrawerIcon').trigger('click');
        window.jQuery('#world_editor_select').val(String(worldInfo.world_names.indexOf('Beta'))).trigger('change');

        assert.equal(editorWorld(), 'Beta');
    } finally {
        await window.happyDOM.close();
    }
});

test('the editor reloads its lorebook when it was saved elsewhere while parked', async () => {
    const { window, worldInfo, seedParkedEditor, openWorldInfoDrawer, editorReloads } = await startWithParkedWorldInfo();
    try {
        seedParkedEditor(['Alpha', 'Beta', 'Gamma'], 'Alpha');
        await worldInfo.updateWorldInfoList();

        // e.g. /setentryfield or a script edits the open lorebook while the drawer is closed.
        await worldInfo.saveWorldInfo('Alpha', { entries: {} });
        await openWorldInfoDrawer();

        assert.deepEqual(editorReloads, ['Alpha']);
    } finally {
        await window.happyDOM.close();
    }
});

test('the editor closes its lorebook when it was deleted while parked', async () => {
    const { window, backend, worldInfo, seedParkedEditor, openWorldInfoDrawer, editorReloads } = await startWithParkedWorldInfo();
    try {
        seedParkedEditor(['Alpha', 'Beta', 'Gamma'], 'Alpha');
        backend.worldNames = ['Beta', 'Gamma'];
        await worldInfo.updateWorldInfoList();

        await openWorldInfoDrawer();

        assert.deepEqual(editorReloads, ['']);
    } finally {
        await window.happyDOM.close();
    }
});

test('a lorebook created while parked opens in the editor with the drawer', async () => {
    const { window, backend, worldInfo, seedParkedEditor, openWorldInfoDrawer, editorWorld, editorReloads } = await startWithParkedWorldInfo();
    try {
        seedParkedEditor(['Alpha', 'Beta', 'Gamma'], 'Alpha');
        await worldInfo.updateWorldInfoList();
        backend.worldNames = ['Alpha', 'Beta', 'Delta', 'Gamma'];

        // What /createlore does while the drawer is closed.
        await worldInfo.createNewWorldInfo('Delta');
        await openWorldInfoDrawer();

        assert.equal(editorWorld(), 'Delta');
        assert.deepEqual(editorReloads, ['Delta']);
    } finally {
        await window.happyDOM.close();
    }
});
