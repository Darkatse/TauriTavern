import { callGenericPopup, POPUP_RESULT, POPUP_TYPE } from '../../../popup.js';
import { isMobile } from '../../../RossAscends-mods.js';
import { t, translate } from '../../../i18n.js';
import { scanQrCodeWithBackCancellation } from '../../../../tauri/main/services/barcode-scanner/barcode-scanner-service.js';
import { LAN_SYNC_DEVICES_CHANGED_EVENT, TT_SYNC_SERVERS_CHANGED_EVENT } from './constants.js';
import { formatTimestamp } from './formatters.js';
import { showErrorPopup } from './popup-utils.js';
import { callTauriTavernPanelPopup } from '../panel-popup.js';
import {
    clearSyncTargetAlias,
    getLanSyncAdvertiseAddress,
    getSyncTargetAlias,
    getSyncTargetDisplayName,
    parseTtSyncPairUri,
    selectLanSyncAdvertiseAddress,
    setLanSyncAdvertiseAddress,
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
        v2Running: Boolean(status?.v2_running),
        v2Port: status?.v2_port ?? null,
        v2SpkiSha256: String(status?.v2_spki_sha256 || ''),
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
        v2Address: String(pairingInfo.v2_address || ''),
        v2PairUri: String(pairingInfo.v2_pair_uri || ''),
        v2QrSvg: String(pairingInfo.v2_qr_svg || ''),
    };
}

function normalizeLanDevice(device) {
    const id = String(device.device_id || '');
    const name = String(device.device_name || id);

    return {
        type: 'lan',
        id,
        name,
        protocolVersion: Number(device.protocol_version || 1),
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
            const [rawStatus, rawDevices, rawServers] = await Promise.all([
                invoke('lan_sync_get_status'),
                invoke('lan_sync_list_devices'),
                invoke('tt_sync_list_servers'),
            ]);
            const status = normalizeStatus(rawStatus);
            const selectedAddress = selectLanSyncAdvertiseAddress(status, getLanSyncAdvertiseAddress());
            setLanSyncAdvertiseAddress(selectedAddress);

            return {
                status,
                selectedAddress,
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
        pullLanDevice: (deviceId) => invoke('lan_sync_sync_from_device', { deviceId }),
        pushLanDevice: (deviceId) => invoke('lan_sync_push_to_device', { deviceId }),
        setLanSyncMode: (mode, persist) => invoke('lan_sync_set_sync_mode', { mode, persist }),
        clearLanSyncModeOverride: () => invoke('lan_sync_clear_sync_mode_override'),
        pairTtSync: (pairUri) => invoke('tt_sync_pair', { pairUri }),
        removeTtSyncServer: async (serverDeviceId) => {
            await invoke('tt_sync_remove_server', { serverDeviceId });
            window.dispatchEvent(new Event(TT_SYNC_SERVERS_CHANGED_EVENT));
        },
        pullTtSyncServer: (serverDeviceId, mode) => invoke('tt_sync_pull', { serverDeviceId, mode }),
        pushTtSyncServer: (serverDeviceId, mode) => invoke('tt_sync_push', { serverDeviceId, mode }),
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

function createSyncActions(client) {
    return {
        copyText: async (text) => {
            await navigator.clipboard.writeText(String(text ?? ''));
        },
        scanPairUri: () => scanQrCodeWithBackCancellation(),
        reportError: (error) => showErrorPopup(error),
        changeSyncMode: (status) => changeSyncMode(client, status),
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

    window.addEventListener(LAN_SYNC_DEVICES_CHANGED_EVENT, refresh);
    window.addEventListener(TT_SYNC_SERVERS_CHANGED_EVENT, refresh);

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
        appHandle.unmount();
    }
}
