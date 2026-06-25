import { callGenericPopup, POPUP_RESULT, POPUP_TYPE } from '../../../popup.js';
import { isMobile } from '../../../RossAscends-mods.js';
import { t, translate } from '../../../i18n.js';
import { scanQrCodeWithBackCancellation } from '../../../../tauri/main/services/barcode-scanner/barcode-scanner-service.js';
import {
    LAN_SYNC_DEVICES_CHANGED_EVENT,
    SYNC_AUTOMATION_CHANGED_EVENT,
    SYNC_AUTOMATION_STATUS_CHANGED_EVENT,
    TT_SYNC_SERVERS_CHANGED_EVENT,
} from './constants.js';
import { formatTimestamp } from './formatters.js';
import { showErrorPopup } from './popup-utils.js';
import { callTauriTavernPanelPopup } from '../panel-popup.js';
import {
    clearSyncTargetAlias,
    getLanSyncAdvertiseAddress,
    getSyncV2DatasetSelection,
    getSyncTargetAlias,
    getSyncTargetDisplayName,
    parseLanSyncPairUri,
    parseTtSyncPairUri,
    selectLanSyncAdvertiseAddress,
    setLanSyncAdvertiseAddress,
    setSyncV2DatasetSelection,
    setSyncTargetAlias,
} from './sync-state.js';

const SYNC_STYLE_ID = 'tauritavern-sync-style';

function getInvoke() {
    const invoke = window.__TAURI__?.core?.invoke;
    if (typeof invoke !== 'function') {
        throw new Error('Tauri invoke API is unavailable');
    }
    return invoke;
}

function ensureSyncStyle() {
    if (document.getElementById(SYNC_STYLE_ID)) {
        return;
    }

    const link = document.createElement('link');
    link.id = SYNC_STYLE_ID;
    link.rel = 'stylesheet';
    link.href = new URL('./sync-app.css', import.meta.url).href;
    document.head.appendChild(link);
}

async function importSyncBundle() {
    return import(new URL('../dist/sync.bundle.js', import.meta.url).href);
}

function createPopupColumn() {
    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.style.gap = '10px';
    return root;
}

