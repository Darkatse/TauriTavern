import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as virtualCore from '@tanstack/virtual-core';

import { installFakeDom } from './helpers/fake-dom.mjs';
import { createChatScrollAdapter } from '../src/tauri/main/adapters/chat-surface/chat-scroll-adapter.js';
import { createTanStackVirtualAdapter } from '../src/tauri/main/adapters/chat-surface/tanstack-virtual-adapter.js';
import {
    CHAT_VIRTUAL_ESTIMATE_PX,
    CHAT_VIRTUAL_MAX_VIEWPORT_ITEMS,
} from '../src/tauri/main/kernel/chat-surface/virtualization-config.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('pinned TanStack adapter exposes tail, normal and forced geometry without leaking policy', () => {
    const dom = installFakeDom();
    try {
        const root = document.createElement('div');
        root._setRect({ width: 800, height: 600 });
        root.style.paddingTop = '10px';
        root.style.paddingBottom = '20px';
        root.style.rowGap = '5px';
        document.body.append(root);

        const changes = [];
        const adapter = createTanStackVirtualAdapter({
            root,
            onGeometryChange: change => changes.push(change),
            virtualCore,
            scrollToFn: createChatScrollAdapter(root).virtualScrollTo,
        });
        adapter.mount();
        const keys = Object.freeze(Array.from({ length: 100 }, (_value, index) => `key-${index}`));
        adapter.setStructure(keys);
        const tail = adapter.geometry();
        assert.deepEqual(tail.viewportItems, []);
        assert.deepEqual(tail.projectedItems.map(item => item.index), [99]);
        assert.equal(tail.projectedItems[0].key, 'key-99');
        assert.deepEqual(tail.metrics, { paddingStart: 10, paddingEnd: 20, gap: 5 });

        adapter.setMode('normal');
        const normal = adapter.geometry();
        assert.ok(normal.viewportItems.length > 0);
        assert.ok(normal.viewportItems.length <= CHAT_VIRTUAL_MAX_VIEWPORT_ITEMS);
        assert.equal(normal.projectedItems.at(-1).index, 99);

        changes.length = 0;
        root.scrollHeight = 20_000;
        root.scrollTo({ top: 100 });
        assert.deepEqual(changes.at(-1), { scrolling: true, programmatic: false });

        const message = document.createElement('div');
        message.classList.add('mes');
        message.setAttribute('data-tt-virtual-index', '0');
        message._setRect({ width: 800, height: 480 });
        root.append(message);
        adapter.measure([message]);
        const messageObserver = dom.createdResizeObservers.find(observer => observer._targets.has(message));
        messageObserver._trigger([{ target: message, borderBoxSize: [{ blockSize: 480 }] }]);
        assert.equal(changes.at(-1).scrolling, true);

        root.dispatchEvent({ type: 'scrollend', target: root });
        assert.equal(changes.at(-1).scrolling, false);
        assert.equal(Object.hasOwn(changes.at(-1), 'sync'), false);

        adapter.force(50);
        const forced = adapter.geometry();
        assert.ok(forced.viewportItems.some(item => item.index === 50));
        assert.equal(forced.projectedItems.at(-1).index, 99);
        assert.ok(changes.length > 0);

        adapter.dispose();
        adapter.reset();
    } finally {
        dom.cleanup();
    }
});

test('layout invalidation drops measurements for unmounted messages', () => {
    const dom = installFakeDom();
    try {
        const root = document.createElement('div');
        root._setRect({ width: 800, height: 600 });
        document.body.append(root);
        const adapter = createTanStackVirtualAdapter({
            root,
            onGeometryChange() {},
            virtualCore,
            scrollToFn: createChatScrollAdapter(root).virtualScrollTo,
        });
        adapter.mount();
        adapter.setStructure(Object.freeze(Array.from({ length: 100 }, (_value, index) => `key-${index}`)));
        adapter.setMode('normal');

        const message = document.createElement('div');
        message.classList.add('mes');
        message.setAttribute('data-tt-virtual-index', '0');
        message._setRect({ width: 800, height: 480 });
        root.append(message);
        adapter.measure([message]);
        const messageObserver = dom.createdResizeObservers.find(observer => observer._targets.has(message));
        messageObserver._trigger([{ target: message, borderBoxSize: [{ blockSize: 480 }] }]);
        assert.equal(adapter.geometry().viewportItems.find(item => item.index === 0).size, 480);

        message.remove();
        adapter.invalidateMeasurements();
        assert.equal(adapter.geometry().viewportItems.find(item => item.index === 0).size, CHAT_VIRTUAL_ESTIMATE_PX);

        adapter.dispose();
        adapter.reset();
    } finally {
        dom.cleanup();
    }
});

