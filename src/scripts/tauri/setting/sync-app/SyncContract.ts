/**
 * Boundary contract for the sync-app feature.
 *
 * This module is the single home for the narrow types shared by the Sync
 * mounts, the runtime validation of the JavaScript host boundary, and the
 * small pure encodings both the view and the controller rely on. It contains
 * no state, no React and no host access.
 */

export type SyncTranslate = (key: string) => string;

// ── Dataset scope (shared with SyncScopeApp) ───────────────────────────────

export type SyncDatasetSelection = {
    policy_version: number;
    dataset_ids: string[];
};

export type SyncScopeDatasetCatalog = {
    policyVersion: number;
    supportedDatasetIds: string[];
    defaultDatasetIds: string[];
};

// ── Host-normalized DTOs (see setting-panel/sync-popup.js) ─────────────────

export type SyncOverwritePolicy = 'exact' | 'prefer-newer';

export type SyncStatus = {
    running: boolean;
    address: string;
    availableAddresses: string[];
    pairingEnabled: boolean;
    pairingExpiresAtMs: number | null;
    syncMode: string;
    syncModeOverridden: boolean;
    overwritePolicy: SyncOverwritePolicy;
};

export type SyncLanDevice = {
    type: 'lan';
    id: string;
    name: string;
    displayName: string;
    lastKnownAddress: string;
    pairedAtMs: number | null;
    lastSyncMs: number | null;
};

export type SyncTtSyncServer = {
    type: 'tt';
    id: string;
    name: string;
    displayName: string;
    baseUrl: string;
    spkiSha256: string;
    permissions: {
        write?: boolean;
        mirror_delete?: boolean;
    };
    pairedAtMs: number | null;
    lastSyncMs: number | null;
};

export type SyncTarget = SyncLanDevice | SyncTtSyncServer;

export type SyncPairingInfo = {
    address: string;
    pairUri: string;
    qrSvg: string;
    expiresAtMs: number | null;
};

export type SyncAutomationTarget = {
    type: string;
    id: string;
};

export type SyncAutomationConfig = {
    lanServerAutoStart: boolean;
    autoSyncEnabled: boolean;
    intervalMinutes: number;
    target: SyncAutomationTarget | null;
    syncMode: string;
    selection: SyncDatasetSelection | null;
};

export type SyncAutomationStatus = {
    running: boolean;
    nextRunAtMs: number | null;
    lastAttemptAtMs: number | null;
    lastSuccessAtMs: number | null;
    lastRequestAcceptedAtMs: number | null;
    lastErrorAtMs: number | null;
    lastError: string;
};

export type SyncLoadedState = {
    status: SyncStatus;
    selectedAddress: string;
    datasetCatalog: SyncScopeDatasetCatalog;
    syncSelection: SyncDatasetSelection;
    automationConfig: SyncAutomationConfig;
    automationStatus: SyncAutomationStatus;
    devices: SyncLanDevice[];
    servers: SyncTtSyncServer[];
};

export type SyncOperationOptions = {
    selection: SyncDatasetSelection;
    overwrite_policy: string;
    require_bundle_zstd: true;
};

/** The panel only inspects the result status; the full shape is the listener's. */
export type SyncJobReport = {
    result?: {
        status?: string | null;
    } | null;
} | null;

// ── Host ports ─────────────────────────────────────────────────────────────

export type SyncClient = {
    loadState: () => Promise<SyncLoadedState>;
    setAdvertiseAddress: (address: string) => void;
    startLanServer: () => Promise<unknown>;
    stopLanServer: () => Promise<unknown>;
    enableLanPairing: (address: string | null) => Promise<SyncPairingInfo | null>;
    getLanPairingInfo: (address: string) => Promise<SyncPairingInfo | null>;
    removeLanDevice: (deviceId: string) => Promise<unknown>;
    pullLanDevice: (deviceId: string, options: SyncOperationOptions) => Promise<SyncJobReport>;
    pushLanDevice: (deviceId: string, options: SyncOperationOptions) => Promise<SyncJobReport>;
    setOverwritePolicy: (overwritePolicy: string) => Promise<unknown>;
    removeTtSyncServer: (serverDeviceId: string) => Promise<unknown>;
    pullTtSyncServer: (serverDeviceId: string, mode: string, options: SyncOperationOptions) => Promise<SyncJobReport>;
    pushTtSyncServer: (serverDeviceId: string, mode: string, options: SyncOperationOptions) => Promise<SyncJobReport>;
    updateAutomationConfig: (
        config: SyncAutomationConfig,
        selection: SyncDatasetSelection | null,
    ) => Promise<SyncAutomationConfig>;
    getAutomationStatus: () => Promise<SyncAutomationStatus>;
};

