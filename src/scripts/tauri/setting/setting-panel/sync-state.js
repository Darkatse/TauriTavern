const LAN_SYNC_DEVICE_ALIAS_STORAGE_PREFIX = 'tauritavern:lan_sync_device_alias:';
const TT_SYNC_SERVER_ALIAS_STORAGE_PREFIX = 'tauritavern:tt_sync_server_alias:';
const LAN_SYNC_ADVERTISE_ADDRESS_STORAGE_KEY = 'tauritavern:lan_sync_advertise_address';
const SYNC_DATASET_SELECTION_STORAGE_KEY = 'tauritavern:sync_dataset_selection';
const LEGACY_SYNC_DATASET_SELECTION_STORAGE_KEY = 'tauritavern:sync_v2_dataset_selection';

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

export function createDefaultSyncDatasetSelection(catalog) {
    return {
        policy_version: Number(catalog?.policyVersion),
        dataset_ids: [...catalog.defaultDatasetIds],
    };
}

function normalizeSyncDatasetSelection(selection, catalog) {
    const policyVersion = Number(selection?.policy_version);
    if (!Number.isInteger(policyVersion) || policyVersion !== Number(catalog?.policyVersion)) {
        throw new Error('Stored sync content selection has an unsupported policy version');
    }

    const supported = new Set(catalog.supportedDatasetIds);
    const datasetIds = [];
    const seen = new Set();
    for (const id of selection?.dataset_ids || []) {
        const value = String(id || '').trim();
        if (!value || seen.has(value)) {
            continue;
        }
        if (!supported.has(value)) {
            throw new Error(`Stored sync content selection contains unsupported dataset: ${value}`);
        }
        seen.add(value);
        datasetIds.push(value);
    }

    if (datasetIds.length === 0) {
        throw new Error('Stored sync content selection is empty');
    }

    return {
        policy_version: policyVersion,
        dataset_ids: datasetIds,
    };
}

export function getSyncDatasetSelection(catalog) {
    for (const key of [SYNC_DATASET_SELECTION_STORAGE_KEY, LEGACY_SYNC_DATASET_SELECTION_STORAGE_KEY]) {
        const raw = localStorage.getItem(key);
        if (!raw) {
            continue;
        }

        try {
            const normalized = normalizeSyncDatasetSelection(JSON.parse(raw), catalog);
            localStorage.setItem(SYNC_DATASET_SELECTION_STORAGE_KEY, JSON.stringify(normalized));
            localStorage.removeItem(LEGACY_SYNC_DATASET_SELECTION_STORAGE_KEY);
            return normalized;
        } catch (error) {
            throw new Error(`Stored sync content selection is invalid: ${error.message}`);
        }
    }

    return createDefaultSyncDatasetSelection(catalog);
}

export function setSyncDatasetSelection(selection, catalog) {
    const normalized = normalizeSyncDatasetSelection(selection, catalog);
    localStorage.setItem(SYNC_DATASET_SELECTION_STORAGE_KEY, JSON.stringify(normalized));
    localStorage.removeItem(LEGACY_SYNC_DATASET_SELECTION_STORAGE_KEY);
    return normalized;
}

export function parseLanSyncPairUri(pairUri, tr = (key) => key) {
    const parsed = new URL(String(pairUri || '').trim());
    if (parsed.protocol.toLowerCase() !== 'tauritavern:') {
        throw new Error(tr('Pair URI must start with tauritavern://'));
    }

    const host = parsed.hostname.toLowerCase();
    const path = parsed.pathname.toLowerCase();
    if (host !== 'lan-sync' || path !== '/pair') {
        throw new Error(tr('Pair URI is not a LAN Sync pairing link'));
    }

    const version = parsed.searchParams.get('v') || '';
    if (version !== '2') {
        throw new Error(tr('LAN Sync Pair URI must be v=2'));
    }

    const baseUrl = parsed.searchParams.get('url') || '';
    if (!baseUrl) {
        throw new Error(tr('Pair URI missing url'));
    }

    const token = parsed.searchParams.get('token') || '';
    if (!token) {
        throw new Error(tr('Pair URI missing token'));
    }

    const spki = parsed.searchParams.get('spki') || '';
    if (!spki) {
        throw new Error(tr('Pair URI missing spki'));
    }

    const expiresAtMsRaw = parsed.searchParams.get('exp') || '';
    const expiresAtMs = expiresAtMsRaw ? Number(expiresAtMsRaw) : null;
    if (!expiresAtMsRaw || Number.isNaN(expiresAtMs)) {
        throw new Error(tr('Pair URI has invalid exp'));
    }

    return { baseUrl, token, spki, expiresAtMs };
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