function buildMirrorWarningContent(titleText, detailText) {
    const content = createPopupColumn();

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

function normalizeStatus(status) {
    return {
        running: Boolean(status?.running),
        address: String(status?.address || ''),
        availableAddresses: Array.isArray(status?.available_addresses)
            ? status.available_addresses
            : [],
        pairingEnabled: Boolean(status?.pairing_enabled),
        pairingExpiresAtMs: status?.pairing_expires_at_ms ?? null,
        syncMode: status?.sync_mode ?? 'Incremental',
        syncModeOverridden: Boolean(status?.sync_mode_overridden),
    };
}

function normalizePairingInfo(pairingInfo) {
    if (!pairingInfo) {
        return null;
    }

    return {
        address: String(pairingInfo.address || ''),
        pairUri: String(pairingInfo.pair_uri || ''),
        qrSvg: String(pairingInfo.qr_svg || ''),
        expiresAtMs: pairingInfo.expires_at_ms ?? null,
    };
}

function normalizeDatasetCatalog(catalog) {
    const supportedDatasetIds = ensureArray(
        catalog?.supported_dataset_ids,
        'sync_v2_get_dataset_catalog.supported_dataset_ids',
    ).map(String);
    const defaultDatasetIds = ensureArray(
        catalog?.default_dataset_ids,
        'sync_v2_get_dataset_catalog.default_dataset_ids',
    ).map(String);

    return {
        policyVersion: Number(catalog?.policy_version),
        supportedDatasetIds,
        supportedProfileIds: ensureArray(
            catalog?.supported_profile_ids,
            'sync_v2_get_dataset_catalog.supported_profile_ids',
        ).map(String),
        defaultDatasetIds,
    };
}

function normalizeLanDevice(device) {
    const id = String(device.device_id || '');
    const name = String(device.device_name || id);

    return {
        type: 'lan',
        id,
        name,
        displayName: getSyncTargetDisplayName('lan', id, name),
        lastKnownAddress: device.last_known_address || '',
        pairedAtMs: device.paired_at_ms ?? null,
        lastSyncMs: device.last_sync_ms ?? null,
    };
}

function normalizeTtSyncServer(server) {
    const id = String(server.server_device_id || '');
    const name = String(server.server_device_name || id);

    return {
        type: 'tt',
        id,
        name,
        displayName: getSyncTargetDisplayName('tt', id, name),
        baseUrl: String(server.base_url || ''),
        spkiSha256: String(server.spki_sha256 || ''),
        permissions: server.permissions || {},
        pairedAtMs: server.paired_at_ms ?? null,
        lastSyncMs: server.last_sync_ms ?? null,
    };
}

function normalizeAutomationTarget(target) {
    if (!target) {
        return null;
    }
    if (target.type === 'lan') {
        return {
            type: 'lan',
            id: String(target.device_id || ''),
        };
    }
    if (target.type === 'tt') {
        return {
            type: 'tt',
            id: String(target.server_device_id || ''),
        };
    }
    return null;
}

function serializeAutomationTarget(target) {
    if (!target) {
        return null;
    }
    if (target.type === 'lan') {
        return {
            type: 'lan',
            device_id: target.id,
        };
    }
    if (target.type === 'tt') {
        return {
            type: 'tt',
            server_device_id: target.id,
        };
    }
    throw new Error(`Unsupported auto sync target type: ${target.type}`);
}

function normalizeAutomationConfig(config, syncSelection) {
    return {
        lanServerAutoStart: Boolean(config?.lan_server_auto_start),
        autoSyncEnabled: Boolean(config?.auto_sync_enabled),
        intervalMinutes: Number(config?.interval_minutes || 30),
        target: normalizeAutomationTarget(config?.target),
        selection: syncSelection,
    };
}

function serializeAutomationConfig(config, syncSelection) {
    return {
        lan_server_auto_start: Boolean(config?.lanServerAutoStart),
        auto_sync_enabled: Boolean(config?.autoSyncEnabled),
        interval_minutes: Number(config?.intervalMinutes || 30),
        target: serializeAutomationTarget(config?.target),
        selection: syncSelection,
    };
}

function normalizeAutomationStatus(status) {
    return {
        running: Boolean(status?.running),
        nextRunAtMs: status?.next_run_at_ms ?? null,
        lastAttemptAtMs: status?.last_attempt_at_ms ?? null,
        lastSuccessAtMs: status?.last_success_at_ms ?? null,
        lastError: status?.last_error || '',
    };
}

function ensureArray(value, commandName) {
    if (!Array.isArray(value)) {
        throw new Error(`${commandName} returned non-array`);
    }
    return value;
}

function createSyncClient() {
    const invoke = getInvoke();

    return {
        async loadState() {
            const [
                rawStatus,
                rawDevices,
                rawServers,
                rawCatalog,
                rawAutomationConfig,
                rawAutomationStatus,
            ] = await Promise.all([
                invoke('lan_sync_get_status'),
                invoke('lan_sync_list_devices'),
                invoke('tt_sync_list_servers'),
                invoke('sync_v2_get_dataset_catalog'),
                invoke('sync_automation_get_config'),
                invoke('sync_automation_get_status'),
            ]);
            const status = normalizeStatus(rawStatus);
            const selectedAddress = selectLanSyncAdvertiseAddress(status, getLanSyncAdvertiseAddress());
            setLanSyncAdvertiseAddress(selectedAddress);
            const datasetCatalog = normalizeDatasetCatalog(rawCatalog);
            const syncSelection = getSyncV2DatasetSelection(datasetCatalog);

            return {
                status,
                selectedAddress,
                datasetCatalog,
                syncSelection,
                automationConfig: normalizeAutomationConfig(rawAutomationConfig, syncSelection),
                automationStatus: normalizeAutomationStatus(rawAutomationStatus),
                devices: ensureArray(rawDevices, 'lan_sync_list_devices').map(normalizeLanDevice),
                servers: ensureArray(rawServers, 'tt_sync_list_servers').map(normalizeTtSyncServer),
            };
        },
        setAdvertiseAddress(address) {
            setLanSyncAdvertiseAddress(address);
        },
        startLanServer: () => invoke('lan_sync_start_server'),
        stopLanServer: () => invoke('lan_sync_stop_server'),
        enableLanPairing: (address) => invoke('lan_sync_enable_pairing', { address }).then(normalizePairingInfo),
        getLanPairingInfo: (address) => invoke('lan_sync_get_pairing_info', { address }).then(normalizePairingInfo),
        requestLanPairing: (pairUri) => invoke('lan_sync_request_pairing', { pairUri }),
        removeLanDevice: (deviceId) => invoke('lan_sync_remove_device', { deviceId }),
        pullLanDevice: (deviceId, options) => invoke('lan_sync_sync_from_device', { deviceId, options }),
        pushLanDevice: (deviceId, options) => invoke('lan_sync_push_to_device', { deviceId, options }),
        setLanSyncMode: (mode, persist) => invoke('lan_sync_set_sync_mode', { mode, persist }),
        clearLanSyncModeOverride: () => invoke('lan_sync_clear_sync_mode_override'),
        pairTtSync: (pairUri) => invoke('tt_sync_pair', { pairUri }),
        removeTtSyncServer: async (serverDeviceId) => {
            await invoke('tt_sync_remove_server', { serverDeviceId });
            window.dispatchEvent(new Event(TT_SYNC_SERVERS_CHANGED_EVENT));
        },
        pullTtSyncServer: (serverDeviceId, mode, options) => invoke('tt_sync_pull', { serverDeviceId, mode, options }),
        pushTtSyncServer: (serverDeviceId, mode, options) => invoke('tt_sync_push', { serverDeviceId, mode, options }),
        updateAutomationConfig: (config, syncSelection) => invoke('sync_automation_update_config', {
            config: serializeAutomationConfig(config, syncSelection),
        }).then((saved) => normalizeAutomationConfig(saved, syncSelection)),
        getAutomationStatus: () => invoke('sync_automation_get_status').then(normalizeAutomationStatus),
    };
}

async function confirmTtSyncPairing(pairUri) {
    const parsed = parseTtSyncPairUri(pairUri, translate);
    const content = createPopupColumn();

    const title = document.createElement('b');
    title.textContent = translate('TT-Sync pairing confirmation (v2 client)');
    content.appendChild(title);

    const meta = createPopupColumn();
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

    return result === POPUP_RESULT.AFFIRMATIVE;
}

async function changeSyncMode(client, status) {
    const effective = status?.syncMode ?? 'Incremental';
    const overridden = Boolean(status?.syncModeOverridden);

    if (effective === 'Mirror') {
        if (overridden) {
            await client.clearLanSyncModeOverride();
            return true;
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
            return false;
        }

        await client.setLanSyncMode('Incremental', true);
        return true;
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
        await client.setLanSyncMode('Mirror', false);
        return true;
    }

    if (result !== POPUP_RESULT.CUSTOM1) {
        return false;
    }

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
        return false;
    }

    await client.setLanSyncMode('Mirror', true);
    return true;
}

