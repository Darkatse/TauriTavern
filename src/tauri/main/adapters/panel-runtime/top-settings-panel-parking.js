// @ts-check

import { PanelRuntimeKind } from '../../services/panel-runtime/panel-runtime-kinds.js';

/**
 * @typedef {import('../../services/embedded-runtime/embedded-runtime-manager.js').createEmbeddedRuntimeManager} createEmbeddedRuntimeManager
 * @typedef {ReturnType<createEmbeddedRuntimeManager>} EmbeddedRuntimeManager
 */

/**
 * @param {HTMLElement} drawerContent
 */
function isDrawerOpen(drawerContent) {
    return drawerContent.classList.contains('openDrawer') && !drawerContent.classList.contains('closedDrawer');
}

const LEFT_NAV_MAIN_API_BLOCKS = Object.freeze({
    koboldhorde: {
        settingsId: 'kobold_api-settings',
        presetsId: 'kobold_api-presets',
        rangesId: null,
        streamingId: null,
    },
    kobold: {
        settingsId: 'kobold_api-settings',
        presetsId: 'kobold_api-presets',
        rangesId: null,
        streamingId: 'streaming_kobold_block',
    },
    textgenerationwebui: {
        settingsId: 'textgenerationwebui_api-settings',
        presetsId: 'textgenerationwebui_api-presets',
        rangesId: null,
        streamingId: 'streaming_textgenerationwebui_block',
    },
    novel: {
        settingsId: 'novel_api-settings',
        presetsId: 'novel_api-presets',
        rangesId: 'range_block_novel',
        streamingId: 'streaming_novel_block',
    },
    openai: {
        settingsId: 'openai_settings',
        presetsId: 'openai_api-presets',
        rangesId: 'range_block_openai',
        streamingId: null,
    },
});

/**
 * @param {string} id
 * @param {string} display
 */
function setDisplay(id, display) {
    const el = document.getElementById(id);
    if (!(el instanceof HTMLElement)) {
        throw new Error(`PanelParking(left-nav): #${id} not found`);
    }
    el.style.display = display;
}

function syncLeftNavMainApiUi() {
    const mainApi = document.getElementById('main_api');
    if (!(mainApi instanceof HTMLSelectElement)) {
        throw new Error('PanelParking(left-nav): #main_api <select> not found');
    }

    const selected = String(mainApi.value || '').trim();
    const active = /** @type {keyof typeof LEFT_NAV_MAIN_API_BLOCKS} */ (selected);
    if (!Object.prototype.hasOwnProperty.call(LEFT_NAV_MAIN_API_BLOCKS, active)) {
        return;
    }

        for (const [apiName, blocks] of Object.entries(LEFT_NAV_MAIN_API_BLOCKS)) {
            if (apiName === active) {
                continue;
            }

            setDisplay(blocks.settingsId, 'none');
            setDisplay(blocks.presetsId, 'none');
            if (blocks.rangesId) {
                setDisplay(blocks.rangesId, 'none');
            }
            if (blocks.streamingId) {
                setDisplay(blocks.streamingId, 'none');
            }
        }

        const activeBlocks = LEFT_NAV_MAIN_API_BLOCKS[active];
        if (activeBlocks.streamingId) {
            setDisplay(activeBlocks.streamingId, 'block');
        }
        setDisplay(activeBlocks.settingsId, 'block');
        if (activeBlocks.rangesId) {
            setDisplay(activeBlocks.rangesId, 'block');
        }
        setDisplay(activeBlocks.presetsId, active === 'openai' ? 'flex' : 'block');

    setDisplay('ai_module_block_novel', active === 'novel' ? 'block' : 'none');

    const textgenType = document.getElementById('textgen_type');
    const isOpenRouter = textgenType instanceof HTMLSelectElement && String(textgenType.value || '').trim() === 'openrouter';
    const showPromptCost = active === 'textgenerationwebui' && isOpenRouter;
    setDisplay('prompt_cost_block', showPromptCost ? 'block' : 'none');

    setDisplay('common-gen-settings-block', active === 'openai' ? 'none' : 'block');
}

