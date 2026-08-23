import {
    parseAutomationTargetValue,
    type SyncActions,
    type SyncAutomationConfig,
    type SyncAutomationStatus,
    type SyncClient,
    type SyncDatasetSelection,
    type SyncJobReport,
    type SyncLanDevice,
    type SyncLoadedState,
    type SyncOperationOptions,
    type SyncOverwritePolicy,
    type SyncPairingInfo,
    type SyncScopeDatasetCatalog,
    type SyncStatus,
    type SyncTarget,
    type SyncTranslate,
    type SyncTtSyncServer,
} from './SyncContract';

/**
 * Mount-local owner of the Sync Main panel state. It is the single source of
 * truth that both the React view (via subscribe/getSnapshot) and the public
 * mount handle (refresh/refreshAutomationStatus) talk to, so neither depends
 * on a committed React render. Every transition produces a new immutable
 * snapshot and notifies listeners synchronously.
 */

export type SyncMainState = {
    status: SyncStatus | null;
    devices: SyncLanDevice[];
    servers: SyncTtSyncServer[];
    selectedAddress: string;
    pairingInfo: SyncPairingInfo | null;
    datasetCatalog: SyncScopeDatasetCatalog | null;
    syncSelection: SyncDatasetSelection | null;
    automationConfig: SyncAutomationConfig;
    automationStatus: SyncAutomationStatus;
    automationExpanded: boolean;
    automationDraftDirty: boolean;
    requestPairUri: string;
    loading: boolean;
    busy: string;
};

export type SyncController = {
    getSnapshot: () => SyncMainState;
    subscribe: (listener: () => void) => () => void;
    refresh: () => Promise<void>;
    refreshAutomationStatus: () => Promise<void>;
    changeSyncMode: () => Promise<void>;
    showOverwritePolicyHelp: () => void;
    setOverwritePolicy: (overwritePolicy: SyncOverwritePolicy) => Promise<void>;
    editSyncScope: () => Promise<void>;
    saveAutomation: () => Promise<void>;
    setLanServerAutoStart: (enabled: boolean) => Promise<void>;
    setAutoSyncEnabled: (enabled: boolean) => Promise<void>;
    setAutomationInterval: (value: string) => void;
    setAutomationMode: (value: string) => void;
    setAutomationTarget: (value: string) => void;
    setAutomationExpanded: (open: boolean) => void;
    selectAddress: (address: string) => Promise<void>;
    startServer: () => Promise<void>;
    stopServer: () => Promise<void>;
    enablePairing: () => Promise<void>;
    copyPairUri: () => Promise<void>;
    scanPairing: () => Promise<void>;
    connectPairing: () => Promise<void>;
    setRequestPairUri: (value: string) => void;
    renameTarget: (target: SyncTarget) => Promise<void>;
    pullTarget: (target: SyncTarget) => Promise<void>;
    pushTarget: (target: SyncTarget) => Promise<void>;
    removeTarget: (target: SyncTarget) => Promise<void>;
};

const DEFAULT_AUTOMATION_CONFIG: SyncAutomationConfig = {
    lanServerAutoStart: false,
    autoSyncEnabled: false,
    intervalMinutes: 30,
    target: null,
    syncMode: 'Incremental',
    selection: null,
};

const DEFAULT_AUTOMATION_STATUS: SyncAutomationStatus = {
    running: false,
    nextRunAtMs: null,
    lastAttemptAtMs: null,
    lastSuccessAtMs: null,
    lastRequestAcceptedAtMs: null,
    lastErrorAtMs: null,
    lastError: '',
};

function initialState(): SyncMainState {
    return {
        status: null,
        devices: [],
        servers: [],
        selectedAddress: '',
        pairingInfo: null,
        datasetCatalog: null,
        syncSelection: null,
        automationConfig: { ...DEFAULT_AUTOMATION_CONFIG },
        automationStatus: { ...DEFAULT_AUTOMATION_STATUS },
        automationExpanded: false,
        automationDraftDirty: false,
        requestPairUri: '',
        loading: false,
        busy: '',
    };
}

