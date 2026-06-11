import { createApp } from 'vue/dist/vue.esm-bundler.js';

import { createTauriTavernDevLogsApp } from './DevLogsApp.js';

export function mountTauriTavernDevLogsApp(mount, options) {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern dev logs mount element is required');
    }

    const app = createApp(createTauriTavernDevLogsApp(options));
    app.mount(mount);

    return {
        unmount: () => app.unmount(),
    };
}
