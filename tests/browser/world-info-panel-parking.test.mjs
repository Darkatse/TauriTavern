import assert from 'node:assert/strict';
import test from 'node:test';
import { createBrowserRuntime } from './runtime.mjs';

test('editing a reattached lorebook does not overwrite changes made while detached', async context => {
    const { window, getModule, load, startHost } = createBrowserRuntime();
    context.after(() => window.happyDOM.close());
    await startHost();
    await load('script.js');
    const worldInfo = getModule('scripts/world-info.js').namespace;
    const $ = window.jQuery;

    // Observe completion without replacing the pagination plugin or using sleeps.
    let pageRendered;
    const pagination = $.fn.pagination;
    $.fn.pagination = function (options, ...args) {
        if (options && typeof options === 'object') {
            const callback = options.callback;
            options.callback = (...args) => (pageRendered = callback(...args));
        }
        return pagination.call(this, options, ...args);
    };
    Object.assign($.fn.pagination, pagination);
    const render = async () => {
        await worldInfo.showWorldEditor('Book');
        await pageRendered;
    };

    let saved;
    window.fetch = async (url, init) => {
        if (url !== '/api/worldinfo/edit') throw new Error(`Unexpected request: ${url}`);
        saved = JSON.parse(init.body).data;
        return new window.Response('{}');
    };
    const data = { entries: { 0: { ...structuredClone(worldInfo.newWorldInfoEntryTemplate), uid: 0, content: 'original' } } };
    worldInfo.worldInfoCache.set('Book', data);
    await render();

    const list = window.document.getElementById('world_popup_entries_list');
    const parent = list.parentNode;
    list.remove();
    const updated = structuredClone(data);
    updated.entries[0].content = 'external update';
    await worldInfo.saveWorldInfo('Book', updated, true);
    await render();
    parent.appendChild(list);

    const order = window.document.querySelector('#world_popup_entries_list input[name="order"]');
    order.value = '101';
    await $(order).triggerHandler('input');
    await worldInfo.flushWorldInfoSaves();
    assert.equal(saved.entries[0].content, 'external update');
    assert.equal(saved.entries[0].order, 101);
});
