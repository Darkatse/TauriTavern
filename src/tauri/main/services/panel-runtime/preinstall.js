// @ts-check

import { preinstallApiConnectionsSubtreeGates } from '../../adapters/panel-runtime/api-connections-subtree-gates.js';

// Keep in sync with:
// - src/tauri/main/services/panel-runtime/install.js
// - src/scripts/tauri/sync/sync-panel.js
//
// Rationale: `panel_runtime_profile` is persisted in Tauri settings (async to load),
// but some panel-runtime hooks must be installed during bootstrap to preserve
// handler ordering for jQuery `.trigger('change')`. We mirror the last known
// profile into localStorage so bootstrap can synchronously honor `off`.
const PANEL_RUNTIME_PROFILE_STORAGE_KEY = 'tt:panelRuntimeProfile';

export function preinstallPanelRuntime() {
    const profile = String(localStorage.getItem(PANEL_RUNTIME_PROFILE_STORAGE_KEY) || '').trim();
    if (profile === 'off') {
        return;
    }

    preinstallApiConnectionsSubtreeGates();
}