async function editSyncScope(catalog, selection) {
    const content = document.createElement('div');
    const bundle = await importSyncBundle();
    const appHandle = bundle.mountTauriTavernSyncScopeApp(content, {
        catalog,
        selection,
        tr: translate,
    });

    try {
        const result = await callGenericPopup(content, POPUP_TYPE.CONFIRM, '', {
            okButton: translate('Save'),
            cancelButton: translate('Cancel'),
            allowVerticalScrolling: true,
            wide: true,
            large: true,
            defaultResult: POPUP_RESULT.NEGATIVE,
        });

        if (result !== POPUP_RESULT.AFFIRMATIVE) {
            return null;
        }

        return setSyncV2DatasetSelection(appHandle.getSelection(), catalog);
    } finally {
        appHandle.unmount();
    }
}

function createSyncActions(client) {
    return {
        copyText: async (text) => {
            await navigator.clipboard.writeText(String(text ?? ''));
        },
        scanPairUri: () => scanQrCodeWithBackCancellation(),
        reportError: (error) => showErrorPopup(error),
        changeSyncMode: (status) => changeSyncMode(client, status),
        editSyncScope: ({ catalog, selection }) => editSyncScope(catalog, selection),
        renameTarget: async ({ type, id, fallbackName }) => {
            const existing = getSyncTargetAlias(type, id);
            const result = await callGenericPopup(
                translate('Rename paired device (local only). Leave empty to reset.'),
                POPUP_TYPE.INPUT,
                existing || fallbackName,
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
                return false;
            }

            const trimmed = result.trim();
            if (!trimmed) {
                clearSyncTargetAlias(type, id);
            } else {
                setSyncTargetAlias(type, id, trimmed);
            }
            return true;
        },
        connectPairUri: async (pairUri) => {
            const trimmed = String(pairUri || '').trim();
            const parsedUrl = new URL(trimmed);

            if (parsedUrl.hostname.toLowerCase() === 'tt-sync') {
                if (!await confirmTtSyncPairing(trimmed)) {
                    return false;
                }
                await client.pairTtSync(trimmed);
                window.dispatchEvent(new Event(TT_SYNC_SERVERS_CHANGED_EVENT));
                return true;
            }

            parseLanSyncPairUri(trimmed, translate);
            await client.requestLanPairing(trimmed);
            return true;
        },
        notifyLanPushRequested: () => {
            toastr.success(translate('Upload request sent.'));
        },
    };
}

