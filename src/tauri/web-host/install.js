// @ts-check

import { createSettings } from './settings.js';
import { createReadCommands } from './commands/read.js';
import { createChatCommands } from './commands/chat.js';
import { createResources } from './protocols/resources.js';
import { createSettingsCommits } from './protocols/settings-commit.js';
import { createBackend } from './backend.js';

/**
 * @typedef {{ method: string, url: string, headers: Record<string, string>, body: string | null }} BackendRequest
 * @typedef {{ status: number, headers: Record<string, string>, body: string }} BackendResponse
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
    const settings = createSettings(store, fetchResource);
    const st = createBackend(backend);
    const resources = createResources();
    /** @type {CommandHandlers} */
    const commands = {
        ...createReadCommands(st),
        ...createChatCommands(st, resources),
        ...resources.commands,
        ...createSettingsCommits(st),
        wait_for_backend_ready: () => undefined,
        backend_error_bridge_ready: () => [],
        get_tauritavern_settings: settings.get,
        get_window_snapshot: () => null,
        // Browser logs already stay in the frontend capture; there is no native log file.
        devlog_append_frontend_logs: () => undefined,
        read_frontend_template: async ({ name }) => {
            const response = await fetchResource(`/scripts/templates/${name}`);
            if (!response.ok) throw new Error(`Cannot read frontend template ${name}: ${response.status}`);
            return response.text();
        },
    };
    /** @param {string} name @returns {never} */
    const unsupported = (name) => { throw { Unsupported: `Web Host does not support ${name}` }; };
    let listenerId = 0;
    /** @type {Map<number, { name: string, callback: (event: any) => unknown }>} */
    const listeners = new Map();
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
            }),
        },
    };
    return {
        /** @param {string} name @param {any} payload */
        emit(name, payload) {
            for (const [id, listener] of listeners) {
                if (listener.name === name) listener.callback({ event: name, id, payload });
            }
        },
    };
}
