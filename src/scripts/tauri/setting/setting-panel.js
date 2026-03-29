import { callGenericPopup, POPUP_RESULT, POPUP_TYPE, Popup } from '../../popup.js';
import { isMobile } from '../../RossAscends-mods.js';
import { t, translate } from '../../i18n.js';
import { getTauriTavernSettings, updateTauriTavernSettings } from '../../../tauri-bridge.js';
import {
    clearLegacyEmbeddedRuntimeProfileName,
    normalizeEmbeddedRuntimeProfileName,
    resolveEffectiveEmbeddedRuntimeProfileName,
    setEmbeddedRuntimeBootstrapProfileName,
} from '../../../tauri/main/services/embedded-runtime/embedded-runtime-profile-state.js';
import {
    CHAT_HISTORY_MODE_WINDOWED,
    normalizeChatHistoryModeName,
    setChatHistoryBootstrapModeName,
} from '../../../tauri/main/services/chat-history/chat-history-mode-state.js';
import { scanQrCodeWithBackCancellation } from '../../../tauri/main/services/barcode-scanner/barcode-scanner-service.js';

const TAURITAVERN_SETTINGS_BUTTON_ID = 'tauritavern_settings_button';
const LAN_SYNC_DEVICES_CHANGED_EVENT = 'tauritavern:lan_sync_devices_changed';
const TT_SYNC_SERVERS_CHANGED_EVENT = 'tauritavern:tt_sync_servers_changed';
const LAN_SYNC_DEVICE_ALIAS_STORAGE_PREFIX = 'tauritavern:lan_sync_device_alias:';
const TT_SYNC_SERVER_ALIAS_STORAGE_PREFIX = 'tauritavern:tt_sync_server_alias:';
const LAN_SYNC_ADVERTISE_ADDRESS_STORAGE_KEY = 'tauritavern:lan_sync_advertise_address';
let pairingListenerInstalled = false;
let syncListenerInstalled = false;
let syncProgressPopup = null;
let syncProgressElements = null;

function isWindowsPlatform() {
    return typeof navigator !== 'undefined'
        && /windows/i.test(String(navigator.userAgent || ''));
}

async function showErrorPopup(error) {
    const message = error?.message ? String(error.message) : String(error);
    await callGenericPopup(translate(message), POPUP_TYPE.TEXT, '', {
        okButton: translate('OK'),
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });
}

function runOrPopup(task) {
    void (async () => {
        try {
            await task();
        } catch (error) {
            await showErrorPopup(error);
        }
    })();
}

export function installTauriTavernSettingsPanel() {
    installPairingListener();
    installSyncListeners();

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', bindLanSyncButton, { once: true });
        return;
    }

    bindLanSyncButton();
}

function buildMirrorWarningContent(titleText, detailText) {
    const content = document.createElement('div');
    content.className = 'flex-container flexFlowColumn';
    content.style.gap = '10px';

    const header = document.createElement('div');
    header.className = 'flex-container alignItemsBaseline';
    header.style.gap = '8px';

    const icon = document.createElement('i');
    icon.className = 'fa-solid fa-triangle-exclamation';
    icon.style.color = 'var(--fullred)';
    header.appendChild(icon);

    const title = document.createElement('b');
    title.textContent = translate(titleText);
    header.appendChild(title);

    content.appendChild(header);

    const details = document.createElement('div');
    details.style.opacity = '0.95';
    details.style.whiteSpace = 'pre-wrap';
    details.textContent = translate(detailText);
    content.appendChild(details);

    return content;
}

function bindLanSyncButton() {
    const button = document.getElementById(TAURITAVERN_SETTINGS_BUTTON_ID);
    if (!button) {
        return;
    }

    button.addEventListener('click', () => {
        runOrPopup(openTauriTavernSettingsPopup);
    });
}

