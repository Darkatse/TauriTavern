import test from 'node:test';
import assert from 'node:assert/strict';
import { installFakeDom } from './helpers/fake-dom.mjs';

for (const [name, file, factory, wrapperClass, prefix] of [
    ['JS-Slash-Runner', 'js-slash-runner', 'createJsSlashRunnerRuntimeAdapter', 'TH-render', 'jsr'],
    ['LittleWhiteBox', 'littlewhitebox', 'createLittleWhiteBoxRuntimeAdapter', 'xiaobaix-iframe-wrapper', 'lwb'],
]) {
    test(`${name}: local blob recovery preserves sibling DOM and renderer guards without a message update`, async (t) => {
        const nativeQueueMicrotask = globalThis.queueMicrotask;
        const dom = installFakeDom();
        globalThis.queueMicrotask = nativeQueueMicrotask;
        const sources = [0, 1].map(index => new Blob([`<p>source ${index}</p>`], { type: 'text/html' }));
        const sourceUrls = sources.map(source => URL.createObjectURL(source));
        const read = Promise.withResolvers();
        const nativeFetch = globalThis.fetch;
        t.mock.method(globalThis, 'fetch', async (url) => {
            const index = sourceUrls.indexOf(url);
            if (index < 0) return nativeFetch(url);
            await read.promise;
            return { ok: true, blob: async () => sources[index] };
        });
        const registered = [];
        const events = await import('../src/scripts/events.js');
        const previousEmit = events.eventSource.emit;
        const emitted = [];
        events.eventSource.emit = async (...args) => { emitted.push(args); };
        try {
            const module = await import(`../src/tauri/main/adapters/embedded-runtime/${file}-runtime-adapter.js`);
            const adapter = module[factory]();
            const message = document.createElement('div');
            message.className = 'mes';
            message.setAttribute('mesid', '42');
            document.body.append(message);
            const manager = {
                profileConfig: { maxSoftParkedIframes: 2, softParkTtlMs: 1000 },
                invalidate() {},
                register(slot) {
                    registered.push(slot);
                    slot.element.dataset.ttRuntimeSlotId = slot.id;
                },
            };
            const frames = sourceUrls.map((url, index) => {
                const wrapper = document.createElement('div');
                wrapper.className = wrapperClass;
                const iframe = document.createElement('iframe');
                iframe.src = url;
                const pre = document.createElement('pre');
                const code = document.createElement('code');
                code.textContent = `source ${index}`;
                pre.append(code);
                pre.dataset.xbFinal = 'true';
                pre.dataset.xbHash = `hash-${index}`;
                wrapper.append(iframe);
                message.append(wrapper);
                if (prefix === 'jsr') {
                    wrapper.append(pre);
                } else {
                    message.append(pre);
                }
                adapter.registerHost(manager, wrapper);
                return { wrapper, iframe, pre };
            });
            assert.equal(registered.length, 2);
            assert.ok(registered.every(slot => slot.id.startsWith(`${prefix}:42:`)));
            read.resolve();
            // Drain the controlled in-memory reads independently of scheduling notifications.
            await new Promise(resolve => setImmediate(resolve));
            for (const slot of registered) {
                slot.hydrate();
            }
            registered[1].dehydrate('visibility');
            URL.revokeObjectURL(sourceUrls[1]);
            registered[1].hydrate();

            assert.equal(emitted.some(([event]) => event === events.event_types.MESSAGE_UPDATED), false,
                'a resource restore must not impersonate a message update');
            assert.equal(frames[0].wrapper.querySelector('iframe'), frames[0].iframe);
            assert.equal(frames[0].iframe.src, sourceUrls[0]);
            assert.equal(frames[1].wrapper.querySelector('iframe'), frames[1].iframe);
            assert.notEqual(frames[1].iframe.src, sourceUrls[1]);
            assert.equal(await (await fetch(frames[1].iframe.src)).text(), '<p>source 1</p>');
            assert.equal(frames[1].pre.dataset.xbFinal, 'true');
            assert.equal(frames[1].pre.dataset.xbHash, 'hash-1');
            adapter.registerHost(manager, frames[1].wrapper);
            assert.equal(registered.length, 2);
        } finally {
            for (const slot of registered) slot.dispose();
            for (const url of sourceUrls) URL.revokeObjectURL(url);
            events.eventSource.emit = previousEmit;
            dom.cleanup();
        }
    });
}
