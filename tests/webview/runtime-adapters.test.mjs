import { expect, rs, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';

const { eventSource, event_types } = await importFrontend('scripts/events.js');
const { createEmbeddedRuntimeManager } = await importFrontend('tauri/main/services/embedded-runtime/embedded-runtime-manager.js');
const { installDomEmbeddedRuntimeAdapter } = await importFrontend('tauri/main/adapters/embedded-runtime/dom-runtime-adapter.js');
const { createJsSlashRunnerRuntimeAdapter } = await importFrontend('tauri/main/adapters/embedded-runtime/js-slash-runner-runtime-adapter.js');
const { createLittleWhiteBoxRuntimeAdapter } = await importFrontend('tauri/main/adapters/embedded-runtime/littlewhitebox-runtime-adapter.js');

const nextLoad = iframe => new Promise(resolve => iframe.addEventListener('load', resolve, { once: true }));

for (const [createAdapter, prefix] of [[createJsSlashRunnerRuntimeAdapter, 'jsr'], [createLittleWhiteBoxRuntimeAdapter, 'lwb']]) {
    const adapter = createAdapter();
    test(`${adapter.hostSelector}: cold recoveries reload only their page and leave the sibling, source UI and message alone`, async () => {
        const emit = rs.spyOn(eventSource, 'emit');
        const manager = createEmbeddedRuntimeManager({ profile: {
            name: 'test', maxActiveWeight: 20, maxActiveIframes: 2, maxActiveSlots: 2,
            maxSoftParkedIframes: 0, softParkTtlMs: 0,
            parkWhenHiddenKinds: ['jsr_html_render', 'lwb_html_render'], rootMargin: '0px', threshold: 0,
        } });
        const message = document.createElement('div');
        message.className = 'mes';
        message.setAttribute('mesid', '1');
        document.body.append(message);
        const frames = [0, 1].map(index => {
            const host = document.createElement('div');
            host.className = adapter.hostSelector.slice(1);
            const iframe = document.createElement('iframe');
            const url = URL.createObjectURL(new Blob(['<input value="initial">'], { type: 'text/html' }));
            // The renderer's source block: JSR keeps it in the wrapper, LWB next to it.
            const source = document.createElement('pre');
            source.dataset.xbFinal = 'true';
            source.dataset.xbHash = `hash-${index}`;
            const frame = { host, iframe, url, source, loads: 0, loaded: nextLoad(iframe) };
            iframe.addEventListener('load', () => frame.loads++);
            iframe.src = url;
            host.append(iframe);
            message.append(host);
            (prefix === 'jsr' ? host : message).append(source);
            return frame;
        });
        let registrations = 0;
        let adapterHandle;
        try {
            await Promise.all(frames.map(frame => frame.loaded));
            adapterHandle = installDomEmbeddedRuntimeAdapter({
                manager: {
                    profileConfig: manager.profileConfig,
                    invalidate: manager.invalidate,
                    touch: manager.touch,
                    unregister: manager.unregister,
                    register: slot => {
                        registrations++;
                        manager.register({ ...slot, visibilityMode: 'manual', initialVisible: true });
                    },
                },
                root: message,
                adapters: [adapter],
            });
            manager.reconcile();
            expect(frames.map(frame => frame.host.dataset.ttRuntimeSlotId.startsWith(`${prefix}:1:`))).toEqual([true, true]);
            const [sibling, target] = frames;
            const siblingWindow = sibling.iframe.contentWindow;
            const siblingDocument = sibling.iframe.contentDocument;
            const siblingOrigin = siblingWindow.performance.timeOrigin;
            siblingDocument.querySelector('input').value = 'keep me';

            for (let cycle = 1; cycle <= 2; cycle++) {
                const removed = new Promise(resolve => {
                    const observer = new MutationObserver(() => {
                        if (!target.iframe.isConnected) {
                            observer.disconnect();
                            resolve();
                        }
                    });
                    observer.observe(target.host, { childList: true });
                });
                manager.setVisible(target.host.dataset.ttRuntimeSlotId, false);
                manager.reconcile();
                await removed;
                URL.revokeObjectURL(target.url);
                const loaded = nextLoad(target.iframe);
                manager.setVisible(target.host.dataset.ttRuntimeSlotId, true);
                manager.reconcile();
                await loaded;

                expect(target.host.querySelector('iframe')).toBe(target.iframe);
                expect(target.loads).toBe(cycle + 1);
                expect(target.iframe.contentDocument.querySelector('input').value).toBe('initial');
                expect(target.source.isConnected).toBe(true);
                expect(target.source.dataset).toMatchObject({ xbFinal: 'true', xbHash: 'hash-1' });
                expect(sibling.iframe.contentWindow).toBe(siblingWindow);
                expect(sibling.iframe.contentDocument).toBe(siblingDocument);
                expect(siblingWindow.performance.timeOrigin).toBe(siblingOrigin);
                expect(sibling.loads).toBe(1);
                expect(siblingDocument.querySelector('input').value).toBe('keep me');
                expect(registrations).toBe(2);
            }
            expect(emit.mock.calls.some(([event]) => event === event_types.MESSAGE_UPDATED)).toBe(false);
        } finally {
            adapterHandle?.dispose();
            for (const { host, url } of frames) {
                if (host.dataset.ttRuntimeSlotId) manager.unregister(host.dataset.ttRuntimeSlotId);
                URL.revokeObjectURL(url);
            }
        }
    });
}
