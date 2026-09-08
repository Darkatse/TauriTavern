import {
    parseAutomationTargetValue,
    type SyncActions,
    type SyncAutomationConfig,
    type SyncAutomationStatus,
    type SyncClient,
    type SyncDatasetSelection,
    type SyncJobReport,
    type SyncLanDevice,
    type SyncNearbyDevice,
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
import { createSyncDiscovery, type SyncDiscoveryState } from './SyncDiscovery';

/** Panel state shared by the React view and the mount's refresh handles. */

export type SyncMainState = SyncDiscoveryState & {
    status: SyncStatus | null;
    devices: SyncLanDevice[];
    servers: SyncTtSyncServer[];
    pairingInfo: SyncPairingInfo | null;
    datasetCatalog: SyncScopeDatasetCatalog | null;
    syncSelection: SyncDatasetSelection | null;
    automationConfig: SyncAutomationConfig;
    automationStatus: SyncAutomationStatus;
    automationDraft: Partial<SyncAutomationConfig> | null;
    requestPairUri: string;
    loading: boolean;
    busy: string;
};

export type SyncController = {
    getSnapshot: () => SyncMainState;
    subscribe: (listener: () => void) => () => void;
    refresh: () => Promise<void>;
    refreshAutomationStatus: () => Promise<void>;
    refreshNearbyDevices: () => Promise<void>;
    dispose: () => void;
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
    startServer: () => Promise<void>;
    stopServer: () => Promise<void>;
    showPairingInfo: () => Promise<void>;
    pairNearbyDevice: (device: SyncNearbyDevice) => Promise<void>;
    copyPairUri: () => Promise<void>;
    scanPairing: () => Promise<void>;
    connectPairing: () => Promise<void>;
    connectLanAddress: () => Promise<void>;
    renameLocalDevice: () => Promise<void>;
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
        nearbyDevices: [],
        discovering: false,
        discoveryError: '',
        pairingInfo: null,
        datasetCatalog: null,
        syncSelection: null,
        automationConfig: { ...DEFAULT_AUTOMATION_CONFIG },
        automationStatus: { ...DEFAULT_AUTOMATION_STATUS },
        automationDraft: null,
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
    const discovery = createSyncDiscovery(client, setState);

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

    function patchAutomationDraft(patch: Partial<SyncAutomationConfig>): void {
        setState({ automationDraft: { ...state.automationDraft, ...patch } });
    }

    function reportError(error: unknown): void {
        void actions.reportError(error);
    }

    async function withBusy(name: string, task: () => Promise<void>): Promise<void> {
        setState({ busy: name });
        try {
            await task();
        } catch (error) {
            reportError(error);
        } finally {
            if (state.busy === name) {
                setState({ busy: '' });
            }
        }
    }

    function applySnapshot(snapshot: SyncLoadedState): void {
        setState({
            status: snapshot.status,
            pairingInfo: snapshot.status.running ? state.pairingInfo : null,
            devices: snapshot.devices,
            servers: snapshot.servers,
            datasetCatalog: snapshot.datasetCatalog,
            syncSelection: snapshot.syncSelection,
            // Refresh persisted facts; unsaved edits remain in automationDraft.
            automationConfig: snapshot.automationConfig,
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

    async function persistAutomationConfig(config: SyncAutomationConfig): Promise<void> {
        const saved = await client.updateAutomationConfig(config, state.syncSelection);
        setState({ automationConfig: saved });
        await refreshAutomationStatus();
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

        const previous = state.status;
        await withBusy('overwrite-policy', async () => {
            setState({ status: { ...previous, overwritePolicy } });
            try {
                await client.setOverwritePolicy(overwritePolicy);
            } catch (error) {
                if (state.status) {
                    setState({ status: { ...state.status, overwritePolicy: previous.overwritePolicy } });
                }
                throw error;
            }
        });
    }

    async function editSyncScope(): Promise<void> {
        await withBusy('scope', async () => {
            const next = await actions.editSyncScope({
                catalog: state.datasetCatalog,
                selection: state.syncSelection,
            });
            if (next) {
                setState({ syncSelection: next });
                await persistAutomationConfig(state.automationConfig);
            }
        });
    }

    async function saveAutomation(): Promise<void> {
        await withBusy('automation', async () => {
            await persistAutomationConfig({ ...state.automationConfig, ...state.automationDraft });
            setState({ automationDraft: null });
        });
    }

    async function saveAutomationPatch(patch: Partial<SyncAutomationConfig>): Promise<void> {
        await withBusy('automation', async () => {
            const previous = state.automationConfig;
            setState({ automationConfig: { ...previous, ...patch } });
            try {
                await persistAutomationConfig(state.automationConfig);
            } catch (error) {
                setState({ automationConfig: previous });
                throw error;
            }
        });
    }

    async function setLanServerAutoStart(enabled: boolean): Promise<void> {
        await saveAutomationPatch({ lanServerAutoStart: enabled });
    }

    async function setAutoSyncEnabled(enabled: boolean): Promise<void> {
        if (!state.automationConfig.target) {
            // A first-time target and its enable flag are committed together by Save.
            patchAutomationDraft({ autoSyncEnabled: enabled });
            return;
        }
        await saveAutomationPatch({ autoSyncEnabled: enabled });
    }

    function setAutomationInterval(value: string): void {
        patchAutomationDraft({ intervalMinutes: Number(value) });
    }

    function setAutomationMode(value: string): void {
        patchAutomationDraft({ syncMode: value === 'Mirror' ? 'Mirror' : 'Incremental' });
    }

    function setAutomationTarget(value: string): void {
        patchAutomationDraft({ target: parseAutomationTargetValue(value) });
    }

    function showOverwritePolicyHelp(): void {
        void actions.showOverwritePolicyHelp();
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

    async function showPairingInfo(): Promise<void> {
        await withBusy('pairing-info', async () => {
            setState({ pairingInfo: await client.getLanPairingInfo() });
        });
    }

    async function pairNearbyDevice(device: SyncNearbyDevice): Promise<void> {
        await withBusy(`pair:${device.id}`, async () => {
            await client.pairLanDevice(device.id);
        });
        await refresh();
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
        });
        await refresh();
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
        refreshNearbyDevices: discovery.refresh,
        dispose: discovery.dispose,
        changeSyncMode,
        setOverwritePolicy,
        editSyncScope,
        saveAutomation,
        setLanServerAutoStart,
        setAutoSyncEnabled,
        setAutomationInterval,
        setAutomationMode,
        setAutomationTarget,
        showOverwritePolicyHelp,
        startServer,
        stopServer,
        showPairingInfo,
        pairNearbyDevice,
        copyPairUri,
        scanPairing,
        connectPairing,
        connectLanAddress: () => withBusy('manual-connect', async () => {
            await actions.connectLanAddress();
            await refresh();
        }),
        renameLocalDevice: () => withBusy('rename-local', async () => {
            if (state.status && await actions.renameLocalDevice(state.status.deviceName)) await refresh();
        }),
        setRequestPairUri,
        renameTarget,
        pullTarget,
        pushTarget,
        removeTarget,
    };
}
