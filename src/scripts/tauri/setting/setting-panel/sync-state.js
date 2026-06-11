const LAN_SYNC_DEVICE_ALIAS_STORAGE_PREFIX = 'tauritavern:lan_sync_device_alias:';
const TT_SYNC_SERVER_ALIAS_STORAGE_PREFIX = 'tauritavern:tt_sync_server_alias:';
const LAN_SYNC_ADVERTISE_ADDRESS_STORAGE_KEY = 'tauritavern:lan_sync_advertise_address';

const SYNC_TARGET_STORAGE_PREFIX = {
    lan: LAN_SYNC_DEVICE_ALIAS_STORAGE_PREFIX,
    tt: TT_SYNC_SERVER_ALIAS_STORAGE_PREFIX,
};

function storagePrefixForTarget(type) {
    const prefix = SYNC_TARGET_STORAGE_PREFIX[type];
    if (!prefix) {
        throw new Error(`Unsupported sync target type: ${type}`);
    }
    return prefix;
}

export function getSyncTargetAlias(type, id) {
    return localStorage.getItem(`${storagePrefixForTarget(type)}${id}`) || '';
}

export function setSyncTargetAlias(type, id, alias) {
    localStorage.setItem(`${storagePrefixForTarget(type)}${id}`, alias);
}

export function clearSyncTargetAlias(type, id) {
    localStorage.removeItem(`${storagePrefixForTarget(type)}${id}`);
}

export function getSyncTargetDisplayName(type, id, fallbackName) {
    return getSyncTargetAlias(type, id) || fallbackName;
}

export function getLanSyncAdvertiseAddress() {
    return localStorage.getItem(LAN_SYNC_ADVERTISE_ADDRESS_STORAGE_KEY) || '';
}

export function setLanSyncAdvertiseAddress(value) {
    const normalized = String(value || '').trim();
    if (!normalized) {
        localStorage.removeItem(LAN_SYNC_ADVERTISE_ADDRESS_STORAGE_KEY);
        return;
    }

    localStorage.setItem(LAN_SYNC_ADVERTISE_ADDRESS_STORAGE_KEY, normalized);
}

export function selectLanSyncAdvertiseAddress(status, storedAddress = getLanSyncAdvertiseAddress()) {
    const availableAddresses = Array.isArray(status?.availableAddresses)
        ? status.availableAddresses
        : [];
    const currentAddress = String(status?.address || '').trim();
    const stored = String(storedAddress || '').trim();

    const defaultAddress = currentAddress && availableAddresses.includes(currentAddress)
        ? currentAddress
        : availableAddresses[0] || currentAddress || '';

    return stored && availableAddresses.includes(stored) ? stored : defaultAddress;
}

export function parseTtSyncPairUri(pairUri, tr = (key) => key) {
    const parsed = new URL(String(pairUri || '').trim());
    if (parsed.protocol.toLowerCase() !== 'tauritavern:') {
        throw new Error(tr('Pair URI must start with tauritavern://'));
    }

    const host = parsed.hostname.toLowerCase();
    const path = parsed.pathname.toLowerCase();
    if (host !== 'tt-sync' || path !== '/pair') {
        throw new Error(tr('Pair URI is not a TT-Sync pairing link'));
    }

    const version = parsed.searchParams.get('v') || '';
    if (version !== '2') {
        throw new Error(tr('Pair URI must be v=2'));
    }

    const baseUrl = parsed.searchParams.get('url') || '';
    if (!baseUrl) {
        throw new Error(tr('Pair URI missing url'));
    }

    const spki = parsed.searchParams.get('spki') || '';
    if (!spki) {
        throw new Error(tr('Pair URI missing spki'));
    }

    const expiresAtMsRaw = parsed.searchParams.get('exp') || '';
    const expiresAtMs = expiresAtMsRaw ? Number(expiresAtMsRaw) : null;
    if (expiresAtMsRaw && (expiresAtMs === null || Number.isNaN(expiresAtMs))) {
        throw new Error(tr('Pair URI has invalid exp'));
    }

    return { baseUrl, spki, expiresAtMs };
}