/**
 * @param {EmbeddedRuntimeManager} manager
 * @param {{ panelId: string; parkedSelector: string; pinnedSelectors?: string[]; afterHydrate?: (reason: string) => void }} config
 */
function registerDrawerParking(manager, { panelId, parkedSelector, pinnedSelectors = [], afterHydrate }) {
    const host = document.getElementById(panelId);
    if (!(host instanceof HTMLElement)) {
        throw new Error(`PanelParking: #${panelId} not found`);
    }

    const drawer = host.closest('.drawer');
    if (!(drawer instanceof HTMLElement)) {
        throw new Error(`PanelParking(${panelId}): .drawer ancestor not found`);
    }

    const toggle = drawer.querySelector('.drawer-toggle');
    if (!(toggle instanceof HTMLElement)) {
        throw new Error(`PanelParking(${panelId}): .drawer-toggle not found`);
    }

    const parkedRoot = host.querySelector(parkedSelector);
    if (!(parkedRoot instanceof HTMLElement)) {
        throw new Error(`PanelParking(${panelId}): parked root not found: ${parkedSelector}`);
    }

    /** @type {{ park: () => void; restore: () => void } | null} */
    let pinned = null;
    if (Array.isArray(pinnedSelectors) && pinnedSelectors.length > 0) {
        const anchorZone = document.createElement('div');
        anchorZone.dataset.ttPanelAnchorZone = panelId;
        anchorZone.style.display = 'none';
        host.appendChild(anchorZone);

        /** @type {Array<{ selector: string; start: Comment; end: Comment; slot: HTMLElement }>} */
        const ranges = [];
        for (const rawSelector of pinnedSelectors) {
            const selector = String(rawSelector || '').trim();
            if (!selector) {
                continue;
            }

            const anchor = parkedRoot.querySelector(selector);
            if (!(anchor instanceof Element)) {
                throw new Error(`PanelParking(${panelId}): pinned selector not found in parked root: ${selector}`);
            }

            const parent = anchor.parentNode;
            if (!parent) {
                throw new Error(`PanelParking(${panelId}): pinned selector has no parent: ${selector}`);
            }

            const start = document.createComment(`tt:pinned-start:${panelId}:${selector}`);
            const end = document.createComment(`tt:pinned-end:${panelId}:${selector}`);
            parent.insertBefore(start, anchor);
            parent.insertBefore(end, anchor.nextSibling);

            const slot = document.createElement('div');
            slot.dataset.ttPinnedSelector = selector;
            anchorZone.appendChild(slot);

            ranges.push({ selector, start, end, slot });
        }

        pinned = {
            park: () => {
                for (const r of ranges) {
                    const parent = r.start.parentNode;
                    if (!parent) {
                        throw new Error(`PanelParking(${panelId}): pinned start lost: ${r.selector}`);
                    }
                    if (r.end.parentNode !== parent) {
                        throw new Error(`PanelParking(${panelId}): pinned range split: ${r.selector}`);
                    }

                    while (r.start.nextSibling && r.start.nextSibling !== r.end) {
                        r.slot.appendChild(r.start.nextSibling);
                    }
                }
            },
            restore: () => {
                for (const r of ranges) {
                    const parent = r.start.parentNode;
                    if (!parent) {
                        throw new Error(`PanelParking(${panelId}): pinned start lost: ${r.selector}`);
                    }
                    if (r.end.parentNode !== parent) {
                        throw new Error(`PanelParking(${panelId}): pinned range split: ${r.selector}`);
                    }

                    while (r.slot.firstChild) {
                        parent.insertBefore(r.slot.firstChild, r.end);
                    }
                }
            },
        };
    }

    const anchor = document.createComment(`tt:panel-park:${panelId}`);
    const parent = parkedRoot.parentNode;
    if (!parent) {
        throw new Error(`PanelParking(${panelId}): parked root has no parent`);
    }
    parent.insertBefore(anchor, parkedRoot.nextSibling);

    const slotId = `panel:${panelId}`;

    /** @param {string} reason */
    const hydrate = (reason) => {
        if (parkedRoot.isConnected) {
            if (pinned) {
                pinned.restore();
            }
            if (afterHydrate) {
                afterHydrate(reason);
            }
            return;
        }
        if (!anchor.parentNode) {
            throw new Error(`PanelParking(${panelId}).hydrate(${reason}): anchor lost`);
        }
        anchor.parentNode.insertBefore(parkedRoot, anchor);
        if (pinned) {
            pinned.restore();
        }
        if (afterHydrate) {
            afterHydrate(reason);
        }
    };

    /** @param {string} reason */
    const dehydrate = (reason) => {
        if (pinned) {
            pinned.park();
        }
        if (!parkedRoot.isConnected) {
            return;
        }
        parkedRoot.remove();
    };

    manager.register({
        id: slotId,
        kind: PanelRuntimeKind.DrawerContent,
        element: host,
        visibilityMode: 'manual',
        initialVisible: isDrawerOpen(host),
        hydrate,
        dehydrate,
    });

    const onToggleClickCapture = () => {
        manager.setVisible(slotId, true);
        manager.reconcile();
    };
    toggle.addEventListener('click', onToggleClickCapture, true);

    const classObserver = new MutationObserver(() => {
        const open = isDrawerOpen(host);
        manager.setVisible(slotId, open);
        if (open) {
            manager.reconcile();
        }
    });

    classObserver.observe(host, { attributes: true, attributeFilter: ['class'] });

    return {
        drawerId: typeof drawer.id === 'string' ? drawer.id : '',
        slotId,
        dispose: () => {
            classObserver.disconnect();
            toggle.removeEventListener('click', onToggleClickCapture, true);
        },
    };
}