function canScanPairUri() {
    return isMobile() && Boolean(window.__TAURI__?.barcodeScanner?.scan);
}

export async function openSyncPopup() {
    ensureSyncStyle();

    const bundle = await importSyncBundle();
    const client = createSyncClient();
    const mount = document.createElement('div');
    const appHandle = bundle.mountTauriTavernSyncApp(mount, {
        client,
        actions: createSyncActions(client),
        canScanPairUri: canScanPairUri(),
        tr: translate,
    });

    const refresh = () => {
        void appHandle.refresh();
    };
    const refreshAutomationStatus = () => {
        void appHandle.refreshAutomationStatus();
    };

    window.addEventListener(LAN_SYNC_DEVICES_CHANGED_EVENT, refresh);
    window.addEventListener(TT_SYNC_SERVERS_CHANGED_EVENT, refresh);
    window.addEventListener(SYNC_AUTOMATION_CHANGED_EVENT, refresh);
    window.addEventListener(SYNC_AUTOMATION_STATUS_CHANGED_EVENT, refreshAutomationStatus);

    try {
        await callTauriTavernPanelPopup(mount, POPUP_TYPE.TEXT, '', {
            okButton: translate('Close'),
            allowVerticalScrolling: true,
            wide: false,
            large: false,
        });
    } finally {
        window.removeEventListener(LAN_SYNC_DEVICES_CHANGED_EVENT, refresh);
        window.removeEventListener(TT_SYNC_SERVERS_CHANGED_EVENT, refresh);
        window.removeEventListener(SYNC_AUTOMATION_CHANGED_EVENT, refresh);
        window.removeEventListener(SYNC_AUTOMATION_STATUS_CHANGED_EVENT, refreshAutomationStatus);
        appHandle.unmount();
    }
}