export type SyncActions = {
    copyText: (text: string) => Promise<unknown>;
    scanPairUri: () => Promise<string | null>;
    changeSyncMode: (status: SyncStatus | null) => Promise<boolean>;
    editSyncScope: (current: {
        catalog: SyncScopeDatasetCatalog | null;
        selection: SyncDatasetSelection | null;
    }) => Promise<SyncDatasetSelection | null>;
    showOverwritePolicyHelp: () => Promise<unknown>;
    renameTarget: (target: { type: string; id: string; fallbackName: string }) => Promise<boolean>;
    connectPairUri: (pairUri: string) => Promise<boolean>;
    notifyLanPushRequested: () => void;
    reportError: (error: unknown) => unknown;
    showSyncReportResult: (report: SyncJobReport) => Promise<void>;
};

export type SyncMainOptions = {
    client: SyncClient;
    actions: SyncActions;
    canScanPairUri?: boolean;
    tr: SyncTranslate;
};

export type SyncMainHandle = {
    refresh: () => Promise<void>;
    refreshAutomationStatus: () => Promise<void>;
    unmount: () => void;
};

// ── Boundary validation ────────────────────────────────────────────────────

const REQUIRED_CLIENT_METHODS = [
    'loadState',
    'setAdvertiseAddress',
    'startLanServer',
    'stopLanServer',
    'enableLanPairing',
    'getLanPairingInfo',
    'removeLanDevice',
    'pullLanDevice',
    'pushLanDevice',
    'setOverwritePolicy',
    'removeTtSyncServer',
    'pullTtSyncServer',
    'pushTtSyncServer',
    'updateAutomationConfig',
    'getAutomationStatus',
] as const;

const REQUIRED_ACTIONS = [
    'copyText',
    'scanPairUri',
    'changeSyncMode',
    'editSyncScope',
    'showOverwritePolicyHelp',
    'renameTarget',
    'connectPairUri',
    'notifyLanPushRequested',
    'reportError',
    'showSyncReportResult',
] as const;

function requireMethods(source: unknown, names: readonly string[], label: string): void {
    const record = source as Record<string, unknown> | null | undefined;
    for (const name of names) {
        if (typeof record?.[name] !== 'function') {
            throw new Error(`TauriTavern Sync ${label} is unavailable: ${name}`);
        }
    }
}

/** Validates the parts of the JS host boundary that TypeScript cannot see. */
export function validateSyncMainBoundary(
    options: SyncMainOptions | undefined,
): asserts options is SyncMainOptions {
    if (typeof options?.tr !== 'function') {
        throw new Error('TauriTavern Sync translator is required');
    }
    requireMethods(options.client, REQUIRED_CLIENT_METHODS, 'client method');
    requireMethods(options.actions, REQUIRED_ACTIONS, 'action');
}

// ── Automation target encoding ("<type>:<id>" in the target select) ────────

export function automationTargetValue(target: SyncAutomationTarget | null | undefined): string {
    if (!target?.type || !target?.id) {
        return '';
    }
    return `${target.type}:${target.id}`;
}

export function parseAutomationTargetValue(value: string): SyncAutomationTarget | null {
    const raw = String(value || '').trim();
    if (!raw) {
        return null;
    }

    const separator = raw.indexOf(':');
    if (separator <= 0) {
        throw new Error(`Invalid auto sync target: ${raw}`);
    }

    return {
        type: raw.slice(0, separator),
        id: raw.slice(separator + 1),
    };
}
