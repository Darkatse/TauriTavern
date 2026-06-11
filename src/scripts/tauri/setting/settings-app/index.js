import { createApp } from 'vue/dist/vue.esm-bundler.js';

import { createTauriTavernSettingsApp } from './SettingsApp.js';

export function mountTauriTavernSettingsApp(mount, options) {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern settings mount element is required');
    }

    const app = createApp(createTauriTavernSettingsApp(options));
    const vm = app.mount(mount);

    return {
        getDraft: () => vm.getDraft(),
        unmount: () => app.unmount(),
    };
}
