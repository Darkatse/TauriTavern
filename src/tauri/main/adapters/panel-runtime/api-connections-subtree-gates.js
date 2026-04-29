// @ts-check

import { PanelRuntimeKind } from '../../services/panel-runtime/panel-runtime-kinds.js';

/**
 * @typedef {import('../../services/embedded-runtime/embedded-runtime-manager.js').createEmbeddedRuntimeManager} createEmbeddedRuntimeManager
 * @typedef {ReturnType<createEmbeddedRuntimeManager>} EmbeddedRuntimeManager
 */

const MAIN_API_TO_CONNECTOR_ID = Object.freeze({
    koboldhorde: 'kobold_horde',
    kobold: 'kobold_api',
    novel: 'novel_api',
    textgenerationwebui: 'textgenerationwebui_api',
    openai: 'openai_api',
});

/** @type {boolean} */
let preinstalled = false;

/** @type {EmbeddedRuntimeManager | null} */
let manager = null;

/** @type {Map<string, string> | null} */
let providerSlotIdByMainApi = null;

/** @type {ReturnType<typeof createAttributeChildGate> | null} */
let openaiSourceGate = null;

/** @type {ReturnType<typeof createAttributeChildGate> | null} */
let textgenTypeGate = null;

function requireJQuery() {
    const jq = /** @type {any} */ (globalThis).jQuery || /** @type {any} */ (globalThis).$;
    if (typeof jq !== 'function') {
        throw new Error('PanelRuntime: jQuery not found');
    }
    return jq;
}

/**
 * @param {string} id
 * @returns {HTMLElement}
 */
function mustGetElementById(id) {
    const el = document.getElementById(id);
    if (!(el instanceof HTMLElement)) {
        throw new Error(`PanelRuntime: #${id} not found`);
    }
    return el;
}

/**
 * @param {HTMLElement} host
 * @param {DocumentFragment} frag
 */
function parkChildrenInto(host, frag) {
    while (host.firstChild) {
        frag.appendChild(host.firstChild);
    }
}

/**
 * @param {HTMLElement} host
 * @param {DocumentFragment} frag
 */
function restoreChildren(host, frag) {
    if (!frag.firstChild) {
        return;
    }
    host.appendChild(frag);
}

/**
 * Registers a slot that parks/restores an element's children while keeping the
 * element itself connected for selector compatibility.
 *
 * @param {EmbeddedRuntimeManager} manager
 * @param {{ slotId: string; element: HTMLElement; initialVisible: boolean }} config
 */
function registerChildParkingSlot(manager, { slotId, element, initialVisible }) {
    /** @type {DocumentFragment} */
    let parked = document.createDocumentFragment();

    manager.register({
        id: slotId,
        kind: PanelRuntimeKind.SubtreeGate,
        element,
        visibilityMode: 'manual',
        initialVisible,
        hydrate: () => {
            restoreChildren(element, parked);
        },
        dehydrate: () => {
            parkChildrenInto(element, parked);
        },
    });
}

/**
 * @param {HTMLElement} root
 * @param {string} attr
 * @returns {HTMLElement[]}
 */
function collectAttributeHosts(root, attr) {
    return Array.from(root.querySelectorAll(`[${attr}]`)).filter((el) => el instanceof HTMLElement);
}

/**
 * @param {HTMLElement} root
 * @param {{
 *   attr: string;
 *   modeAttr: string;
 *   includeAll?: boolean;
 * }} config
 */
function createAttributeChildGate(root, { attr, modeAttr, includeAll = false }) {
    /** @type {Array<{ host: HTMLElement; parked: DocumentFragment; values: string[]; mode: 'except' | 'include' }>} */
    const gates = [];
    for (const host of collectAttributeHosts(root, attr)) {
        const rawList = String(host.getAttribute(attr) || '').trim();
        if (!rawList) {
            continue;
        }

        const values = rawList.split(',').map((s) => s.trim()).filter(Boolean);
        const rawMode = String(host.getAttribute(modeAttr) || '').trim().toLowerCase();
        const mode = rawMode === 'except' ? 'except' : 'include';

        gates.push({
            host,
            parked: document.createDocumentFragment(),
            values,
            mode,
        });
    }

    return {
        /**
         * @param {string} value
         */
        apply: (value) => {
            const v = String(value || '').trim();
            for (const g of gates) {
                const matchesValue = g.values.includes(v);
                const shouldBeVisible = g.mode === 'except'
                    ? !matchesValue
                    : matchesValue || (includeAll && g.values.includes('all'));

                if (shouldBeVisible) {
                    restoreChildren(g.host, g.parked);
                } else {
                    parkChildrenInto(g.host, g.parked);
                }
            }
        },
    };
}

/**
 * @param {string} id
 */
function currentSelectValue(id) {
    const el = document.getElementById(id);
    if (!(el instanceof HTMLSelectElement)) {
        return '';
    }
    return String(el.value || '').trim();
}

/**
 * @param {string} source
 */