function installPairingListener() {
    if (pairingListenerInstalled) {
        return;
    }
    pairingListenerInstalled = true;

    const invoke = window.__TAURI__.core.invoke;
    const listen = window.__TAURI__.event.listen;

    void (async () => {
        await listen('lan_sync:pair_request', async (event) => {
            const payload = event.payload;
            const requestId = payload.request_id;
            const peerDeviceName = payload.peer_device_name;
            const peerDeviceId = payload.peer_device_id;
            const peerIp = payload.peer_ip;

            const content = document.createElement('div');
            content.className = 'flex-container flexFlowColumn';
            content.style.gap = '10px';

            const title = document.createElement('b');
            title.textContent = translate('LAN Sync pairing request');
            content.appendChild(title);

            const details = document.createElement('div');
            details.className = 'flex-container flexFlowColumn';
            details.style.gap = '6px';

            const deviceLine = document.createElement('div');
            deviceLine.textContent = `${translate('Device')}: ${peerDeviceName} (${peerDeviceId})`;
            details.appendChild(deviceLine);

            const ipLine = document.createElement('div');
            ipLine.textContent = `${translate('IP')}: ${peerIp}`;
            details.appendChild(ipLine);

            content.appendChild(details);

            const result = await callGenericPopup(content, POPUP_TYPE.CONFIRM, '', {
                okButton: translate('Allow'),
                cancelButton: translate('Deny'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });

            const accept = result === POPUP_RESULT.AFFIRMATIVE;
            await invoke('lan_sync_confirm_pairing', { requestId, accept });
            if (accept) {
                window.dispatchEvent(new Event(LAN_SYNC_DEVICES_CHANGED_EVENT));
            }
        });
    })();
}

function installSyncListeners() {
    if (syncListenerInstalled) {
        return;
    }
    syncListenerInstalled = true;

    const listen = window.__TAURI__.event.listen;

    void (async () => {
        await listen('lan_sync:progress', (event) => {
            const payload = event.payload;

            ensureSyncProgressPopup('LAN Sync progress');
            updateSyncProgressPopup(payload);
        });

        await listen('lan_sync:completed', async (event) => {
            const payload = event.payload;

            if (syncProgressPopup) {
                await syncProgressPopup.completeAffirmative();
            }
            syncProgressPopup = null;
            syncProgressElements = null;

            const files = payload.files_total;
            const bytes = payload.bytes_total;
            const deleted = payload.files_deleted;
            const message = [
                translate('LAN Sync completed.'),
                t`Files: ${files}`,
                typeof deleted === 'number' && deleted > 0 ? t`Deleted: ${deleted}` : null,
                t`Bytes: ${formatBytes(bytes)}`,
                '',
                translate('The app will now reload to refresh data.'),
            ].filter(Boolean).join('\n');
            await callGenericPopup(message, POPUP_TYPE.TEXT, '', {
                okButton: translate('OK'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });

            window.location.reload();
        });

        await listen('lan_sync:error', async (event) => {
            const payload = event.payload;

            if (syncProgressPopup) {
                await syncProgressPopup.completeAffirmative();
            }
            syncProgressPopup = null;
            syncProgressElements = null;

            const message = translate(payload.message);
            await callGenericPopup(String(message), POPUP_TYPE.TEXT, '', {
                okButton: translate('OK'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });
        });

        await listen('tt_sync:progress', (event) => {
            const payload = event.payload;

            ensureSyncProgressPopup('TT-Sync progress');
            updateSyncProgressPopup(payload);
        });

        await listen('tt_sync:completed', async (event) => {
            const payload = event.payload;

            if (syncProgressPopup) {
                await syncProgressPopup.completeAffirmative();
            }
            syncProgressPopup = null;
            syncProgressElements = null;

            window.dispatchEvent(new Event(TT_SYNC_SERVERS_CHANGED_EVENT));

            const files = payload.files_total;
            const bytes = payload.bytes_total;
            const deleted = payload.files_deleted;
            const direction = payload.direction === 'Push' ? translate('Push') : translate('Pull');

            const message = [
                t`TT-Sync ${direction} completed.`,
                t`Files: ${files}`,
                typeof deleted === 'number' && deleted > 0 ? t`Deleted: ${deleted}` : null,
                t`Bytes: ${formatBytes(bytes)}`,
                payload.direction === 'Pull' ? '' : null,
                payload.direction === 'Pull' ? translate('The app will now reload to refresh data.') : null,
            ].filter(Boolean).join('\n');

            await callGenericPopup(message, POPUP_TYPE.TEXT, '', {
                okButton: translate('OK'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });

            if (payload.direction === 'Pull') {
                window.location.reload();
            }
        });

        await listen('tt_sync:error', async (event) => {
            const payload = event.payload;

            if (syncProgressPopup) {
                await syncProgressPopup.completeAffirmative();
            }
            syncProgressPopup = null;
            syncProgressElements = null;

            const message = translate(payload.message);
            await callGenericPopup(String(message), POPUP_TYPE.TEXT, '', {
                okButton: translate('OK'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });
        });
    })();
}

function ensureSyncProgressPopup(titleText) {
    if (syncProgressPopup) {
        if (syncProgressElements?.title && titleText) {
            syncProgressElements.title.textContent = translate(titleText);
        }
        return syncProgressPopup;
    }

    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.style.gap = '10px';

    const title = document.createElement('b');
    title.textContent = translate(titleText || 'Sync progress');
    root.appendChild(title);

    const phase = document.createElement('div');
    root.appendChild(phase);

    const counts = document.createElement('div');
    root.appendChild(counts);

    const bytes = document.createElement('div');
    root.appendChild(bytes);

    const current = document.createElement('div');
    current.style.wordBreak = 'break-word';
    current.style.opacity = '0.9';
    root.appendChild(current);

    syncProgressElements = { title, phase, counts, bytes, current };
    updateSyncProgressPopup({
        phase: 'Starting',
        files_done: 0,
        files_total: 0,
        bytes_done: 0,
        bytes_total: 0,
        current_path: null,
    });

    const popup = new Popup(root, POPUP_TYPE.DISPLAY, '', {
        allowVerticalScrolling: true,
        wide: false,
        large: false,
    });

    syncProgressPopup = popup;
    void popup.show().finally(() => {
        if (syncProgressPopup === popup) {
            syncProgressPopup = null;
            syncProgressElements = null;
        }
    });

    return popup;
}

function updateSyncProgressPopup(payload) {
    if (!syncProgressElements) {
        return;
    }

    const direction = payload.direction || null;
    const phase = payload.phase;
    const filesDone = payload.files_done;
    const filesTotal = payload.files_total;
    const bytesDone = payload.bytes_done;
    const bytesTotal = payload.bytes_total;
    const currentPath = payload.current_path;

    syncProgressElements.phase.textContent = direction
        ? t`Phase: ${translate(direction)} / ${translate(phase)}`
        : t`Phase: ${translate(phase)}`;
    syncProgressElements.counts.textContent = t`Files: ${filesDone}/${filesTotal}`;
    syncProgressElements.bytes.textContent = t`Bytes: ${formatBytes(bytesDone)}/${formatBytes(bytesTotal)}`;
    syncProgressElements.current.textContent = currentPath ? t`Current: ${currentPath}` : '';
}

function getLocalAlias(storagePrefix, id) {
    return localStorage.getItem(`${storagePrefix}${id}`) || '';
}

function setLocalAlias(storagePrefix, id, alias) {
    localStorage.setItem(`${storagePrefix}${id}`, alias);
}

function clearLocalAlias(storagePrefix, id) {
    localStorage.removeItem(`${storagePrefix}${id}`);
}

function bindLocalRename(nameElement, storagePrefix, id, fallbackName, rerender) {
    nameElement.style.cursor = 'pointer';
    nameElement.title = translate('Click to rename');
    nameElement.addEventListener('click', () => {
        runOrPopup(async () => {
            const existing = getLocalAlias(storagePrefix, id);
            const initial = existing || fallbackName;
            const result = await callGenericPopup(
                translate('Rename paired device (local only). Leave empty to reset.'),
                POPUP_TYPE.INPUT,
                initial,
                {
                    okButton: translate('Save'),
                    cancelButton: translate('Cancel'),
                    rows: 1,
                    allowVerticalScrolling: true,
                    wide: false,
                    large: false,
                },
            );

            if (typeof result !== 'string') {
                return;
            }

            const trimmed = result.trim();
            if (!trimmed) {
                clearLocalAlias(storagePrefix, id);
            } else {
                setLocalAlias(storagePrefix, id, trimmed);
            }

            rerender();
        });
    });
}

function buildRenamableNameLine(displayName, storagePrefix, id, fallbackName, rerender) {
    const name = document.createElement('b');
    name.textContent = displayName;

    const edit = document.createElement('i');
    edit.className = 'fa-solid fa-pen-to-square';
    edit.style.marginLeft = '6px';
    edit.style.opacity = '0.7';
    edit.style.fontSize = '0.85em';

    name.appendChild(edit);

    bindLocalRename(name, storagePrefix, id, fallbackName, rerender);
    return name;
}

function getLanSyncAdvertiseAddress() {
    return localStorage.getItem(LAN_SYNC_ADVERTISE_ADDRESS_STORAGE_KEY) || '';
}

function setLanSyncAdvertiseAddress(value) {
    if (!value) {
        localStorage.removeItem(LAN_SYNC_ADVERTISE_ADDRESS_STORAGE_KEY);
        return;
    }

    localStorage.setItem(LAN_SYNC_ADVERTISE_ADDRESS_STORAGE_KEY, value);
}

async function scanPairUriFromCamera() {
    return scanQrCodeWithBackCancellation();
}

async function openTauriTavernSettingsPopup() {
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
        const { openFrontendLogsPanel } = await import('./dev-logs.js');
        await openFrontendLogsPanel();
    }));

    const openBackendLogsButton = root.querySelector('#tt-open-backend-logs');
    if (!(openBackendLogsButton instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: open backend logs button not found');
    }
    openBackendLogsButton.addEventListener('click', () => runOrPopup(async () => {
        const { openBackendLogsPanel } = await import('./dev-logs.js');
        await openBackendLogsPanel();
    }));

    const openLlmApiLogsButton = root.querySelector('#tt-open-llm-api-logs');
    if (!(openLlmApiLogsButton instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: open llm api logs button not found');
    }
    openLlmApiLogsButton.addEventListener('click', () => runOrPopup(async () => {
        const { openLlmApiLogsPanel } = await import('./dev-logs.js');
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

    const openSyncButton = root.querySelector('#tt-open-sync');
    if (!(openSyncButton instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: sync button not found');
    }
    openSyncButton.addEventListener('click', () => runOrPopup(openSyncPopup));

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
    const hasRequestProxyChange = nextRequestProxyEnabled !== currentRequestProxyEnabled
        || nextRequestProxyUrl !== normalizedCurrentRequestProxyUrl
        || !arraysEqual(nextRequestProxyBypass, normalizedCurrentRequestProxyBypass);

    if (!hasPanelRuntimeChange && !hasEmbeddedRuntimeChange && !hasChatHistoryModeChange && !hasCloseToTrayOnCloseChange && !hasRequestProxyChange) {
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
    if (hasRequestProxyChange) {
        nextSettings.request_proxy = {
            enabled: nextRequestProxyEnabled,
            url: nextRequestProxyUrl,
            bypass: nextRequestProxyBypass,
        };
    }

    await updateTauriTavernSettings(nextSettings);

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

async function openSyncPopup() {
    const panel = buildSyncPopup();

    const onDevicesChanged = () => {
        void panel.refresh();
    };
    const onServersChanged = () => {
        void panel.refresh();
    };

    window.addEventListener(LAN_SYNC_DEVICES_CHANGED_EVENT, onDevicesChanged);
    window.addEventListener(TT_SYNC_SERVERS_CHANGED_EVENT, onServersChanged);

    await callGenericPopup(panel.root, POPUP_TYPE.TEXT, '', {
        okButton: translate('Close'),
        allowVerticalScrolling: true,
        wide: false,
        large: false,
        onClose: () => {
            window.removeEventListener(LAN_SYNC_DEVICES_CHANGED_EVENT, onDevicesChanged);
            window.removeEventListener(TT_SYNC_SERVERS_CHANGED_EVENT, onServersChanged);
        },
    });
}

function buildSyncPopup() {
    return buildLanSyncPopup();
}

function buildLanSyncPopup() {
    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.innerHTML = `
            <div class="flex-container flexFlowColumn" style="gap: 10px;">
            <div class="flex-container alignItemsBaseline" style="justify-content: space-between; gap: 10px;">
                <b data-i18n="Sync">Sync</b>
                <div class="flex-container" style="gap: 10px;">
                    <div id="lan-sync-mode-button" class="menu_button menu_button_icon margin0" title="Sync mode" data-i18n="[title]Sync mode">
                        <i class="fa-solid fa-code-branch"></i>
                        <span id="lan-sync-mode-text" style="margin-left: 6px;"></span>
                    </div>
                </div>
            </div>
            <div class="flex-container flexFlowColumn" style="gap: 6px;">
                <div>
                    <span data-i18n="Status">Status</span>: <b id="lan-sync-status-text">...</b>
                </div>
                <div class="flex-container alignItemsBaseline" style="gap: 6px; flex-wrap: wrap;">
                    <span data-i18n="Address">Address</span>:
                    <select id="lan-sync-address-select" class="text_pole" style="margin: 0; width: auto; min-width: 260px; max-width: 100%; flex: 1;"></select>
                </div>
                <div>
                    <span data-i18n="Pairing">Pairing</span>: <b id="lan-sync-pairing-text">...</b>
                </div>
            </div>
            <div class="flex-container flexFlowRow" style="gap: 10px;">
                <div id="lan-sync-start" class="menu_button" data-i18n="Start">Start</div>
                <div id="lan-sync-stop" class="menu_button" data-i18n="Stop">Stop</div>
                <div id="lan-sync-enable-pairing" class="menu_button" data-i18n="Enable Pairing">Enable Pairing</div>
            </div>

            <div class="flex-container flexFlowColumn" style="gap: 6px; border-top: 1px solid rgba(255,255,255,0.08); padding-top: 10px;">
                <b data-i18n="Pair via QR">Pair via QR</b>
                <div class="flex-container flexFlowRow" style="gap: 10px; align-items: flex-start;">
                <div id="lan-sync-qr-wrap" style="width: 210px; height: 210px; background: rgba(255,255,255,0.03); display: flex; align-items: center; justify-content: center;">
                        <span style="opacity: 0.7;" data-i18n="No QR">No QR</span>
                    </div>
                    <div class="flex-container flexFlowColumn" style="gap: 6px; flex: 1;">
                        <div>
                            <span data-i18n="Expires">Expires</span>: <code id="lan-sync-pair-expiry">N/A</code>
                        </div>
                        <textarea id="lan-sync-pair-uri" rows="4" style="width: 100%; resize: vertical;" placeholder="Pair URI (scan QR or copy)" data-i18n="[placeholder]Pair URI (scan QR or copy)"></textarea>
                        <div class="flex-container flexFlowRow" style="gap: 10px;">
                            <div id="lan-sync-copy-uri" class="menu_button" data-i18n="Copy URI">Copy URI</div>
                        </div>
                    </div>
                </div>
            </div>

            <div class="flex-container flexFlowColumn" style="gap: 6px; border-top: 1px solid rgba(255,255,255,0.08); padding-top: 10px;">
                <b data-i18n="Connect device">Connect device</b>
                <div class="flex-container flexFlowColumn" style="gap: 6px;">
                    <textarea id="lan-sync-request-uri" rows="3" style="width: 100%; resize: vertical;" placeholder="Paste Pair URI here (pairs new or reconnects existing)" data-i18n="[placeholder]Paste Pair URI here (pairs new or reconnects existing)"></textarea>
                    <div class="flex-container flexFlowRow" style="gap: 10px;">
                        <div id="lan-sync-scan-pairing" class="menu_button" data-i18n="Scan">Scan</div>
                        <div id="lan-sync-request-pairing" class="menu_button" data-i18n="Connect">Connect</div>
                    </div>
                </div>
            </div>

            <div class="flex-container flexFlowColumn" style="gap: 6px; border-top: 1px solid rgba(255,255,255,0.08); padding-top: 10px;">
                <div class="flex-container alignItemsBaseline" style="justify-content: space-between;">
                    <b data-i18n="Paired devices">Paired devices</b>
                    <div class="flex-container">
                        <div id="lan-sync-devices-refresh" class="menu_button menu_button_icon margin0" title="Refresh" data-i18n="[title]Refresh">
                            <i class="fa-solid fa-arrows-rotate"></i>
                        </div>
                    </div>
                </div>
                <div id="lan-sync-devices" class="flex-container flexFlowColumn" style="gap: 6px;"></div>
            </div>
        </div>
    `.trim();

    const statusText = root.querySelector('#lan-sync-status-text');
    const addressSelect = root.querySelector('#lan-sync-address-select');
    const pairingText = root.querySelector('#lan-sync-pairing-text');
    const modeButton = root.querySelector('#lan-sync-mode-button');
    const modeButtonText = root.querySelector('#lan-sync-mode-text');
    const startButton = root.querySelector('#lan-sync-start');
    const stopButton = root.querySelector('#lan-sync-stop');
    const enablePairingButton = root.querySelector('#lan-sync-enable-pairing');

    const qrWrap = root.querySelector('#lan-sync-qr-wrap');
    const pairUriTextArea = root.querySelector('#lan-sync-pair-uri');
    const pairExpiryText = root.querySelector('#lan-sync-pair-expiry');
    const copyUriButton = root.querySelector('#lan-sync-copy-uri');
    pairExpiryText.textContent = translate(pairExpiryText.textContent);

    const requestPairUriTextArea = root.querySelector('#lan-sync-request-uri');
    const scanPairingButton = root.querySelector('#lan-sync-scan-pairing');
    const requestPairingButton = root.querySelector('#lan-sync-request-pairing');

    const devicesRefreshButton = root.querySelector('#lan-sync-devices-refresh');
    const devicesContainer = root.querySelector('#lan-sync-devices');

    const invoke = window.__TAURI__.core.invoke;
    let currentStatus = null;
    let currentDevices = [];
    let currentServers = [];
    let currentAdvertiseAddress = null;

    const getModeLabel = (status) => {
        const effective = status?.sync_mode ?? 'Incremental';
        const overridden = Boolean(status?.sync_mode_overridden);

        if (effective === 'Mirror') {
            return overridden ? translate('Mirror Mode (session)') : translate('Mirror Mode');
        }

        return translate('Incremental Mode');
    };

    const updateModeButton = (status) => {
        modeButtonText.textContent = getModeLabel(status);
        modeButton.title = translate('Sync mode');

        if (status?.sync_mode === 'Mirror') {
            modeButton.classList.add('red_button');
        } else {
            modeButton.classList.remove('red_button');
        }
    };

    modeButton.addEventListener('click', () => runOrPopup(async () => {
        if (!currentStatus) {
            await refresh();
        }

        const effective = currentStatus?.sync_mode ?? 'Incremental';
        const overridden = Boolean(currentStatus?.sync_mode_overridden);

        if (effective === 'Mirror') {
            if (overridden) {
                await invoke('lan_sync_clear_sync_mode_override');
                await refresh();
                return;
            }

            const content = buildMirrorWarningContent(
                'Switch to incremental mode?',
                'Incremental mode will not delete files on the target device during sync.',
            );

            const result = await callGenericPopup(content, POPUP_TYPE.CONFIRM, '', {
                okButton: translate('Switch'),
                cancelButton: translate('Cancel'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
                defaultResult: POPUP_RESULT.NEGATIVE,
            });

            if (result !== POPUP_RESULT.AFFIRMATIVE) {
                return;
            }

            await invoke('lan_sync_set_sync_mode', { mode: 'Incremental', persist: true });
            await refresh();
            return;
        }

        const content = buildMirrorWarningContent(
            'Mirror mode can delete files',
            'Mirror mode will delete files on the target device that do not exist on the source device. This is risky and may cause data loss.',
        );

        const result = await callGenericPopup(content, POPUP_TYPE.CONFIRM, '', {
            okButton: translate('Switch'),
            cancelButton: translate('Cancel'),
            customButtons: [
                {
                    text: translate('Always Mirror'),
                    result: POPUP_RESULT.CUSTOM1,
                    classes: ['red_button'],
                },
            ],
            allowVerticalScrolling: true,
            wide: false,
            large: false,
            defaultResult: POPUP_RESULT.NEGATIVE,
        });

        if (result === POPUP_RESULT.AFFIRMATIVE) {
            await invoke('lan_sync_set_sync_mode', { mode: 'Mirror', persist: false });
            await refresh();
            return;
        }

        if (result === POPUP_RESULT.CUSTOM1) {
            const confirmContent = buildMirrorWarningContent(
                'Always mirror mode?',
                'This will set LAN Sync to mirror mode by default. All future syncs may delete files on the target device.\n\nContinue?',
            );

            const confirmResult = await callGenericPopup(confirmContent, POPUP_TYPE.CONFIRM, '', {
                okButton: translate('Confirm'),
                cancelButton: translate('Cancel'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
                defaultResult: POPUP_RESULT.NEGATIVE,
            });

            if (confirmResult !== POPUP_RESULT.AFFIRMATIVE) {
                return;
            }

            await invoke('lan_sync_set_sync_mode', { mode: 'Mirror', persist: true });
            await refresh();
            return;
        }
    }));

    const renderPairingInfo = (pairingInfo) => {
        if (!pairingInfo) {
            pairUriTextArea.value = '';
            pairExpiryText.textContent = translate('N/A');
            qrWrap.innerHTML = '<span style="opacity: 0.7;" data-i18n="No QR">No QR</span>';
            return;
        }

        pairUriTextArea.value = pairingInfo.pair_uri || '';
        pairExpiryText.textContent = pairingInfo.expires_at_ms
            ? formatTimestamp(pairingInfo.expires_at_ms)
            : translate('N/A');

        const svg = pairingInfo.qr_svg || '';
        if (!svg) {
            qrWrap.innerHTML = '<span style="opacity: 0.7;" data-i18n="No QR">No QR</span>';
            return;
        }

        const img = document.createElement('img');
        img.alt = 'LAN Sync Pair QR';
        img.width = 200;
        img.height = 200;
        img.style.background = '#fff';
        img.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;

        qrWrap.innerHTML = '';
        qrWrap.appendChild(img);
    };

    const renderDevices = (devices, servers) => {
        devicesContainer.innerHTML = '';

        if (devices.length === 0 && servers.length === 0) {
            const empty = document.createElement('div');
            empty.style.opacity = '0.7';
            empty.textContent = translate('No paired devices');
            devicesContainer.appendChild(empty);
            return;
        }

        for (const device of devices) {
            const deviceId = device.device_id;
            const deviceName = device.device_name;

            const row = document.createElement('div');
            row.className = 'flex-container alignItemsBaseline';
            row.style.justifyContent = 'space-between';
            row.style.gap = '10px';

            const meta = document.createElement('div');
            meta.className = 'flex-container flexFlowColumn';
            meta.style.gap = '2px';

            meta.appendChild(buildRenamableNameLine(
                getLocalAlias(LAN_SYNC_DEVICE_ALIAS_STORAGE_PREFIX, deviceId) || deviceName,
                LAN_SYNC_DEVICE_ALIAS_STORAGE_PREFIX,
                deviceId,
                deviceName,
                () => renderDevices(currentDevices, currentServers),
            ));

            const deviceIdLine = document.createElement('div');
            deviceIdLine.style.opacity = '0.8';
            deviceIdLine.style.fontSize = '0.9em';
            deviceIdLine.textContent = deviceId;
            meta.appendChild(deviceIdLine);

            const addressLine = document.createElement('div');
            addressLine.style.opacity = '0.8';
            addressLine.style.fontSize = '0.9em';
            addressLine.textContent = device.last_known_address
                ? device.last_known_address
                : translate('Address: N/A (reconnect needed)');
            meta.appendChild(addressLine);

            const syncInfo = document.createElement('div');
            syncInfo.style.opacity = '0.8';
            syncInfo.style.fontSize = '0.9em';
            const lastSync = device.last_sync_ms ? formatTimestamp(device.last_sync_ms) : translate('Never');
            syncInfo.textContent = t`Last sync: ${lastSync}`;
            meta.appendChild(syncInfo);

            row.appendChild(meta);

            const actions = document.createElement('div');
            actions.className = 'flex-container';
            actions.style.gap = '10px';

            const download = document.createElement('div');
            download.className = 'menu_button menu_button_icon margin0';
            download.title = translate('Download (pull from this device)');
            download.innerHTML = '<i class="fa-solid fa-download"></i>';
            download.addEventListener('click', () => runOrPopup(async () => {
                await invoke('lan_sync_sync_from_device', { deviceId });
            }));

            const upload = document.createElement('div');
            upload.className = 'menu_button menu_button_icon margin0';
            upload.title = translate('Upload (request device to pull from you)');
            upload.innerHTML = '<i class="fa-solid fa-upload"></i>';
            upload.addEventListener('click', () => runOrPopup(async () => {
                await invoke('lan_sync_push_to_device', { deviceId });
                toastr.success(translate('Upload request sent.'));
            }));

            if (!device.last_known_address) {
                download.style.opacity = '0.6';
                download.style.pointerEvents = 'none';
                download.title = translate('Address missing. Reconnect using Pair URI.');
                upload.style.opacity = '0.6';
                upload.style.pointerEvents = 'none';
                upload.title = translate('Address missing. Reconnect using Pair URI.');
            }

            if (!currentStatus.running) {
                upload.style.opacity = '0.6';
                upload.style.pointerEvents = 'none';
                upload.title = translate('Start LAN Sync server first (peer needs to download from you).');
            }

            const remove = document.createElement('div');
            remove.className = 'menu_button menu_button_icon margin0';
            remove.title = translate('Remove device');
            remove.innerHTML = '<i class="fa-solid fa-trash-can"></i>';
            remove.addEventListener('click', () => runOrPopup(async () => {
                await invoke('lan_sync_remove_device', { deviceId });
                await refresh();
            }));

            actions.appendChild(download);
            actions.appendChild(upload);
            actions.appendChild(remove);
            row.appendChild(actions);

            devicesContainer.appendChild(row);
        }

        for (const server of servers) {
            const serverDeviceId = server.server_device_id;
            const serverDeviceName = server.server_device_name;
            const baseUrl = server.base_url;

            const row = document.createElement('div');
            row.className = 'flex-container alignItemsBaseline';
            row.style.justifyContent = 'space-between';
            row.style.gap = '10px';

            const meta = document.createElement('div');
            meta.className = 'flex-container flexFlowColumn';
            meta.style.gap = '2px';

            meta.appendChild(buildRenamableNameLine(
                getLocalAlias(TT_SYNC_SERVER_ALIAS_STORAGE_PREFIX, serverDeviceId) || serverDeviceName,
                TT_SYNC_SERVER_ALIAS_STORAGE_PREFIX,
                serverDeviceId,
                serverDeviceName,
                () => renderDevices(currentDevices, currentServers),
            ));

            const deviceIdLine = document.createElement('div');
            deviceIdLine.style.opacity = '0.8';
            deviceIdLine.style.fontSize = '0.9em';
            deviceIdLine.textContent = serverDeviceId;
            meta.appendChild(deviceIdLine);

            const addressLine = document.createElement('div');
            addressLine.style.opacity = '0.8';
            addressLine.style.fontSize = '0.9em';
            addressLine.style.wordBreak = 'break-word';
            const urlText = document.createElement('span');
            urlText.textContent = baseUrl;
            addressLine.appendChild(urlText);
            const badge = document.createElement('code');
            badge.textContent = 'TT-Sync';
            badge.style.marginLeft = '6px';
            badge.style.fontSize = '0.85em';
            addressLine.appendChild(badge);
            meta.appendChild(addressLine);

            const syncInfo = document.createElement('div');
            syncInfo.style.opacity = '0.8';
            syncInfo.style.fontSize = '0.9em';
            const lastSync = server.last_sync_ms ? formatTimestamp(server.last_sync_ms) : translate('Never');
            syncInfo.textContent = t`Last sync: ${lastSync}`;
            meta.appendChild(syncInfo);

            row.appendChild(meta);

            const actions = document.createElement('div');
            actions.className = 'flex-container';
            actions.style.gap = '10px';

            const download = document.createElement('div');
            download.className = 'menu_button menu_button_icon margin0';
            download.title = translate('Download (pull from this server)');
            download.innerHTML = '<i class="fa-solid fa-download"></i>';
            download.addEventListener('click', () => runOrPopup(async () => {
                if (!currentStatus) {
                    await refresh();
                }
                const mode = currentStatus?.sync_mode ?? 'Incremental';
                await invoke('tt_sync_pull', { serverDeviceId, mode });
            }));

            const upload = document.createElement('div');
            upload.className = 'menu_button menu_button_icon margin0';
            upload.title = translate('Upload (push to this server)');
            upload.innerHTML = '<i class="fa-solid fa-upload"></i>';
            upload.addEventListener('click', () => runOrPopup(async () => {
                if (!currentStatus) {
                    await refresh();
                }
                const mode = currentStatus?.sync_mode ?? 'Incremental';
                await invoke('tt_sync_push', { serverDeviceId, mode });
            }));

            const remove = document.createElement('div');
            remove.className = 'menu_button menu_button_icon margin0';
            remove.title = translate('Remove server');
            remove.innerHTML = '<i class="fa-solid fa-trash-can"></i>';
            remove.addEventListener('click', () => runOrPopup(async () => {
                await invoke('tt_sync_remove_server', { serverDeviceId });
                window.dispatchEvent(new Event(TT_SYNC_SERVERS_CHANGED_EVENT));
                await refresh();
            }));

            actions.appendChild(download);
            actions.appendChild(upload);
            actions.appendChild(remove);
            row.appendChild(actions);

            devicesContainer.appendChild(row);
        }
    };

    const refresh = async () => {
        const status = await invoke('lan_sync_get_status');
        currentStatus = status;
        statusText.textContent = translate(status.running ? 'Running' : 'Stopped');
        statusText.style.color = status.running ? '#0f0' : '#f00';

        const availableAddresses = status.available_addresses;

        const stored = getLanSyncAdvertiseAddress();
        const defaultAddress = status.address && availableAddresses.includes(status.address)
            ? status.address
            : availableAddresses[0] || status.address || null;

        const selected = stored && availableAddresses.includes(stored) ? stored : defaultAddress;
        currentAdvertiseAddress = selected;
        setLanSyncAdvertiseAddress(selected);

        addressSelect.innerHTML = '';
        addressSelect.disabled = availableAddresses.length === 0;
        addressSelect.title = translate('Address');

        if (availableAddresses.length === 0) {
            const option = document.createElement('option');
            option.value = '';
            option.textContent = translate('N/A');
            addressSelect.appendChild(option);
            addressSelect.value = '';
        } else {
            for (const address of availableAddresses) {
                const option = document.createElement('option');
                option.value = address;
                option.textContent = address;
                addressSelect.appendChild(option);
            }
            addressSelect.value = selected || availableAddresses[0];
        }

        updateModeButton(status);

        if (status.pairing_enabled) {
            pairingText.textContent = t`Enabled (expires ${formatTimestamp(status.pairing_expires_at_ms)})`;
            pairingText.style.color = '#0f0';
        } else {
            pairingText.textContent = translate('Disabled');
            pairingText.style.color = '#f00';
        }

        startButton.style.display = status.running ? 'none' : '';
        stopButton.style.display = status.running ? '' : 'none';
        enablePairingButton.style.display = status.running ? '' : 'none';

        const devices = await invoke('lan_sync_list_devices');
        if (!Array.isArray(devices)) {
            throw new Error('lan_sync_list_devices returned non-array');
        }
        currentDevices = devices;

        const servers = await invoke('tt_sync_list_servers');
        if (!Array.isArray(servers)) {
            throw new Error('tt_sync_list_servers returned non-array');
        }
        currentServers = servers;

        renderDevices(currentDevices, currentServers);
    };

    devicesRefreshButton.addEventListener('click', () => runOrPopup(refresh));
    startButton.addEventListener('click', () => runOrPopup(async () => {
        await invoke('lan_sync_start_server');
        await refresh();
    }));
    stopButton.addEventListener('click', () => runOrPopup(async () => {
        await invoke('lan_sync_stop_server');
        renderPairingInfo(null);
        await refresh();
    }));
    enablePairingButton.addEventListener('click', () => runOrPopup(async () => {
        const pairingInfo = await invoke('lan_sync_enable_pairing', { address: currentAdvertiseAddress });
        renderPairingInfo(pairingInfo);
        await refresh();
    }));
    copyUriButton.addEventListener('click', () => runOrPopup(async () => {
        const value = pairUriTextArea.value.trim();
        if (!value) {
            throw new Error(translate('Pair URI is empty'));
        }
        await navigator.clipboard.writeText(value);
    }));

    const confirmAndPairTtSync = async (pairUri) => {
        const parsed = parseTtSyncPairUri(pairUri);

        const content = document.createElement('div');
        content.className = 'flex-container flexFlowColumn';
        content.style.gap = '10px';

        const title = document.createElement('b');
        title.textContent = translate('TT-Sync pairing confirmation (v2 client)');
        content.appendChild(title);

        const meta = document.createElement('div');
        meta.className = 'flex-container flexFlowColumn';
        meta.style.gap = '6px';

        const urlLine = document.createElement('div');
        urlLine.style.wordBreak = 'break-word';
        urlLine.textContent = t`URL: ${parsed.baseUrl}`;
        meta.appendChild(urlLine);

        const spkiLine = document.createElement('div');
        spkiLine.style.wordBreak = 'break-word';
        const spkiLabel = document.createElement('span');
        spkiLabel.textContent = `${translate('SPKI')}: `;
        spkiLine.appendChild(spkiLabel);
        const spkiValue = document.createElement('code');
        spkiValue.textContent = parsed.spki;
        spkiLine.appendChild(spkiValue);
        meta.appendChild(spkiLine);

        if (parsed.expiresAtMs) {
            const expLine = document.createElement('div');
            expLine.textContent = t`Expires: ${formatTimestamp(parsed.expiresAtMs)}`;
            meta.appendChild(expLine);
        }

        content.appendChild(meta);

        const result = await callGenericPopup(content, POPUP_TYPE.CONFIRM, '', {
            okButton: translate('Trust & Pair'),
            cancelButton: translate('Cancel'),
            allowVerticalScrolling: true,
            wide: false,
            large: false,
            defaultResult: POPUP_RESULT.NEGATIVE,
        });

        if (result !== POPUP_RESULT.AFFIRMATIVE) {
            return;
        }

        await invoke('tt_sync_pair', { pairUri });
        window.dispatchEvent(new Event(TT_SYNC_SERVERS_CHANGED_EVENT));
    };

    const requestPairing = async (pairUri) => {
        const trimmed = String(pairUri || '').trim();
        const parsedUrl = new URL(trimmed);

        if (parsedUrl.hostname.toLowerCase() === 'tt-sync') {
            await confirmAndPairTtSync(trimmed);
            requestPairUriTextArea.value = '';
            await refresh();
            return;
        }

        await invoke('lan_sync_request_pairing', { pairUri: trimmed });
        requestPairUriTextArea.value = '';
        await refresh();
    };

    if (!isMobile() || !window.__TAURI__?.barcodeScanner?.scan) {
        scanPairingButton.style.display = 'none';
    } else {
        scanPairingButton.addEventListener('click', () => runOrPopup(async () => {
            const pairUri = await scanPairUriFromCamera();
            if (pairUri === null) {
                return;
            }
            requestPairUriTextArea.value = pairUri;
            await requestPairing(pairUri);
        }));
    }

    requestPairingButton.addEventListener('click', () => runOrPopup(async () => {
        const value = requestPairUriTextArea.value.trim();
        if (!value) {
            throw new Error(translate('Pair URI is empty'));
        }
        await requestPairing(value);
    }));

    addressSelect.addEventListener('change', () => runOrPopup(async () => {
        const next = String(addressSelect.value || '').trim();
        currentAdvertiseAddress = next || null;
        setLanSyncAdvertiseAddress(next);

        if (currentStatus?.pairing_enabled && next) {
            const pairingInfo = await invoke('lan_sync_get_pairing_info', { address: next });
            renderPairingInfo(pairingInfo);
        }
    }));

    void refresh();
    return { root, refresh };
}

function parseTtSyncPairUri(pairUri) {
    const parsed = new URL(pairUri);
    if (parsed.protocol.toLowerCase() !== 'tauritavern:') {
        throw new Error(translate('Pair URI must start with tauritavern://'));
    }

    const host = parsed.hostname.toLowerCase();
    const path = parsed.pathname.toLowerCase();
    if (host !== 'tt-sync' || path !== '/pair') {
        throw new Error(translate('Pair URI is not a TT-Sync pairing link'));
    }

    const version = parsed.searchParams.get('v') || '';
    if (version !== '2') {
        throw new Error(translate('Pair URI must be v=2'));
    }

    const baseUrl = parsed.searchParams.get('url') || '';
    if (!baseUrl) {
        throw new Error(translate('Pair URI missing url'));
    }

    const spki = parsed.searchParams.get('spki') || '';
    if (!spki) {
        throw new Error(translate('Pair URI missing spki'));
    }

    const expiresAtMsRaw = parsed.searchParams.get('exp') || '';
    const expiresAtMs = expiresAtMsRaw ? Number(expiresAtMsRaw) : null;
    if (expiresAtMsRaw && (expiresAtMs === null || Number.isNaN(expiresAtMs))) {
        throw new Error(translate('Pair URI has invalid exp'));
    }

    return { baseUrl, spki, expiresAtMs };
}

function formatTimestamp(ms) {
    if (!ms) {
        return translate('N/A');
    }

    const date = new Date(Number(ms));
    if (Number.isNaN(date.getTime())) {
        return translate('Invalid time');
    }

    return date.toLocaleString();
}

function formatBytes(value) {
    const bytes = Number(value) || 0;
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let size = bytes;
    let unitIndex = 0;

    while (size >= 1024 && unitIndex < units.length - 1) {
        size /= 1024;
        unitIndex += 1;
    }

    return `${size.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}
