// @ts-check

/**
 * @param {string} selector
 * @returns {HTMLElement}
 */
function mustGetConnectedElement(selector) {
    const el = document.querySelector(selector);
    if (!(el instanceof HTMLElement)) {
        throw new Error(`PanelRuntime validate: ${selector} is missing or disconnected`);
    }
    return el;
}

/**
 * @param {{ profileName: string; pinnedSelectors: readonly string[] }} options
 */
export function validatePanelRuntimeInvariants({ profileName, pinnedSelectors }) {
    const profile = String(profileName || '').trim();
    if (!profile) {
        throw new Error('PanelRuntime validate: profileName is required');
    }
    if (profile !== 'compat' && profile !== 'aggressive') {
        throw new Error(`PanelRuntime validate: unknown profile '${profile}'`);
    }

    // Extensions mount points.
    mustGetConnectedElement('#rm_extensions_block');
    mustGetConnectedElement('#extensions_settings');
    mustGetConnectedElement('#regex_container');
    mustGetConnectedElement('#qr_container');

    for (const selector of pinnedSelectors) {
        mustGetConnectedElement(selector);
    }

    // #rm_api_block is subtree-gated, not drawer-parked: these hold in any park state.
    const mainApiEl = mustGetConnectedElement('#main_api');
    if (!(mainApiEl instanceof HTMLSelectElement)) {
        throw new Error('PanelRuntime validate: #main_api is not a <select>');
    }
    mustGetConnectedElement('#kobold_horde');
    mustGetConnectedElement('#kobold_api');
    mustGetConnectedElement('#novel_api');
    mustGetConnectedElement('#textgenerationwebui_api');
    mustGetConnectedElement('#openai_api');

    if (String(mainApiEl.value || '').trim() === 'openai') {
        mustGetConnectedElement('#chat_completion_source');
    }
    if (String(mainApiEl.value || '').trim() === 'textgenerationwebui') {
        mustGetConnectedElement('#textgen_type');
    }
}
