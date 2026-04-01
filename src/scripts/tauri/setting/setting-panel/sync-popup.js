import { callGenericPopup, POPUP_RESULT, POPUP_TYPE } from '../../../popup.js';
import { isMobile } from '../../../RossAscends-mods.js';
import { t, translate } from '../../../i18n.js';
import { scanQrCodeWithBackCancellation } from '../../../../tauri/main/services/barcode-scanner/barcode-scanner-service.js';
import { LAN_SYNC_DEVICES_CHANGED_EVENT, TT_SYNC_SERVERS_CHANGED_EVENT } from './constants.js';
import { formatTimestamp } from './formatters.js';
import { runOrPopup } from './popup-utils.js';

const LAN_SYNC_DEVICE_ALIAS_STORAGE_PREFIX = 'tauritavern:lan_sync_device_alias:';
const TT_SYNC_SERVER_ALIAS_STORAGE_PREFIX = 'tauritavern:tt_sync_server_alias:';
const LAN_SYNC_ADVERTISE_ADDRESS_STORAGE_KEY = 'tauritavern:lan_sync_advertise_address';

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

export async function openSyncPopup() {
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

