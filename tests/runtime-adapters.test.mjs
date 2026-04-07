import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { installFakeDom } from './helpers/fake-dom.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

async function importStable(modulePath) {
    return import(pathToFileURL(modulePath).href);
}

test('JS-Slash-Runner adapter registers a managed slot and cold rebuild emits MESSAGE_UPDATED', async () => {
    const dom = installFakeDom();
    try {
        const { createJsSlashRunnerRuntimeAdapter } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/js-slash-runner-runtime-adapter.js'),
        );
        const { EmbeddedRuntimeKind } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/runtime-kinds.js'),
        );
        const events = await importStable(path.join(REPO_ROOT, 'src/scripts/events.js'));

        const emitted = [];
        const prevEmit = events.eventSource.emit;
        events.eventSource.emit = async (event, ...args) => {
            emitted.push({ event, args });
        };

        try {
            const message = document.createElement('div');
            message.classList.add('mes');
            message.setAttribute('mesid', '42');
            document.body.append(message);

            const wrapper = document.createElement('div');
            wrapper.classList.add('TH-render');

            const iframe = document.createElement('iframe');
            iframe.src = 'blob:jsr';

            const pre = document.createElement('pre');
            const code = document.createElement('code');
            code.textContent = 'signature';
            pre.append(code);

            wrapper.append(iframe, pre);
            message.append(wrapper);

            const registered = [];
            const manager = {
                profileConfig: { maxSoftParkedIframes: 0, softParkTtlMs: 0 },
                register(slot) {
                    registered.push(slot);
                    slot.element.dataset.ttRuntimeSlotId = slot.id;
                },
            };

            const adapter = createJsSlashRunnerRuntimeAdapter();
            adapter.registerHost(manager, wrapper);

            assert.equal(registered.length, 1);
            const slot = registered[0];
            assert.equal(slot.kind, EmbeddedRuntimeKind.JsrHtmlRender);
            assert.ok(slot.id.startsWith('jsr:42:'));
            assert.equal(wrapper.dataset.ttRuntimeSlotId, slot.id);

            slot.hydrate();
            iframe.remove();
            slot.hydrate();

            assert.deepEqual(emitted, [{ event: events.event_types.MESSAGE_UPDATED, args: ['42'] }]);
            assert.equal(wrapper.querySelector('iframe'), null);

            adapter.registerHost(manager, wrapper);
            assert.equal(registered.length, 1);
        } finally {
            events.eventSource.emit = prevEmit;
        }
    } finally {
        dom.cleanup();
    }
});

test('LittleWhiteBox adapter registers a managed slot and cold rebuild emits MESSAGE_UPDATED', async () => {
    const dom = installFakeDom();
    try {
        const { createLittleWhiteBoxRuntimeAdapter } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/adapters/embedded-runtime/littlewhitebox-runtime-adapter.js'),
        );
        const { EmbeddedRuntimeKind } = await importFresh(
            path.join(REPO_ROOT, 'src/tauri/main/services/embedded-runtime/runtime-kinds.js'),
        );
        const events = await importStable(path.join(REPO_ROOT, 'src/scripts/events.js'));

        const emitted = [];
        const prevEmit = events.eventSource.emit;
        events.eventSource.emit = async (event, ...args) => {
            emitted.push({ event, args });
        };

        try {
            const message = document.createElement('div');
            message.classList.add('mes');
            message.setAttribute('mesid', '7');
            document.body.append(message);

            const wrapper = document.createElement('div');
            wrapper.classList.add('xiaobaix-iframe-wrapper');

            const iframe = document.createElement('iframe');
            iframe.src = 'blob:lwb';
            wrapper.append(iframe);

            const pre = document.createElement('pre');
            const code = document.createElement('code');
            code.textContent = 'signature';
            pre.append(code);
            pre.dataset.xbHash = 'hash';

            message.append(wrapper, pre);

            const registered = [];
            const manager = {
                profileConfig: { maxSoftParkedIframes: 0, softParkTtlMs: 0 },
                register(slot) {
                    registered.push(slot);
                    slot.element.dataset.ttRuntimeSlotId = slot.id;
                },
            };

            const adapter = createLittleWhiteBoxRuntimeAdapter();
            adapter.registerHost(manager, wrapper);

            assert.equal(registered.length, 1);
            const slot = registered[0];
            assert.equal(slot.kind, EmbeddedRuntimeKind.LittleWhiteBoxHtmlRender);
            assert.ok(slot.id.startsWith('lwb:7:'));
            assert.equal(wrapper.dataset.ttRuntimeSlotId, slot.id);

            slot.hydrate();
            iframe.remove();
            slot.hydrate();

            assert.deepEqual(emitted, [{ event: events.event_types.MESSAGE_UPDATED, args: ['7'] }]);
            assert.equal(wrapper.querySelector('iframe'), null);
        } finally {
            events.eventSource.emit = prevEmit;
        }
    } finally {
        dom.cleanup();
    }
});
