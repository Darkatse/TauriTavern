import { createApp } from 'vue/dist/vue.esm-bundler.js';

import { createTauriTavernSettingsApp } from './SettingsApp.js';

export function mountTauriTavernSettingsApp(mount, options) {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern settings mount element is required');
    }

    const app = createApp(createTauriTavernSettingsApp(options));
    const vm = app.mount(mount);
    let mounted = true;

    return {
        getDraft: () => vm.getDraft(),
        setChatBackupStorageStats: stats => {
            if (mounted) {
                vm.chatBackupStorageStats = stats;
            }
        },
        unmount: () => {
            mounted = false;
            app.unmount();
        },
    };
}