function normalizeChatCompletionSourceForGate(source) {
    const value = String(source || '').trim();
    if (value.startsWith('custom_')) {
        return 'custom';
    }
    return value;
}

/**
 * Hydrates the selected provider subtree and parks all others, then applies any
 * nested gates (OpenAI source / Textgen type) for the active provider.
 *
 * Must run synchronously inside the `change` event so downstream handlers can
 * safely query their expected DOM.
 *
 * @param {string} mainApi
 */
function handleMainApiChange(mainApi) {
    if (!manager || !providerSlotIdByMainApi) {
        return;
    }

    const next = String(mainApi || '').trim();
    for (const [api, slotId] of providerSlotIdByMainApi.entries()) {
        manager.setVisible(slotId, api === next);
    }
    manager.reconcile();

    if (next === 'openai' && openaiSourceGate) {
        const selection = currentSelectValue('chat_completion_source') || 'openai';
        openaiSourceGate.apply(normalizeChatCompletionSourceForGate(selection) || 'openai');
    }
    if (next === 'textgenerationwebui' && textgenTypeGate) {
        textgenTypeGate.apply(currentSelectValue('textgen_type') || '');
    }
}

/**
 * @param {string} source
 */
function handleChatCompletionSourceChange(source) {
    if (!openaiSourceGate) {
        return;
    }

    const mainApi = currentSelectValue('main_api');
    if (mainApi !== 'openai') {
        return;
    }

    openaiSourceGate.apply(normalizeChatCompletionSourceForGate(source) || 'openai');
}

/**
 * @param {string} type
 */
function handleTextgenTypeChange(type) {
    if (!textgenTypeGate) {
        return;
    }

    const mainApi = currentSelectValue('main_api');
    if (mainApi !== 'textgenerationwebui') {
        return;
    }

    textgenTypeGate.apply(type || '');
}

/**
 * Registers early `change` listeners so our gate handlers run before
 * SillyTavern's own handlers (including jQuery `.trigger('change')`).
 */
export function preinstallApiConnectionsSubtreeGates() {
    if (preinstalled) {
        return;
    }
    preinstalled = true;

    const $ = requireJQuery();

    const mainApi = mustGetElementById('main_api');
    if (!(mainApi instanceof HTMLSelectElement)) {
        throw new Error('PanelRuntime: #main_api is not a <select>');
    }
    $(mainApi).on('change.ttPanelRuntimeGate', () => {
        handleMainApiChange(String(mainApi.value || ''));
    });

    const chatCompletionSource = mustGetElementById('chat_completion_source');
    if (!(chatCompletionSource instanceof HTMLSelectElement)) {
        throw new Error('PanelRuntime: #chat_completion_source is not a <select>');
    }
    $(chatCompletionSource).on('change.ttPanelRuntimeGate', () => {
        handleChatCompletionSourceChange(String(chatCompletionSource.value || ''));
    });

    const textgenType = mustGetElementById('textgen_type');
    if (!(textgenType instanceof HTMLSelectElement)) {
        throw new Error('PanelRuntime: #textgen_type is not a <select>');
    }
    $(textgenType).on('change.ttPanelRuntimeGate', () => {
        handleTextgenTypeChange(String(textgenType.value || ''));
    });
}

/**
 * Builds gates and immediately parks inactive subtrees.
 *
 * @param {{ manager: EmbeddedRuntimeManager }} options
 */
export function activateApiConnectionsSubtreeGates({ manager: nextManager }) {
    if (!nextManager) {
        throw new Error('activateApiConnectionsSubtreeGates requires manager');
    }
    if (manager) {
        throw new Error('activateApiConnectionsSubtreeGates called twice');
    }

    manager = nextManager;

    const selectedMainApi = currentSelectValue('main_api');
    providerSlotIdByMainApi = new Map();
    for (const [mainApi, connectorId] of Object.entries(MAIN_API_TO_CONNECTOR_ID)) {
        const element = mustGetElementById(connectorId);
        const slotId = `tt:gate:rm_api_block:main_api:${mainApi}`;
        providerSlotIdByMainApi.set(mainApi, slotId);
        registerChildParkingSlot(manager, { slotId, element, initialVisible: mainApi === selectedMainApi });
    }

    const openaiApi = mustGetElementById('openai_api');
    openaiSourceGate = createAttributeChildGate(openaiApi, {
        attr: 'data-source',
        modeAttr: 'data-source-mode',
    });

    const textgenApi = mustGetElementById('textgenerationwebui_api');
    textgenTypeGate = createAttributeChildGate(textgenApi, {
        attr: 'data-tg-type',
        modeAttr: 'data-tg-type-mode',
        includeAll: true,
    });

    manager.reconcile();

    if (selectedMainApi === 'openai') {
        openaiSourceGate.apply(currentSelectValue('chat_completion_source') || 'openai');
    }
    if (selectedMainApi === 'textgenerationwebui') {
        textgenTypeGate.apply(currentSelectValue('textgen_type') || '');
    }
}
