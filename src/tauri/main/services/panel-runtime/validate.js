// @ts-check

/**
 * @param {string} id
 * @returns {HTMLElement}
 */
function mustGetConnectedElementById(id) {
    const el = document.getElementById(id);
    if (!(el instanceof HTMLElement)) {
        throw new Error(`PanelRuntime validate: #${id} not found`);
    }
    if (!el.isConnected) {
        throw new Error(`PanelRuntime validate: #${id} is not connected`);
    }
    return el;
}

/**
 * @param {{ profileName: string }} options
 */
export function validatePanelRuntimeInvariants({ profileName }) {
    const profile = String(profileName || '').trim();
    if (!profile) {
        throw new Error('PanelRuntime validate: profileName is required');
    }
    if (profile !== 'compat' && profile !== 'aggressive') {
        throw new Error(`PanelRuntime validate: unknown profile '${profile}'`);
    }

    // Extensions mount points.
    mustGetConnectedElementById('rm_extensions_block');
    mustGetConnectedElementById('extensions_settings');
    mustGetConnectedElementById('regex_container');
    mustGetConnectedElementById('qr_container');

    // Compat anchor-zone surface: keep the OpenAI preset / prompt / control hosts selectable while parked.
    if (profile === 'compat') {
        mustGetConnectedElementById('openai_api-presets');
        mustGetConnectedElementById('completion_prompt_manager');
        mustGetConnectedElementById('openai_api');
    }

    // API connections control surface (only meaningful when left-nav is hydrated).
    const leftNavScrollable = document.querySelector('#left-nav-panel .scrollableInner');
    const isLeftNavHydrated = leftNavScrollable instanceof HTMLElement && leftNavScrollable.isConnected;
    if (!isLeftNavHydrated) {
        return;
    }

    const mainApiEl = mustGetConnectedElementById('main_api');
    if (!(mainApiEl instanceof HTMLSelectElement)) {
        throw new Error('PanelRuntime validate: #main_api is not a <select>');
    }
    mustGetConnectedElementById('kobold_horde');
    mustGetConnectedElementById('kobold_api');
    mustGetConnectedElementById('novel_api');
    mustGetConnectedElementById('textgenerationwebui_api');
    mustGetConnectedElementById('openai_api');

    if (String(mainApiEl.value || '').trim() === 'openai') {
        mustGetConnectedElementById('chat_completion_source');
    }
    if (String(mainApiEl.value || '').trim() === 'textgenerationwebui') {
        mustGetConnectedElementById('textgen_type');
    }
}