export function createSyncController({
    client,
    actions,
    tr,
}: {
    client: SyncClient;
    actions: SyncActions;
    tr: SyncTranslate;
}): SyncController {
    let state = initialState();
    const listeners = new Set<() => void>();

    function getSnapshot(): SyncMainState {
        return state;
    }

    function subscribe(listener: () => void): () => void {
        listeners.add(listener);
        return () => {
            listeners.delete(listener);
        };
    }

    function setState(patch: Partial<SyncMainState>): void {
        state = { ...state, ...patch };
        for (const listener of listeners) {
            listener();
        }
    }

    /** Patches the automation config; `dirty` marks unsaved user edits. */
    function patchAutomationConfig(patch: Partial<SyncAutomationConfig>, dirty: boolean): void {
        const automationConfig = { ...state.automationConfig, ...patch };
        setState(dirty ? { automationConfig, automationDraftDirty: true } : { automationConfig });
    }

    function reportError(error: unknown): void {
        void actions.reportError(error);
    }

    async function withBusy(name: string, task: () => Promise<void>): Promise<void> {
        const busyName = String(name || '').trim();
        setState({ busy: busyName });
        try {
            await task();
        } catch (error) {
            reportError(error);
        } finally {
            if (state.busy === busyName) {
                setState({ busy: '' });
            }
        }
    }

    async function withBusyStrict<T>(name: string, task: () => Promise<T>): Promise<T> {
        const busyName = String(name || '').trim();
        setState({ busy: busyName });
        try {
            return await task();
        } finally {
            if (state.busy === busyName) {
                setState({ busy: '' });
            }
        }
    }

    function applySnapshot(snapshot: SyncLoadedState): void {
        setState({
            status: snapshot.status,
            devices: snapshot.devices,
            servers: snapshot.servers,
            selectedAddress: snapshot.selectedAddress || '',
            datasetCatalog: snapshot.datasetCatalog,
            syncSelection: snapshot.syncSelection,
            // A dirty automation draft is the user's unsaved edit; a background
            // refresh must not clobber it.
            automationConfig: state.automationDraftDirty
                ? state.automationConfig
                : snapshot.automationConfig,
            automationStatus: snapshot.automationStatus,
        });
    }

    async function refresh(): Promise<void> {
        setState({ loading: true });
        try {
            applySnapshot(await client.loadState());
        } catch (error) {
            reportError(error);
        } finally {
            setState({ loading: false });
        }
    }

    async function refreshAutomationStatus(): Promise<void> {
        try {
            setState({ automationStatus: await client.getAutomationStatus() });
        } catch (error) {
            reportError(error);
        }
    }

    async function persistAutomationConfig(): Promise<void> {
        const saved = await client.updateAutomationConfig(state.automationConfig, state.syncSelection);
        setState({
            automationConfig: saved,
            automationDraftDirty: false,
        });
        setState({ automationStatus: await client.getAutomationStatus() });
    }

    async function runSyncCommand(command: () => Promise<SyncJobReport>): Promise<SyncJobReport> {
        const report = await command();
        await actions.showSyncReportResult(report);
        return report;
    }

    function syncOperationOptions(): SyncOperationOptions {
        if (!state.syncSelection) {
            throw new Error(tr('Sync content selection is unavailable'));
        }
        if (!state.status) {
            throw new Error(tr('Sync status is unavailable'));
        }

        return {
            selection: state.syncSelection,
            overwrite_policy: state.status.overwritePolicy,
            require_bundle_zstd: true,
        };
    }

    async function changeSyncMode(): Promise<void> {
        await withBusy('mode', async () => {
            if (!state.status) {
                await refresh();
            }
            if (await actions.changeSyncMode(state.status)) {
                await refresh();
            }
        });
    }

    async function setOverwritePolicy(overwritePolicy: SyncOverwritePolicy): Promise<void> {
        if (!state.status) {
            return;
        }

        const previous = state.status.overwritePolicy;
        setState({ status: { ...state.status, overwritePolicy } });
        try {
            await withBusyStrict('overwrite-policy', () => client.setOverwritePolicy(overwritePolicy));
        } catch (error) {
            if (state.status) {
                setState({ status: { ...state.status, overwritePolicy: previous } });
            }
            reportError(error);
        }
    }

    async function editSyncScope(): Promise<void> {
        await withBusy('scope', async () => {
            const next = await actions.editSyncScope({
                catalog: state.datasetCatalog,
                selection: state.syncSelection,
            });
            if (next) {
                setState({ syncSelection: next });
                setState({ automationConfig: { ...state.automationConfig, selection: next } });
                await persistAutomationConfig();
            }
        });
    }

    async function saveAutomation(): Promise<void> {
        await withBusy('automation', persistAutomationConfig);
    }

    async function setLanServerAutoStart(enabled: boolean): Promise<void> {
        const previous = state.automationConfig.lanServerAutoStart;
        patchAutomationConfig({ lanServerAutoStart: enabled }, false);
        try {
            await withBusyStrict('automation-port', persistAutomationConfig);
        } catch (error) {
            patchAutomationConfig({ lanServerAutoStart: previous }, false);
            reportError(error);
        }
    }

    async function setAutoSyncEnabled(enabled: boolean): Promise<void> {
        if (enabled && !state.automationConfig.target) {
            patchAutomationConfig({ autoSyncEnabled: true }, true);
            setState({ automationExpanded: true });
            return;
        }

        const previous = state.automationConfig.autoSyncEnabled;
        patchAutomationConfig({ autoSyncEnabled: enabled }, false);
        if (enabled) {
            setState({ automationExpanded: true });
        }
        try {
            await withBusyStrict('automation', persistAutomationConfig);
        } catch (error) {
            patchAutomationConfig({ autoSyncEnabled: previous }, false);
            reportError(error);
        }
    }

    function setAutomationInterval(value: string): void {
        patchAutomationConfig({ intervalMinutes: Number(value) }, true);
    }

    function setAutomationMode(value: string): void {
        patchAutomationConfig({ syncMode: value === 'Mirror' ? 'Mirror' : 'Incremental' }, true);
    }

    function setAutomationTarget(value: string): void {
        patchAutomationConfig({ target: parseAutomationTargetValue(value) }, true);
    }

    function setAutomationExpanded(open: boolean): void {
        setState({ automationExpanded: open });
    }

    function showOverwritePolicyHelp(): void {
        void actions.showOverwritePolicyHelp();
    }

    async function selectAddress(address: string): Promise<void> {
        setState({ selectedAddress: address });
        await withBusy('address', async () => {
            client.setAdvertiseAddress(address);
            if (state.status?.pairingEnabled && address) {
                setState({ pairingInfo: await client.getLanPairingInfo(address) });
            }
        });
    }

    async function startServer(): Promise<void> {
        await withBusy('start', async () => {
            await client.startLanServer();
            await refresh();
        });
    }

    async function stopServer(): Promise<void> {
        await withBusy('stop', async () => {
            await client.stopLanServer();
            setState({ pairingInfo: null });
            await refresh();
        });
    }

    async function enablePairing(): Promise<void> {
        await withBusy('pairing', async () => {
            setState({ pairingInfo: await client.enableLanPairing(state.selectedAddress || null) });
            await refresh();
        });
    }

    async function copyPairUri(): Promise<void> {
        await withBusy('copyPairUri', async () => {
            const value = (state.pairingInfo?.pairUri || '').trim();
            if (!value) {
                throw new Error(tr('Pair URI is empty'));
            }
            await actions.copyText(value);
        });
    }

    async function connectPairing(): Promise<void> {
        await withBusy('connect', async () => {
            const value = state.requestPairUri.trim();
            if (!value) {
                throw new Error(tr('Pair URI is empty'));
            }
            if (!await actions.connectPairUri(value)) {
                return;
            }
            setState({ requestPairUri: '' });
            await refresh();
        });
    }

    async function scanPairing(): Promise<void> {
        await withBusy('scan', async () => {
            const pairUri = await actions.scanPairUri();
            if (pairUri === null) {
                return;
            }
            setState({ requestPairUri: pairUri });
            await connectPairing();
        });
    }

    function setRequestPairUri(value: string): void {
        setState({ requestPairUri: value });
    }

    async function renameTarget(target: SyncTarget): Promise<void> {
        await withBusy(`rename:${target.type}:${target.id}`, async () => {
            if (await actions.renameTarget({
                type: target.type,
                id: target.id,
                fallbackName: target.name,
            })) {
                await refresh();
            }
        });
    }

    async function pullTarget(target: SyncTarget): Promise<void> {
        await withBusy(`pull:${target.type}:${target.id}`, async () => {
            const options = syncOperationOptions();
            if (target.type === 'lan') {
                await runSyncCommand(() => client.pullLanDevice(target.id, options));
                return;
            }

            const mode = state.status?.syncMode ?? 'Incremental';
            await runSyncCommand(() => client.pullTtSyncServer(target.id, mode, options));
        });
    }

    async function pushTarget(target: SyncTarget): Promise<void> {
        await withBusy(`push:${target.type}:${target.id}`, async () => {
            const options = syncOperationOptions();
            if (target.type === 'lan') {
                const report = await runSyncCommand(() => client.pushLanDevice(target.id, options));
                // LAN push is a pull-request: only an accepted request is a
                // success. A failed report already produced an error popup, so
                // it must not also show the success toast.
                if (report?.result?.status === 'remote_request_accepted') {
                    actions.notifyLanPushRequested();
                }
                return;
            }

            const mode = state.status?.syncMode ?? 'Incremental';
            await runSyncCommand(() => client.pushTtSyncServer(target.id, mode, options));
        });
    }

    async function removeTarget(target: SyncTarget): Promise<void> {
        await withBusy(`remove:${target.type}:${target.id}`, async () => {
            if (target.type === 'lan') {
                await client.removeLanDevice(target.id);
            } else {
                await client.removeTtSyncServer(target.id);
            }
            await refresh();
        });
    }

    return {
        getSnapshot,
        subscribe,
        refresh,
        refreshAutomationStatus,
        changeSyncMode,
        setOverwritePolicy,
        editSyncScope,
        saveAutomation,
        setLanServerAutoStart,
        setAutoSyncEnabled,
        setAutomationInterval,
        setAutomationMode,
        setAutomationTarget,
        setAutomationExpanded,
        showOverwritePolicyHelp,
        selectAddress,
        startServer,
        stopServer,
        enablePairing,
        copyPairUri,
        scanPairing,
        connectPairing,
        setRequestPairUri,
        renameTarget,
        pullTarget,
        pushTarget,
        removeTarget,
    };
}
