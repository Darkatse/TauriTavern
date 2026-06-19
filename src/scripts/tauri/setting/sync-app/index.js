import { createApp } from 'vue/dist/vue.esm-bundler.js';
import { createTauriTavernSyncApp } from './SyncApp.js';
import { createTauriTavernSyncProgressApp } from './SyncProgressApp.js';
import { createTauriTavernSyncScopeApp } from './SyncScopeApp.js';

export function mountTauriTavernSyncApp(mount, options) {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern Sync mount element is required');
    }

    const app = createApp(createTauriTavernSyncApp(options));
    const instance = app.mount(mount);

    return {
        refresh: () => instance.refresh(),
        refreshAutomationStatus: () => instance.refreshAutomationStatus(),
        unmount: () => app.unmount(),
    };
}

export function mountTauriTavernSyncProgressApp(mount, options) {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern Sync progress mount element is required');
    }

    const app = createApp(createTauriTavernSyncProgressApp(options));
    const instance = app.mount(mount);

    return {
        update: (next) => instance.update(next),
        unmount: () => app.unmount(),
    };
}

export function mountTauriTavernSyncScopeApp(mount, options) {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern Sync scope mount element is required');
    }

    const app = createApp(createTauriTavernSyncScopeApp(options));
    const instance = app.mount(mount);

    return {
        getSelection: () => instance.getSelection(),
        unmount: () => app.unmount(),
    };
}
