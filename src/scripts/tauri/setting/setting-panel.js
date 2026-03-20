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

const TAURITAVERN_SETTINGS_BUTTON_ID = 'tauritavern_settings_button';
const LAN_SYNC_DEVICES_CHANGED_EVENT = 'tauritavern:lan_sync_devices_changed';
const DEVICE_ALIAS_STORAGE_PREFIX = 'tauritavern:lan_sync_device_alias:';
const LAN_SYNC_ADVERTISE_ADDRESS_STORAGE_KEY = 'tauritavern:lan_sync_advertise_address';
let pairingListenerInstalled = false;
let syncListenerInstalled = false;
let syncProgressPopup = null;
let syncProgressElements = null;

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

export function installLanSyncPanel() {
    installPairingListener();
    installSyncListeners();

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', bindLanSyncButton, { once: true });
        return;
    }

    bindLanSyncButton();
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

            ensureSyncProgressPopup();
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
    })();
}

function ensureSyncProgressPopup() {
    if (syncProgressPopup) {
        return syncProgressPopup;
    }

    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.style.gap = '10px';

    const title = document.createElement('b');
    title.textContent = translate('LAN Sync progress');
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

    syncProgressElements = { phase, counts, bytes, current };
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

    const phase = payload.phase;
    const filesDone = payload.files_done;
    const filesTotal = payload.files_total;
    const bytesDone = payload.bytes_done;
    const bytesTotal = payload.bytes_total;
    const currentPath = payload.current_path;

    syncProgressElements.phase.textContent = t`Phase: ${translate(phase)}`;
    syncProgressElements.counts.textContent = t`Files: ${filesDone}/${filesTotal}`;
    syncProgressElements.bytes.textContent = t`Bytes: ${formatBytes(bytesDone)}/${formatBytes(bytesTotal)}`;
    syncProgressElements.current.textContent = currentPath ? t`Current: ${currentPath}` : '';
}

function getDeviceAlias(deviceId) {
    return localStorage.getItem(`${DEVICE_ALIAS_STORAGE_PREFIX}${deviceId}`) || '';
}

function setDeviceAlias(deviceId, alias) {
    localStorage.setItem(`${DEVICE_ALIAS_STORAGE_PREFIX}${deviceId}`, alias);
}

function clearDeviceAlias(deviceId) {
    localStorage.removeItem(`${DEVICE_ALIAS_STORAGE_PREFIX}${deviceId}`);
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
    const barcodeScanner = window.__TAURI__.barcodeScanner;
    const granted = await barcodeScanner.requestPermissions();
    if (!granted) {
        throw new Error(translate('Camera permission is required to scan QR codes'));
    }

    const result = await barcodeScanner.scan({ formats: [barcodeScanner.Format.QRCode] });

    const content = String(result?.content ?? '').trim();
    if (!content) {
        throw new Error(translate('Scanned Pair URI is empty'));
    }

    return content;
}

async function openTauriTavernSettingsPopup() {
    const settings = await getTauriTavernSettings();

    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.style.gap = '12px';
    root.innerHTML = `
        <div class="flex-container flexFlowColumn" style="gap: 12px;">
            <b data-i18n="TauriTavern Settings">TauriTavern Settings</b>

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
                    <b data-i18n="LAN Sync">LAN Sync</b>
                </div>
                <div class="flex-container flexFlowRow" style="gap: 10px;">
                    <div id="tt-open-lan-sync" class="menu_button" data-i18n="Open Panel">Open Panel</div>
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

    const openLanSyncButton = root.querySelector('#tt-open-lan-sync');
    if (!(openLanSyncButton instanceof HTMLElement)) {
        throw new Error('TauriTavern settings: LAN sync button not found');
    }
    openLanSyncButton.addEventListener('click', () => runOrPopup(openLanSyncPopup));

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

    const hasPanelRuntimeChange = Boolean(nextPanelRuntimeProfile) && nextPanelRuntimeProfile !== currentPanelRuntimeProfile;
    const requiresEmbeddedRuntimeMigration = configuredEmbeddedRuntimeProfile !== currentEmbeddedRuntimeProfile;
    const hasEmbeddedRuntimeChange = Boolean(nextEmbeddedRuntimeProfile)
        && (nextEmbeddedRuntimeProfile !== currentEmbeddedRuntimeProfile || requiresEmbeddedRuntimeMigration);
    const hasChatHistoryModeChange = nextChatHistoryMode !== currentChatHistoryMode;

    if (!hasPanelRuntimeChange && !hasEmbeddedRuntimeChange && !hasChatHistoryModeChange) {
        return;
    }

    /** @type {Record<string, string>} */
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

    window.location.reload();
}

async function openLanSyncPopup() {
    const panel = buildLanSyncPopup();

    const onDevicesChanged = () => {
        void panel.refresh();
    };
    window.addEventListener(LAN_SYNC_DEVICES_CHANGED_EVENT, onDevicesChanged);

    await callGenericPopup(panel.root, POPUP_TYPE.TEXT, '', {
        okButton: translate('Close'),
        allowVerticalScrolling: true,
        wide: false,
        large: false,
        onClose: () => {
            window.removeEventListener(LAN_SYNC_DEVICES_CHANGED_EVENT, onDevicesChanged);
        },
    });
}

function buildLanSyncPopup() {
    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.innerHTML = `
        <div class="flex-container flexFlowColumn" style="gap: 10px;">
            <div class="flex-container alignItemsBaseline" style="justify-content: space-between; gap: 10px;">
                <b data-i18n="LAN Sync">LAN Sync</b>
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

    const buildMirrorWarningContent = (titleText, detailText) => {
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

    const renderDevices = (devices) => {
        devicesContainer.innerHTML = '';

        if (devices.length === 0) {
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

            const name = document.createElement('b');
            const alias = getDeviceAlias(deviceId);
            name.textContent = alias || deviceName;
            name.style.cursor = 'pointer';
            name.title = translate('Click to rename');
            name.addEventListener('click', () => {
                runOrPopup(async () => {
                    const existing = getDeviceAlias(deviceId);
                    const initial = existing || deviceName;
                    const result = await callGenericPopup(translate('Rename paired device (local only). Leave empty to reset.'), POPUP_TYPE.INPUT, initial, {
                        okButton: translate('Save'),
                        cancelButton: translate('Cancel'),
                        rows: 1,
                        allowVerticalScrolling: true,
                        wide: false,
                        large: false,
                    });

                    if (typeof result !== 'string') {
                        return;
                    }

                    const trimmed = result.trim();
                    if (!trimmed) {
                        clearDeviceAlias(deviceId);
                    } else {
                        setDeviceAlias(deviceId, trimmed);
                    }

                    renderDevices(currentDevices);
                });
            });
            meta.appendChild(name);

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
        renderDevices(currentDevices);
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

    const requestPairing = async (pairUri) => {
        await invoke('lan_sync_request_pairing', { pairUri });
        requestPairUriTextArea.value = '';
        await refresh();
    };

    if (!isMobile() || !window.__TAURI__?.barcodeScanner?.scan) {
        scanPairingButton.style.display = 'none';
    } else {
        scanPairingButton.addEventListener('click', () => runOrPopup(async () => {
            const pairUri = await scanPairUriFromCamera();
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
