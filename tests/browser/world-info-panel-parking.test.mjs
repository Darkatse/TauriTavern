import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserRuntime, createPaginationWaiter } from './runtime.mjs';

test('editing a reattached lorebook does not overwrite changes made while detached', async context => {
    const { window, getModule, load, startHost } = await createBrowserRuntime();
    context.after(() => window.happyDOM.close());
    await startHost();
    await load('script.js');
    const worldInfo = getModule('scripts/world-info.js').namespace;
    const $ = window.jQuery;

    const waitForPagination = createPaginationWaiter(window);

    let saved;
    window.fetch = async (url, init) => {
        if (url !== '/api/worldinfo/edit') throw new Error(`Unexpected request: ${url}`);
        saved = JSON.parse(init.body).data;
        return new window.Response('{}');
    };
    const data = { entries: { 0: { ...structuredClone(worldInfo.newWorldInfoEntryTemplate), uid: 0, content: 'original' } } };
    worldInfo.worldInfoCache.set('Book', data);
    await waitForPagination(() => worldInfo.showWorldEditor('Book'));

    const list = window.document.getElementById('world_popup_entries_list');
    const parent = list.parentNode;
    list.remove();
    const updated = structuredClone(data);
    updated.entries[0].content = 'external update';
    await worldInfo.saveWorldInfo('Book', updated, true);
    await waitForPagination(() => worldInfo.showWorldEditor('Book'));
    parent.appendChild(list);

    const order = window.document.querySelector('#world_popup_entries_list input[name="order"]');
    order.value = '101';
    await $(order).triggerHandler('input');
    await worldInfo.flushWorldInfoSaves();
    assert.equal(saved.entries[0].content, 'external update');
    assert.equal(saved.entries[0].order, 101);
});
