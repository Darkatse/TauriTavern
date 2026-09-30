import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserRuntime } from './runtime.mjs';

/**
 * Starts the app with compat Panel Runtime parking installed and the World Info drawer closed.
 */
async function startWithParkedWorldInfo() {
    const { window, getModule, load, startHost } = createBrowserRuntime();
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
    // Render every page in one synchronous pass, through world-info.js's own page callback.
    window.jQuery.fn.pagination = function (options) {
        if (options && typeof options === 'object') {
            this.data('renderedPage', options.callback(options.dataSource()));
        }
        return this;
    };

    const backend = {
        worldNames: ['Alpha', 'Beta', 'Gamma'],
        books: new Map(),
    };
    window.fetch = async (url, init) => {
        const body = init?.body ? JSON.parse(init.body) : {};
        switch (url) {
            case '/api/settings/get':
                return new window.Response(JSON.stringify({ world_names: backend.worldNames }));
            case '/api/worldinfo/get':
                return new window.Response(JSON.stringify(backend.books.get(body.name) ?? { entries: {} }));
            case '/api/worldinfo/sanitize-name':
                return new window.Response(JSON.stringify({ name: body.name }));
            default:
                return new window.Response(JSON.stringify({}));
        }
    };

    const worldInfo = getModule('scripts/world-info.js').namespace;
    worldInfo.initWorldInfo();
    await worldInfo.updateWorldInfoList();

    const { createEmbeddedRuntimeManager } = (await load('tauri/main/services/embedded-runtime/embedded-runtime-manager.js')).namespace;
    const { resolvePanelRuntimeProfile } = (await load('tauri/main/services/panel-runtime/panel-runtime-profiles.js')).namespace;
    const { installTopSettingsPanelParking } = (await load('tauri/main/adapters/panel-runtime/top-settings-panel-parking.js')).namespace;
    const manager = createEmbeddedRuntimeManager({ profile: resolvePanelRuntimeProfile('compat') });
    installTopSettingsPanelParking({ manager });

    // Editor renders chain through change handlers, fetches and the page callback.
    const settle = async () => {
        for (let i = 0; i < 20; i++) {
            await new Promise(resolve => window.setTimeout(resolve, 0));
            await window.jQuery('#world_info_pagination').data('renderedPage');
        }
    };
    const setWorldInfoDrawerOpen = async (open) => {
        manager.setVisible('panel:WorldInfo', open);
        manager.reconcile();
        await settle();
    };
    const optionStates = id => Array.from(document.getElementById(id).options)
        .filter(option => option.value !== '')
        .map(option => [option.value, option.textContent, option.selected]);
    const pickInEditor = async (name) => {
        window.jQuery('#world_editor_select').val(String(worldInfo.world_names.indexOf(name))).trigger('change');
        await settle();
    };
    const renderedComments = () => Array.from(
        document.querySelectorAll('#world_popup_entries_list .world_entry textarea[name="comment"]'),
        textarea => textarea.value,
    );
    const book = (...comments) => ({
        entries: Object.fromEntries(comments.map((comment, uid) => [uid, { uid, comment, key: [], keysecondary: [], content: '' }])),
    });

    return { window, document, backend, worldInfo, settle, setWorldInfoDrawerOpen, optionStates, pickInEditor, renderedComments, book };
}

test('compat parking keeps the World Info lists current while the drawer is closed', async () => {
    const { window, document, worldInfo, optionStates } = await startWithParkedWorldInfo();
    try {
        assert.equal(document.getElementById('world_popup_entries_list'), null, 'the entry list is parked');
        assert.ok(document.getElementById('world_info'), 'the lorebook selects stay connected');

        // A card import links a new lorebook that sorts before an active one.
        worldInfo.selected_world_info.push('Gamma');
        worldInfo.world_names.length = 0;
        await worldInfo.updateWorldInfoList();

        assert.deepEqual(optionStates('world_info'), [['0', 'Alpha', false], ['1', 'Beta', false], ['2', 'Gamma', true]]);
        assert.deepEqual(optionStates('world_editor_select'), [['0', 'Alpha', false], ['1', 'Beta', false], ['2', 'Gamma', false]]);
    } finally {
        await window.happyDOM.close();
    }
});

test('/world activates a lorebook while the World Info drawer is closed', async () => {
    const { window, worldInfo, optionStates } = await startWithParkedWorldInfo();
    try {
        worldInfo.onWorldInfoChange({ state: 'on', silent: 'true' }, 'beta');

        // The native change handler then reads the selection back from #world_info. happy-dom
        // leaves selectedIndex at -1 for multi-selects, so assert the options it would read.
        assert.deepEqual(optionStates('world_info'), [['0', 'Alpha', false], ['1', 'Beta', true], ['2', 'Gamma', false]]);
    } finally {
        await window.happyDOM.close();
    }
});

test('reopening the drawer shows entries saved elsewhere while it was closed', async () => {
    const { window, backend, worldInfo, settle, setWorldInfoDrawerOpen, pickInEditor, renderedComments, book } = await startWithParkedWorldInfo();
    try {
        backend.books.set('Alpha', book('first'));
        await setWorldInfoDrawerOpen(true);
        await pickInEditor('Alpha');
        assert.deepEqual(renderedComments(), ['first']);
        await setWorldInfoDrawerOpen(false);

        // What /createentry does while the drawer is closed.
        const data = await worldInfo.loadWorldInfo('Alpha');
        data.entries[1] = { uid: 1, comment: 'second', key: [], keysecondary: [], content: '' };
        await worldInfo.saveWorldInfo('Alpha', data, true);
        worldInfo.reloadEditor('Alpha');
        await settle();
        await setWorldInfoDrawerOpen(true);

        assert.deepEqual(renderedComments(), ['first', 'second']);
    } finally {
        await window.happyDOM.close();
    }
});

test('reopening the drawer closes a lorebook deleted while it was closed', async () => {
    const { window, backend, worldInfo, settle, setWorldInfoDrawerOpen, pickInEditor, renderedComments, book } = await startWithParkedWorldInfo();
    try {
        backend.books.set('Alpha', book('first'));
        await setWorldInfoDrawerOpen(true);
        await pickInEditor('Alpha');
        await setWorldInfoDrawerOpen(false);

        backend.worldNames = ['Beta', 'Gamma'];
        await worldInfo.deleteWorldInfo('Alpha');
        await settle();
        await setWorldInfoDrawerOpen(true);

        assert.deepEqual(renderedComments(), []);
    } finally {
        await window.happyDOM.close();
    }
});

test('reopening the drawer shows a lorebook created while it was closed', async () => {
    const { window, document, backend, worldInfo, settle, setWorldInfoDrawerOpen, pickInEditor, renderedComments, book } = await startWithParkedWorldInfo();
    try {
        backend.books.set('Alpha', book('first'));
        await setWorldInfoDrawerOpen(true);
        await pickInEditor('Alpha');
        await setWorldInfoDrawerOpen(false);

        // What /createlore does while the drawer is closed.
        backend.worldNames = ['Alpha', 'Beta', 'Delta', 'Gamma'];
        await worldInfo.createNewWorldInfo('Delta');
        await settle();
        await setWorldInfoDrawerOpen(true);

        const editorSelect = document.getElementById('world_editor_select');
        assert.equal(editorSelect.options[editorSelect.selectedIndex].textContent, 'Delta');
        assert.deepEqual(renderedComments(), []);
    } finally {
        await window.happyDOM.close();
    }
});
