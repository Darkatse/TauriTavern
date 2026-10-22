// @ts-check

import { createSettings } from './settings.js';
import { createReadCommands } from './commands/read.js';
import { createChatCommands } from './commands/chat.js';
import { createResources } from './protocols/resources.js';
import { createSettingsCommits } from './protocols/settings-commit.js';
import { createBackend } from './backend.js';
import { createChatCommits } from './protocols/chat-commit.js';
import { createWriteCommands } from './commands/write.js';
import { createGenerationCommands } from './protocols/generation.js';
import { createExtensionCommands } from './commands/extensions.js';
import { TAURI_GRACEFUL_EXIT_EVENT } from '../main/services/lifecycle/lifecycle-flush-service.js';

/**
 * @typedef {{ kind: 'form', entries: [string, string][] }} FormEntries
 * @typedef {{ method: string, url: string, headers: Record<string, string>, body: string | FormEntries | null }} BackendRequest
 * @typedef {{ status: number, headers: Record<string, string>, body: string | ReadableStream<Uint8Array> | null }} BackendResponse
 * @typedef {{
 *   identity: import('../../scripts/util/host-identity.js').HostIdentity,
 *   backend: { handle: (request: BackendRequest) => BackendResponse },
 *   store: { load: () => Record<string, any> | null, save: (value: Record<string, any>) => void },
 * }} WebHostOptions
 * @typedef {Record<string, (args: any, options: any) => any>} CommandHandlers
 */

/** Install before init.js. Backend and store belong to the embedding Session. @param {WebHostOptions} options */
export function installWebHost({ identity, backend, store }) {
    Object.defineProperty(window, '__TAURITAVERN_HOST__', { value: Object.freeze({ ...identity }) });
    const fetchResource = window.fetch.bind(window);
    /** @type {Promise<any> | undefined} */
    let defaults;
    function loadDefaults() {
        return defaults ??= fetchResource(new URL('./default-state.json', import.meta.url)).then(async response => {
            if (!response.ok) throw new Error(`Cannot load Web Host defaults: ${response.status}`);
            return response.json();
        });
    }
    const settings = createSettings(store, async () => (await loadDefaults()).settings);
    const st = createBackend(backend);
    const resources = createResources();
    /** @type {CommandHandlers} */
    const commands = {
        ...createReadCommands(st),
        ...createChatCommands(st, resources),
        ...resources.commands,
        ...createSettingsCommits(st),
        ...createChatCommits(st),
        ...createWriteCommands(st),
        ...createGenerationCommands(st),
        ...createExtensionCommands(st, fetchResource, loadDefaults),
        wait_for_backend_ready: () => undefined,
        backend_error_bridge_ready: () => [],
        get_tauritavern_settings: settings.get,
        update_tauritavern_settings: ({ dto }) => settings.update(dto),
        get_window_snapshot: () => null,
        // Browser logs already stay in the frontend capture; there is no native log file.
        devlog_append_frontend_logs: () => undefined,
        database_handle: ({ request }) => {
            if (request.type !== 'flushAll') return unsupported(`database_handle:${request.type}`);
            return null;
        },
    };
    /** @param {string} name @returns {never} */
    const unsupported = (name) => { throw { Unsupported: `Web Host does not support ${name}` }; };
    let listenerId = 0;
    /** @type {Map<number, { name: string, callback: (event: any) => unknown }>} */
    const listeners = new Map();
    /** @type {Promise<void> | undefined} */
    let exited;
    /** @type {() => void} */
    let markExited = () => {};
    window.__TAURI__ = {
        core: {
            /** @param {string} command @param {any} args @param {any} options */
            async invoke(command, args = {}, options = {}) {
                const handler = Object.hasOwn(commands, command) ? commands[command] : undefined;
                if (handler === undefined) return unsupported(command);
                return handler(args, options);
            },
            Channel: class {
                /** @param {(payload: any) => void} onmessage */
                constructor(onmessage) { this.onmessage = onmessage; }
            },
            convertFileSrc: () => unsupported('core.convertFileSrc'),
        },
        event: {
            /** @param {string} name @param {(event: any) => unknown} callback */
            async listen(name, callback) {
                const id = ++listenerId;
                listeners.set(id, { name, callback });
                return () => { listeners.delete(id); };
            },
        },
        window: {
            getCurrentWindow: () => ({
                isFullscreen: async () => false,
                setFullscreen: async () => unsupported('window.setFullscreen'),
                // TT closes the window once its graceful-exit flush succeeds.
                destroy: async () => markExited(),
            }),
        },
    };
    /** @param {string} name @param {any} payload */
    function emit(name, payload) {
        for (const [id, listener] of listeners) {
            if (listener.name === name) listener.callback({ event: name, id, payload });
        }
    }
    return {
        emit,
        /** Close through TT's native exit path. A failed flush keeps the page open and this pending; call again to retry. */
        exit() {
            exited ??= new Promise(resolve => { markExited = resolve; });
            emit(TAURI_GRACEFUL_EXIT_EVENT, null);
            return exited;
        },
    };
}