test('TanStack private lifecycle is isolated to the pinned adapter', async () => {
    const adapterSource = await readFile(
        path.join(REPO_ROOT, 'src/tauri/main/adapters/chat-surface/tanstack-virtual-adapter.js'),
        'utf8',
    );
    assert.doesNotMatch(adapterSource, /from '@tanstack\/virtual-core'/);
    assert.match(adapterSource, /virtualizer\._didMount\(\)/);
    assert.match(adapterSource, /virtualizer\._willUpdate\(\)/);
    assert.doesNotMatch(adapterSource, /virtualizer\.(?:range|getMeasurements)\b/);

    const vendorSource = await readFile(path.join(REPO_ROOT, 'src/lib-bundle-core.js'), 'utf8');
    assert.match(vendorSource, /from '@tanstack\/virtual-core'/);

    const sourceFiles = [
        'src/tauri/main/services/chat-surface/bounded-chat-surface.js',
        'src/tauri/main/services/chat-surface/chat-surface-controller.js',
        'src/script.js',
    ];
    for (const sourceFile of sourceFiles) {
        const source = await readFile(path.join(REPO_ROOT, sourceFile), 'utf8');
        assert.doesNotMatch(source, /virtualizer\.(?:_didMount|_willUpdate|range|getMeasurements)\b/, sourceFile);
        assert.doesNotMatch(source, /from '@tanstack\/virtual-core'/, sourceFile);
    }
});

test('composition invalidates global layout changes and offers explicit fault recovery', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const install = await readFile(
        path.join(REPO_ROOT, 'src/tauri/main/services/chat-surface/install.js'),
        'utf8',
    );

    assert.match(install, /addEventListener\(CHAT_LAYOUT_CHANGED_EVENT, scheduleLayoutRefresh\)/);
    assert.doesNotMatch(install, /SETTINGS_UPDATED/);
    assert.match(install, /addEventListener\(DYNAMIC_THEME_CHANGED_EVENT, scheduleLayoutRefresh\)/);
    assert.match(install, /root\.clientWidth !== lastLayoutWidth/);
    assert.match(install, /addEventListener\('resize'/);
    assert.match(install, /document\.fonts\?\.addEventListener\?\.\('loadingdone', scheduleLayoutRefresh\)/);
    assert.match(source, /updateTauriTavernSettings\(\{ chat_virtualization_enabled: false \}\)/);
    assert.match(source, /cancelButton:\s*startup \? t`Abort startup` : t`Keep stopped`/);
    assert.match(source, /message\.startsWith\('Bounded ChatSurface requires extension "'\)/);
    assert.match(source, /JS-Slash-Runner 4\.9\.1 or later/);
    assert.match(source, /LittleWhiteBox 3\.0\.4 or later/);
    assert.match(source, /https:\/\/github\.com\/N0VI028\/JS-Slash-Runner/);
    assert.match(source, /https:\/\/github\.com\/RT15548\/LittleWhiteBox/);
    assert.doesNotMatch(source, /github\.com\/Darkatse\/(?:JS-Slash-Runner|LittleWhiteBox)/);
    assert.match(source, /catch \(error\) \{\s*await offerChatVirtualizationRecovery\(error, \{ startup: true \}\);\s*throw error;/);
    assert.match(source, /onFault: error => \{[\s\S]*?offerChatVirtualizationRecovery\(error\);[\s\S]*?\},/);
});
