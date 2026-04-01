import { callGenericPopup, POPUP_RESULT, POPUP_TYPE } from '../../../popup.js';
import { isMobile } from '../../../RossAscends-mods.js';
import { t, translate } from '../../../i18n.js';
import { getTauriTavernSettings, updateTauriTavernSettings } from '../../../../tauri-bridge.js';
import {
    clearLegacyEmbeddedRuntimeProfileName,
    normalizeEmbeddedRuntimeProfileName,
    resolveEffectiveEmbeddedRuntimeProfileName,
    setEmbeddedRuntimeBootstrapProfileName,
} from '../../../../tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js';
import { DYNAMIC_THEME_CHANGED_EVENT } from '../../../../tauri/main/services/dynamic-theme/constants.js';
import {
    CHAT_HISTORY_MODE_WINDOWED,
    normalizeChatHistoryModeName,
    setChatHistoryBootstrapModeName,
} from '../../../../tauri/main/services/chat-history/chat-history-mode-state.js';
import { runOrPopup } from './popup-utils.js';

function isWindowsPlatform() {
    return typeof navigator !== 'undefined'
        && /windows/i.test(String(navigator.userAgent || ''));
}

export async function openTauriTavernSettingsPopup() {
    const settings = await getTauriTavernSettings();
    const supportsCloseToTrayOnClose = isWindowsPlatform() && !isMobile();

    const closeToTrayRow = supportsCloseToTrayOnClose
        ? `
            <div class="flex-container alignItemsCenter" style="justify-content: space-between; gap: 12px; flex-wrap: wrap;">
                <div class="flex-container alignItemsBaseline" style="gap: 8px; min-width: 220px; flex: 1;">
                    <span data-i18n="Minimize to tray on close (Windows)">Minimize to tray on close (Windows)</span>
                    <a id="tt-help-close-to-tray" class="notes-link" href="javascript:void(0);">
                        <span class="fa-solid fa-circle-question note-link-span" title="Learn more" data-i18n="[title]Learn more"></span>
                    </a>
                </div>
                <input id="tt-close-to-tray-on-close" type="checkbox" style="margin: 0;" />
            </div>
        `.trim()
        : '';

    const interfacePanel = closeToTrayRow
        ? `
            <div class="flex-container flexFlowColumn" style="gap: 10px; padding: 12px; border: 1px solid rgba(255,255,255,0.10); border-radius: 10px; background: rgba(0,0,0,0.12);">
                <div class="flex-container alignItemsBaseline" style="justify-content: space-between; gap: 10px;">
                    <b data-i18n="Interface">Interface</b>
                </div>

                ${closeToTrayRow}
            </div>
        `.trim()
        : '';

    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.style.gap = '12px';
    root.innerHTML = `
        <div class="flex-container flexFlowColumn" style="gap: 12px;">
            <b data-i18n="TauriTavern Settings">TauriTavern Settings</b>

            ${interfacePanel}

            <div class="flex-container flexFlowColumn" style="gap: 10px; padding: 12px; border: 1px solid rgba(255,255,255,0.10); border-radius: 10px; background: rgba(0,0,0,0.12);">
                <div class="flex-container alignItemsBaseline" style="justify-content: space-between; gap: 10px;">
                    <b data-i18n="Performance">Performance</b>
                </div>

                <div class="flex-container alignItemsCenter" style="justify-content: space-between; gap: 12px; flex-wrap: wrap;">
                    <div class="flex-container alignItemsBaseline" style="gap: 8px; min-width: 220px; flex: 1;">
                        <span data-i18n="Panel Runtime">Panel Runtime</span>
                        <a id="tt-help-panel-runtime" class="notes-link" href="javascript:void(0);">
                            <span class="fa-solid fa-circle-question note-link-span" title="Learn more" data-i18n="[title]Learn more"></span>
                        </a>
                    </div>
                    <select id="tt-panel-runtime-profile" class="text_pole" style="margin: 0; width: auto; min-width: 260px; max-width: 100%; flex: 1;">
                        <option value="compat" data-i18n="Compact (Recommended)">Compact (Recommended)</option>
                        <option value="aggressive" data-i18n="Aggressive (More DOM Parking)">Aggressive (More DOM Parking)</option>
                        <option value="off" data-i18n="Off (Legacy)">Off (Legacy)</option>
                    </select>
                </div>

                <div class="flex-container alignItemsCenter" style="justify-content: space-between; gap: 12px; flex-wrap: wrap;">
                    <div class="flex-container alignItemsBaseline" style="gap: 8px; min-width: 220px; flex: 1;">
                        <span data-i18n="Embedded Runtime">Embedded Runtime</span>
                        <a id="tt-help-embedded-runtime" class="notes-link" href="javascript:void(0);">
                            <span class="fa-solid fa-circle-question note-link-span" title="Learn more" data-i18n="[title]Learn more"></span>
                        </a>
                    </div>
                    <select id="tt-embedded-runtime-profile" class="text_pole" style="margin: 0; width: auto; min-width: 260px; max-width: 100%; flex: 1;">
                        <option value="auto" data-i18n="Auto (Recommended)">Auto (Recommended)</option>
                        <option value="compat" data-i18n="Balanced">Balanced</option>
                        <option value="mobile-safe" data-i18n="Power Saver">Power Saver</option>
                        <option value="off" data-i18n="Off (Legacy)">Off (Legacy)</option>
                    </select>
                </div>

                <div class="flex-container alignItemsCenter" style="justify-content: space-between; gap: 12px; flex-wrap: wrap;">
                    <div class="flex-container alignItemsBaseline" style="gap: 8px; min-width: 220px; flex: 1;">
                        <span data-i18n="Chat History">Chat History</span>
                        <a id="tt-help-chat-history" class="notes-link" href="javascript:void(0);">
                            <span class="fa-solid fa-circle-question note-link-span" title="Learn more" data-i18n="[title]Learn more"></span>
                        </a>
                    </div>
                    <select id="tt-chat-history-mode" class="text_pole" style="margin: 0; width: auto; min-width: 260px; max-width: 100%; flex: 1;">
                        <option value="windowed" data-i18n="Windowed (Recommended)">Windowed (Recommended)</option>
                        <option value="off" data-i18n="Off (Upstream full history)">Off (Upstream full history)</option>
                    </select>
                </div>

                <small style="opacity: 0.85;" data-i18n="Requires reload to apply.">Requires reload to apply.</small>
            </div>

            <div class="flex-container flexFlowColumn" style="gap: 10px; padding: 12px; border: 1px solid rgba(255,255,255,0.10); border-radius: 10px; background: rgba(0,0,0,0.12);">
                <div class="flex-container alignItemsBaseline" style="justify-content: space-between; gap: 10px;">
                    <b data-i18n="System">System</b>
                </div>

                <style>
                    #tt-request-proxy-details > summary::-webkit-details-marker { display: none; }
                    #tt-request-proxy-details > summary::marker { content: ""; }
                    #tt-request-proxy-summary-chevron { transition: transform 140ms ease; }
                    #tt-request-proxy-details[open] #tt-request-proxy-summary-chevron { transform: rotate(180deg); }
                    #tt-request-proxy-details > summary:hover { background: rgba(0,0,0,0.18); }
                    #tt-dynamic-theme-details > summary::-webkit-details-marker { display: none; }
                    #tt-dynamic-theme-details > summary::marker { content: ""; }
                    #tt-dynamic-theme-summary-chevron { transition: transform 140ms ease; }
                    #tt-dynamic-theme-details[open] #tt-dynamic-theme-summary-chevron { transform: rotate(180deg); }
                    #tt-dynamic-theme-details > summary:hover { background: rgba(0,0,0,0.18); }
                </style>

                <details id="tt-request-proxy-details">
                    <summary id="tt-request-proxy-summary" class="flex-container alignItemsCenter" style="cursor: pointer; gap: 12px; padding: 8px 10px; border: 1px solid rgba(255,255,255,0.08); border-radius: 8px; background: rgba(0,0,0,0.10); user-select: none;">
                        <div class="flex-container alignItemsCenter" style="gap: 8px; flex: 1; min-width: 220px;">
                            <span data-i18n="Request Proxy (Advanced)">Request Proxy (Advanced)</span>
                        </div>
                        <div class="flex-container alignItemsCenter" style="gap: 8px;">
                            <small id="tt-request-proxy-summary-hint" style="opacity: 0.75;"></small>
                            <i id="tt-request-proxy-summary-chevron" class="fa-solid fa-chevron-down" style="opacity: 0.8;"></i>
                        </div>
                    </summary>

                    <div class="flex-container flexFlowColumn" style="gap: 10px; padding-top: 10px;">
                        <div class="flex-container alignItemsCenter" style="justify-content: space-between; gap: 12px; flex-wrap: wrap;">
                            <span data-i18n="Enable Request Proxy">Enable Request Proxy</span>
                            <input id="tt-request-proxy-enabled" type="checkbox" style="margin: 0;" />
                        </div>

                        <div class="flex-container alignItemsBaseline" style="justify-content: space-between; gap: 12px; flex-wrap: wrap;">
                            <span data-i18n="Request Proxy URL">Request Proxy URL</span>
                            <input id="tt-request-proxy-url" class="text_pole" type="text" style="margin: 0; width: auto; min-width: 260px; max-width: 100%; flex: 1;" placeholder="http://127.0.0.1:7890" />
                        </div>

                        <div class="flex-container flexFlowColumn" style="gap: 6px;">
                            <span data-i18n="Bypass (one per line)">Bypass (one per line)</span>
                            <textarea id="tt-request-proxy-bypass" rows="6" style="width: 100%; resize: vertical;" placeholder="localhost&#10;127.0.0.1&#10;10.0.0.0/8"></textarea>
                            <small style="opacity: 0.85;" data-i18n="Matching hosts will connect directly (no proxy).">Matching hosts will connect directly (no proxy).</small>
                        </div>

                        <small style="opacity: 0.85;" data-i18n="Applies to all backend requests.">Applies to all backend requests.</small>
                    </div>
                </details>
            </div>

            <div class="flex-container flexFlowColumn" style="gap: 10px; padding: 12px; border: 1px solid rgba(255,255,255,0.10); border-radius: 10px; background: rgba(0,0,0,0.12);">
                <div class="flex-container alignItemsBaseline" style="justify-content: space-between; gap: 10px;">
                    <b data-i18n="Misc">Misc</b>
                </div>

	                <div class="flex-container alignItemsCenter" style="justify-content: space-between; gap: 12px; flex-wrap: wrap;">
	                    <div class="flex-container alignItemsBaseline" style="gap: 8px; min-width: 220px; flex: 1;">
	                        <span data-i18n="Allow Keys Exposure">Allow Keys Exposure</span>
	                        <a id="tt-help-allow-keys-exposure" class="notes-link" href="javascript:void(0);">
	                            <span
	                                class="fa-solid fa-circle-question note-link-span"
	                                title="When enabled, API keys can be viewed/copied inside the app. Takes effect after restart."
	                                data-i18n="[title]When enabled, API keys can be viewed/copied inside the app. Takes effect after restart."
                            ></span>
                        </a>
                    </div>
                    <input id="tt-allow-keys-exposure" type="checkbox" style="margin: 0;" />
                </div>

                <details id="tt-dynamic-theme-details">
                    <summary class="flex-container alignItemsCenter" style="cursor: pointer; gap: 12px; padding: 8px 10px; border: 1px solid rgba(255,255,255,0.08); border-radius: 8px; background: rgba(0,0,0,0.10); user-select: none;">
                        <div class="flex-container alignItemsCenter" style="gap: 8px; flex: 1; min-width: 220px;">
                            <span data-i18n="Dynamic Theme">Dynamic Theme</span>
                        </div>
                        <div class="flex-container alignItemsCenter" style="gap: 8px;">
                            <small id="tt-dynamic-theme-summary-hint" style="opacity: 0.75;"></small>
                            <i id="tt-dynamic-theme-summary-chevron" class="fa-solid fa-chevron-down" style="opacity: 0.8;"></i>
                        </div>
                    </summary>

                    <div class="flex-container flexFlowColumn" style="gap: 10px; padding-top: 10px;">
                        <div class="flex-container alignItemsCenter" style="justify-content: space-between; gap: 12px; flex-wrap: wrap;">
                            <div class="flex-container alignItemsBaseline" style="gap: 8px; min-width: 220px; flex: 1;">
                                <span data-i18n="Enable Dynamic Theme">Enable Dynamic Theme</span>
                                <a id="tt-help-dynamic-theme" class="notes-link" href="javascript:void(0);">
                                    <span class="fa-solid fa-circle-question note-link-span" title="Learn more" data-i18n="[title]Learn more"></span>
                                </a>
                            </div>
                            <input id="tt-dynamic-theme-enabled" type="checkbox" style="margin: 0;" />
                        </div>

                        <div class="flex-container alignItemsCenter" style="justify-content: space-between; gap: 12px; flex-wrap: wrap;">
                            <span data-i18n="Day Theme">Day Theme</span>
                            <select id="tt-dynamic-theme-day-theme" class="text_pole" style="margin: 0; width: auto; min-width: 260px; max-width: 100%; flex: 1;"></select>
                        </div>

                        <div class="flex-container alignItemsCenter" style="justify-content: space-between; gap: 12px; flex-wrap: wrap;">
                            <span data-i18n="Night Theme">Night Theme</span>
                            <select id="tt-dynamic-theme-night-theme" class="text_pole" style="margin: 0; width: auto; min-width: 260px; max-width: 100%; flex: 1;"></select>
                        </div>

                        <small style="opacity: 0.85;" data-i18n="Dynamic Theme hint">Automatically switches SillyTavern themes based on your system light/dark mode.</small>
                    </div>
                </details>
            </div>

            <div class="flex-container flexFlowColumn" style="gap: 10px; padding: 12px; border: 1px solid rgba(255,255,255,0.10); border-radius: 10px; background: rgba(0,0,0,0.12);">
                <div class="flex-container alignItemsBaseline" style="justify-content: space-between; gap: 10px;">
                    <b data-i18n="Development">Development</b>
                </div>

                <div class="flex-container flexFlowRow" style="gap: 10px; flex-wrap: wrap;">
                    <div id="tt-open-frontend-logs" class="menu_button" data-i18n="Frontend Logs">Frontend Logs</div>
                    <div id="tt-open-backend-logs" class="menu_button" data-i18n="Backend Logs">Backend Logs</div>
                    <div id="tt-open-llm-api-logs" class="menu_button" data-i18n="LLM API Logs">LLM API Logs</div>
                </div>
            </div>

            <div class="flex-container flexFlowColumn" style="gap: 10px; padding: 12px; border: 1px solid rgba(255,255,255,0.10); border-radius: 10px; background: rgba(0,0,0,0.12);">
                <div class="flex-container alignItemsBaseline" style="justify-content: space-between; gap: 10px;">
                    <b data-i18n="Sync">Sync</b>
                </div>
                <div class="flex-container flexFlowRow" style="gap: 10px;">
                    <div id="tt-open-sync" class="menu_button" data-i18n="Open Panel">Open Panel</div>
                </div>
            </div>
        </div>
    `.trim();

    const profileSelect = root.querySelector('#tt-panel-runtime-profile');
    if (!(profileSelect instanceof HTMLSelectElement)) {
        throw new Error('TauriTavern settings: panel runtime selector not found');
    }

    const embeddedProfileSelect = root.querySelector('#tt-embedded-runtime-profile');
    if (!(embeddedProfileSelect instanceof HTMLSelectElement)) {
        throw new Error('TauriTavern settings: embedded runtime selector not found');
    }

    const chatHistoryModeSelect = root.querySelector('#tt-chat-history-mode');
    if (!(chatHistoryModeSelect instanceof HTMLSelectElement)) {
        throw new Error('TauriTavern settings: chat history mode selector not found');
    }

    const requestProxyDetails = root.querySelector('#tt-request-proxy-details');
    if (!(requestProxyDetails instanceof HTMLDetailsElement)) {
        throw new Error('TauriTavern settings: request proxy details not found');
    }

    const requestProxySummaryHint = root.querySelector('#tt-request-proxy-summary-hint');
    if (!(requestProxySummaryHint instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: request proxy summary hint not found');
    }

    const requestProxyEnabledToggle = root.querySelector('#tt-request-proxy-enabled');
    if (!(requestProxyEnabledToggle instanceof HTMLInputElement)) {
        throw new Error('TauriTavern settings: request proxy toggle not found');
    }

    const requestProxyUrlInput = root.querySelector('#tt-request-proxy-url');
    if (!(requestProxyUrlInput instanceof HTMLInputElement)) {
        throw new Error('TauriTavern settings: request proxy url input not found');
    }

    const requestProxyBypassInput = root.querySelector('#tt-request-proxy-bypass');
    if (!(requestProxyBypassInput instanceof HTMLTextAreaElement)) {
        throw new Error('TauriTavern settings: request proxy bypass input not found');
    }

    const allowKeysExposureToggle = root.querySelector('#tt-allow-keys-exposure');
    if (!(allowKeysExposureToggle instanceof HTMLInputElement)) {
        throw new Error('TauriTavern settings: allow keys exposure toggle not found');
    }

    const dynamicThemeDetails = root.querySelector('#tt-dynamic-theme-details');
    if (!(dynamicThemeDetails instanceof HTMLDetailsElement)) {
        throw new Error('TauriTavern settings: dynamic theme details not found');
    }

    const dynamicThemeSummaryHint = root.querySelector('#tt-dynamic-theme-summary-hint');
    if (!(dynamicThemeSummaryHint instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: dynamic theme summary hint not found');
    }

    const dynamicThemeEnabledToggle = root.querySelector('#tt-dynamic-theme-enabled');
    if (!(dynamicThemeEnabledToggle instanceof HTMLInputElement)) {
        throw new Error('TauriTavern settings: dynamic theme toggle not found');
    }

    const dynamicThemeDaySelect = root.querySelector('#tt-dynamic-theme-day-theme');
    if (!(dynamicThemeDaySelect instanceof HTMLSelectElement)) {
        throw new Error('TauriTavern settings: dynamic theme day selector not found');
    }

    const dynamicThemeNightSelect = root.querySelector('#tt-dynamic-theme-night-theme');
    if (!(dynamicThemeNightSelect instanceof HTMLSelectElement)) {
        throw new Error('TauriTavern settings: dynamic theme night selector not found');
    }

    /** @type {HTMLInputElement | null} */
    let closeToTrayToggle = null;
    if (supportsCloseToTrayOnClose) {
        closeToTrayToggle = root.querySelector('#tt-close-to-tray-on-close');
        if (!(closeToTrayToggle instanceof HTMLInputElement)) {
            throw new Error('TauriTavern settings: close to tray toggle not found');
        }
    }

    const openFrontendLogsButton = root.querySelector('#tt-open-frontend-logs');
    if (!(openFrontendLogsButton instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: open frontend logs button not found');
    }
    openFrontendLogsButton.addEventListener('click', () => runOrPopup(async () => {
        const { openFrontendLogsPanel } = await import('../dev-logs.js');
        await openFrontendLogsPanel();
    }));

    const openBackendLogsButton = root.querySelector('#tt-open-backend-logs');
    if (!(openBackendLogsButton instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: open backend logs button not found');
    }
    openBackendLogsButton.addEventListener('click', () => runOrPopup(async () => {
        const { openBackendLogsPanel } = await import('../dev-logs.js');
        await openBackendLogsPanel();
    }));

    const openLlmApiLogsButton = root.querySelector('#tt-open-llm-api-logs');
    if (!(openLlmApiLogsButton instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: open llm api logs button not found');
    }
    openLlmApiLogsButton.addEventListener('click', () => runOrPopup(async () => {
        const { openLlmApiLogsPanel } = await import('../dev-logs.js');
        await openLlmApiLogsPanel();
    }));

    const currentPanelRuntimeProfile = settings.panel_runtime_profile;
    profileSelect.value = typeof currentPanelRuntimeProfile === 'string' && currentPanelRuntimeProfile ? currentPanelRuntimeProfile : 'off';

    const configuredEmbeddedRuntimeProfile = normalizeEmbeddedRuntimeProfileName(settings.embedded_runtime_profile);
    const currentEmbeddedRuntimeProfile = resolveEffectiveEmbeddedRuntimeProfileName(configuredEmbeddedRuntimeProfile);
    embeddedProfileSelect.value = currentEmbeddedRuntimeProfile;

    const currentChatHistoryMode = normalizeChatHistoryModeName(
        typeof settings.chat_history_mode === 'string' && settings.chat_history_mode
            ? settings.chat_history_mode
            : CHAT_HISTORY_MODE_WINDOWED,
    );
    chatHistoryModeSelect.value = currentChatHistoryMode;

    const currentCloseToTrayOnClose = Boolean(settings.close_to_tray_on_close);
    if (closeToTrayToggle) {
        closeToTrayToggle.checked = currentCloseToTrayOnClose;
    }

    const currentRequestProxyEnabled = Boolean(settings.request_proxy?.enabled);
    const currentRequestProxyUrl = typeof settings.request_proxy?.url === 'string' ? settings.request_proxy.url : '';
    const currentRequestProxyBypass = Array.isArray(settings.request_proxy?.bypass) ? settings.request_proxy.bypass : [];

    requestProxyDetails.open = currentRequestProxyEnabled;

    const syncRequestProxySummaryHint = () => {
        requestProxySummaryHint.textContent = translate(
            requestProxyDetails.open ? 'Click to collapse' : 'Click to expand',
        );
    };
    requestProxyDetails.addEventListener('toggle', syncRequestProxySummaryHint);
    syncRequestProxySummaryHint();

    requestProxyEnabledToggle.checked = currentRequestProxyEnabled;
    requestProxyUrlInput.value = currentRequestProxyUrl;
    requestProxyBypassInput.value = currentRequestProxyBypass.join('\n');

    const currentAllowKeysExposure = Boolean(settings.allow_keys_exposure);
    allowKeysExposureToggle.checked = currentAllowKeysExposure;

    const upstreamThemeSelect = document.getElementById('themes');
    if (!(upstreamThemeSelect instanceof HTMLSelectElement)) {
        throw new Error('TauriTavern settings: SillyTavern theme selector not found');
    }

    const syncThemeOptions = (targetSelect, storedValue) => {
        targetSelect.innerHTML = '';
        for (const option of upstreamThemeSelect.options) {
            const cloned = document.createElement('option');
            cloned.value = option.value;
            cloned.textContent = option.textContent;
            targetSelect.appendChild(cloned);
        }

        const normalizedStoredValue = String(storedValue || '').trim();
        if (!normalizedStoredValue) {
            return;
        }

        const hasStoredOption = Array.from(targetSelect.options).some((option) => option.value === normalizedStoredValue);
        if (!hasStoredOption) {
            const missing = document.createElement('option');
            missing.value = normalizedStoredValue;
            missing.textContent = normalizedStoredValue;
            targetSelect.appendChild(missing);
        }
    };

    if (!settings.dynamic_theme || typeof settings.dynamic_theme !== 'object') {
        throw new Error('TauriTavern settings: dynamic theme settings missing');
    }

    const currentDynamicThemeEnabled = Boolean(settings.dynamic_theme.enabled);
    const currentDynamicThemeDayTheme = String(settings.dynamic_theme.day_theme || '').trim();
    const currentDynamicThemeNightTheme = String(settings.dynamic_theme.night_theme || '').trim();

    dynamicThemeDetails.open = false;

    const syncDynamicThemeSummaryHint = () => {
        dynamicThemeSummaryHint.textContent = translate(
            dynamicThemeDetails.open ? 'Click to collapse' : 'Click to expand',
        );
    };
    dynamicThemeDetails.addEventListener('toggle', syncDynamicThemeSummaryHint);
    syncDynamicThemeSummaryHint();

    syncThemeOptions(dynamicThemeDaySelect, currentDynamicThemeDayTheme);
    syncThemeOptions(dynamicThemeNightSelect, currentDynamicThemeNightTheme);
    dynamicThemeEnabledToggle.checked = currentDynamicThemeEnabled;
    if (currentDynamicThemeDayTheme) {
        dynamicThemeDaySelect.value = currentDynamicThemeDayTheme;
    }
    if (currentDynamicThemeNightTheme) {
        dynamicThemeNightSelect.value = currentDynamicThemeNightTheme;
    }

    const syncRequestProxyInputs = () => {
        const enabled = requestProxyEnabledToggle.checked;
        requestProxyUrlInput.disabled = !enabled;
        requestProxyBypassInput.disabled = !enabled;
        if (enabled) {
            requestProxyDetails.open = true;
        }
    };

    requestProxyEnabledToggle.addEventListener('change', () => {
        syncRequestProxyInputs();
        if (requestProxyEnabledToggle.checked) {
            requestProxyUrlInput.focus();
        }
    });
    syncRequestProxyInputs();

    const syncDynamicThemeInputs = () => {
        const enabled = dynamicThemeEnabledToggle.checked;
        dynamicThemeDaySelect.disabled = !enabled;
        dynamicThemeNightSelect.disabled = !enabled;
    };

    dynamicThemeEnabledToggle.addEventListener('change', () => {
        syncDynamicThemeInputs();
        if (dynamicThemeEnabledToggle.checked) {
            dynamicThemeDetails.open = true;
            dynamicThemeDaySelect.focus();
        }
    });
    syncDynamicThemeInputs();

    const openSyncButton = root.querySelector('#tt-open-sync');
    if (!(openSyncButton instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: sync button not found');
    }
    openSyncButton.addEventListener('click', () => runOrPopup(async () => {
        const { openSyncPopup } = await import('./sync-popup.js');
        await openSyncPopup();
    }));

    const panelRuntimeHelp = root.querySelector('#tt-help-panel-runtime');
    if (!(panelRuntimeHelp instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: panel runtime help button not found');
    }
    panelRuntimeHelp.addEventListener('click', (event) => {
        event.preventDefault();
        runOrPopup(async () => {
            const content = document.createElement('div');
            content.className = 'flex-container flexFlowColumn';
            content.style.gap = '8px';
            content.innerHTML = `
                <b data-i18n="Panel Runtime">Panel Runtime</b>
                <div data-i18n="Panel Runtime help: compact">Compact: ~40% less DOM pressure, best compatibility.</div>
                <div data-i18n="Panel Runtime help: aggressive">Aggressive: ~60% less DOM pressure, but some scripts may not work (e.g. SPresets).</div>
                <div data-i18n="Panel Runtime help: off">Off: legacy behavior (no DOM parking).</div>
            `.trim();
            await callGenericPopup(content, POPUP_TYPE.TEXT, '', {
                okButton: translate('Close'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });
        });
    });

    const embeddedRuntimeHelp = root.querySelector('#tt-help-embedded-runtime');
    if (!(embeddedRuntimeHelp instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: embedded runtime help button not found');
    }
    embeddedRuntimeHelp.addEventListener('click', (event) => {
        event.preventDefault();
        runOrPopup(async () => {
            const content = document.createElement('div');
            content.className = 'flex-container flexFlowColumn';
            content.style.gap = '8px';
            content.innerHTML = `
                <b data-i18n="Embedded Runtime">Embedded Runtime</b>
                <div data-i18n="Embedded Runtime help: off">Off: disables TauriTavern runtime takeover and uses upstream SillyTavern behavior.</div>
                <div data-i18n="Embedded Runtime help: auto">Auto: picks a profile based on your device.</div>
                <div data-i18n="Embedded Runtime help: balanced">Balanced: keeps more runtimes active for compatibility.</div>
                <div data-i18n="Embedded Runtime help: saver">Power Saver: reduces memory/CPU by parking more aggressively.</div>
            `.trim();
            await callGenericPopup(content, POPUP_TYPE.TEXT, '', {
                okButton: translate('Close'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });
        });
    });

    const chatHistoryHelp = root.querySelector('#tt-help-chat-history');
    if (!(chatHistoryHelp instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: chat history help button not found');
    }
    chatHistoryHelp.addEventListener('click', (event) => {
        event.preventDefault();
        runOrPopup(async () => {
            const content = document.createElement('div');
            content.className = 'flex-container flexFlowColumn';
            content.style.gap = '8px';
            content.innerHTML = `
                <b data-i18n="Chat History">Chat History</b>
                <div data-i18n="Chat History help: windowed">Windowed: drastically improves loading speed and reduces memory usage for long chats by loading only the most recent messages.</div>
                <div data-i18n="Chat History help: off">Off: legacy upstream behavior, loads the entire chat history at once.</div>
            `.trim();
            await callGenericPopup(content, POPUP_TYPE.TEXT, '', {
                okButton: translate('Close'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });
        });
    });

    if (supportsCloseToTrayOnClose) {
        const closeToTrayHelp = root.querySelector('#tt-help-close-to-tray');
        if (!(closeToTrayHelp instanceof HTMLElement)) {
            throw new Error('TauriTavern settings: close to tray help button not found');
        }
        closeToTrayHelp.addEventListener('click', (event) => {
            event.preventDefault();
            runOrPopup(async () => {
                const content = document.createElement('div');
                content.className = 'flex-container flexFlowColumn';
                content.style.gap = '8px';
                content.innerHTML = `
                    <b data-i18n="Minimize to tray on close (Windows)">Minimize to tray on close (Windows)</b>
                    <div data-i18n="Minimize to tray help: on">On: clicking the window close button hides TauriTavern to the system tray.</div>
                    <div data-i18n="Minimize to tray help: off">Off: clicking close exits the app.</div>
                    <div data-i18n="Minimize to tray help: exit">Use the tray icon menu to show the window or exit.</div>
                `.trim();
                await callGenericPopup(content, POPUP_TYPE.TEXT, '', {
                    okButton: translate('Close'),
                    allowVerticalScrolling: true,
                    wide: false,
                    large: false,
                });
            });
        });
    }

    const allowKeysExposureHelp = root.querySelector('#tt-help-allow-keys-exposure');
    if (!(allowKeysExposureHelp instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: allow keys exposure help button not found');
    }
    allowKeysExposureHelp.addEventListener('click', (event) => {
        event.preventDefault();
        runOrPopup(async () => {
            const content = document.createElement('div');
            content.className = 'flex-container flexFlowColumn';
            content.style.gap = '8px';
            content.innerHTML = `
                <b data-i18n="Allow Keys Exposure">Allow Keys Exposure</b>
                <div data-i18n="When enabled, API keys can be viewed/copied inside the app. Takes effect after restart.">When enabled, API keys can be viewed/copied inside the app. Takes effect after restart.</div>
            `.trim();
            await callGenericPopup(content, POPUP_TYPE.TEXT, '', {
                okButton: translate('Close'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });
        });
    });

    const dynamicThemeHelp = root.querySelector('#tt-help-dynamic-theme');
    if (!(dynamicThemeHelp instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: dynamic theme help button not found');
    }
    dynamicThemeHelp.addEventListener('click', (event) => {
        event.preventDefault();
        runOrPopup(async () => {
            const content = document.createElement('div');
            content.className = 'flex-container flexFlowColumn';
            content.style.gap = '8px';
            content.innerHTML = `
                <b data-i18n="Dynamic Theme">Dynamic Theme</b>
                <div data-i18n="Dynamic Theme help: mapping">Day = system light mode. Night = system dark mode.</div>
                <div data-i18n="Dynamic Theme help: behavior">The switch is equivalent to manually changing the theme selector.</div>
            `.trim();
            await callGenericPopup(content, POPUP_TYPE.TEXT, '', {
                okButton: translate('Close'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });
        });
    });

    const result = await callGenericPopup(root, POPUP_TYPE.CONFIRM, '', {
        okButton: translate('Save'),
        cancelButton: translate('Close'),
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });

    if (result !== POPUP_RESULT.AFFIRMATIVE) {
        return;
    }

    const nextPanelRuntimeProfile = String(profileSelect.value || '').trim();
    const nextEmbeddedRuntimeProfile = normalizeEmbeddedRuntimeProfileName(embeddedProfileSelect.value);
    const nextChatHistoryMode = normalizeChatHistoryModeName(chatHistoryModeSelect.value);
    const nextCloseToTrayOnClose = closeToTrayToggle
        ? closeToTrayToggle.checked
        : currentCloseToTrayOnClose;

    const nextDynamicThemeEnabled = dynamicThemeEnabledToggle.checked;
    const nextDynamicThemeDayTheme = String(dynamicThemeDaySelect.value || '').trim();
    const nextDynamicThemeNightTheme = String(dynamicThemeNightSelect.value || '').trim();

    const nextAllowKeysExposure = allowKeysExposureToggle.checked;

    const normalizeRequestProxyBypass = (value) => {
        return String(value || '')
            .split(/\r?\n/)
            .flatMap((line) => line.split(','))
            .map((entry) => entry.trim())
            .filter(Boolean);
    };

    const nextRequestProxyEnabled = requestProxyEnabledToggle.checked;
    const nextRequestProxyUrl = String(requestProxyUrlInput.value || '').trim();
    const nextRequestProxyBypass = normalizeRequestProxyBypass(requestProxyBypassInput.value);

    const normalizedCurrentRequestProxyBypass = normalizeRequestProxyBypass(currentRequestProxyBypass.join('\n'));
    const normalizedCurrentRequestProxyUrl = String(currentRequestProxyUrl || '').trim();

    const arraysEqual = (left, right) => {
        if (left.length !== right.length) {
            return false;
        }

        for (let index = 0; index < left.length; index += 1) {
            if (left[index] !== right[index]) {
                return false;
            }
        }

        return true;
    };

    const hasPanelRuntimeChange = Boolean(nextPanelRuntimeProfile) && nextPanelRuntimeProfile !== currentPanelRuntimeProfile;
    const requiresEmbeddedRuntimeMigration = configuredEmbeddedRuntimeProfile !== currentEmbeddedRuntimeProfile;
    const hasEmbeddedRuntimeChange = Boolean(nextEmbeddedRuntimeProfile)
        && (nextEmbeddedRuntimeProfile !== currentEmbeddedRuntimeProfile || requiresEmbeddedRuntimeMigration);
    const hasChatHistoryModeChange = nextChatHistoryMode !== currentChatHistoryMode;
    const hasCloseToTrayOnCloseChange = nextCloseToTrayOnClose !== currentCloseToTrayOnClose;
    const hasDynamicThemeChange = nextDynamicThemeEnabled !== currentDynamicThemeEnabled
        || nextDynamicThemeDayTheme !== currentDynamicThemeDayTheme
        || nextDynamicThemeNightTheme !== currentDynamicThemeNightTheme;
    const hasAllowKeysExposureChange = nextAllowKeysExposure !== currentAllowKeysExposure;
    const hasRequestProxyChange = nextRequestProxyEnabled !== currentRequestProxyEnabled
        || nextRequestProxyUrl !== normalizedCurrentRequestProxyUrl
        || !arraysEqual(nextRequestProxyBypass, normalizedCurrentRequestProxyBypass);

    if (!hasPanelRuntimeChange && !hasEmbeddedRuntimeChange && !hasChatHistoryModeChange && !hasCloseToTrayOnCloseChange && !hasDynamicThemeChange && !hasAllowKeysExposureChange && !hasRequestProxyChange) {
        return;
    }

    /** @type {Record<string, unknown>} */
    const nextSettings = {};
    if (hasPanelRuntimeChange) {
        nextSettings.panel_runtime_profile = nextPanelRuntimeProfile;
    }
    if (hasEmbeddedRuntimeChange) {
        nextSettings.embedded_runtime_profile = nextEmbeddedRuntimeProfile;
    }
    if (hasChatHistoryModeChange) {
        nextSettings.chat_history_mode = nextChatHistoryMode;
    }
    if (hasCloseToTrayOnCloseChange) {
        nextSettings.close_to_tray_on_close = nextCloseToTrayOnClose;
    }
    if (hasDynamicThemeChange) {
        nextSettings.dynamic_theme = {
            enabled: nextDynamicThemeEnabled,
            day_theme: nextDynamicThemeDayTheme,
            night_theme: nextDynamicThemeNightTheme,
        };
    }
    if (hasAllowKeysExposureChange) {
        nextSettings.allow_keys_exposure = nextAllowKeysExposure;
    }
    if (hasRequestProxyChange) {
        nextSettings.request_proxy = {
            enabled: nextRequestProxyEnabled,
            url: nextRequestProxyUrl,
            bypass: nextRequestProxyBypass,
        };
    }

    const updatedSettings = await updateTauriTavernSettings(nextSettings);

    if (hasDynamicThemeChange) {
        window.dispatchEvent(new CustomEvent(DYNAMIC_THEME_CHANGED_EVENT, {
            detail: updatedSettings.dynamic_theme,
        }));
    }

    if (hasPanelRuntimeChange) {
        // Keep in sync with:
        // - src/tauri/main/services/panel-runtime/preinstall.js
        // - src/tauri/main/services/panel-runtime/install.js
        //
        // Mirror the chosen profile so bootstrap can synchronously honor `off`
        // before Tauri settings are loaded.
        localStorage.setItem('tt:panelRuntimeProfile', nextPanelRuntimeProfile);
    }

    if (hasEmbeddedRuntimeChange) {
        setEmbeddedRuntimeBootstrapProfileName(nextEmbeddedRuntimeProfile);
        clearLegacyEmbeddedRuntimeProfileName();
    }

    if (hasChatHistoryModeChange) {
        setChatHistoryBootstrapModeName(nextChatHistoryMode);
    }

    if (hasPanelRuntimeChange || hasEmbeddedRuntimeChange || hasChatHistoryModeChange) {
        window.location.reload();
    }
}