/**
 * Phase 0: Park the heaviest closed drawers under #top-settings-holder by
 * detaching their heavy subtrees from the main document tree.
 *
 * Controlled by `tauritavern-settings.panel_runtime_profile` (default: off).
 *
 * @param {{ manager: EmbeddedRuntimeManager }} options
 */
export function installTopSettingsPanelParking({ manager }) {
    const leftNavPinnedSelectors = manager.profile === 'compat'
        ? ['#openai_preset_import_file', '#completion_prompt_manager']
        : [];

    const registrations = [
        registerDrawerParking(manager, {
            panelId: 'left-nav-panel',
            parkedSelector: '.scrollableInner',
            pinnedSelectors: leftNavPinnedSelectors,
            afterHydrate: () => syncLeftNavMainApiUi(),
        }),
        registerDrawerParking(manager, { panelId: 'AdvancedFormatting', parkedSelector: '.flex-container.spaceEvenly' }),
        registerDrawerParking(manager, { panelId: 'WorldInfo', parkedSelector: '#wi-holder' }),
        registerDrawerParking(manager, { panelId: 'user-settings-block', parkedSelector: '#user-settings-block-content' }),
        registerDrawerParking(manager, { panelId: 'Backgrounds', parkedSelector: '#bg_tabs' }),
        registerDrawerParking(manager, { panelId: 'PersonaManagement', parkedSelector: '#persona-management-block' }),
    ];

    /** @type {Array<[string, string]>} */
    const entries = [];
    for (const r of registrations) {
        if (r.drawerId) {
            entries.push([r.drawerId, r.slotId]);
        }
    }
    const slotByDrawerId = new Map(entries);

    document.addEventListener('click', (event) => {
        const target = event.target;
        if (!(target instanceof Element)) {
            return;
        }

        const opener = target.closest('.drawer-opener');
        if (!(opener instanceof HTMLElement)) {
            return;
        }

        const drawerId = String(opener.getAttribute('data-target') || '').trim();
        const slotId = drawerId ? slotByDrawerId.get(drawerId) : null;
        if (!slotId) {
            return;
        }

        manager.setVisible(slotId, true);
        manager.reconcile();
    }, true);

    manager.reconcile();
}
