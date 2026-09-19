import { act, waitFor, within } from '@testing-library/react';
import { afterEach, expect } from '@rstest/core';

import { mountTauriTavernSyncApp } from './SyncApp';
import type {
    SyncActions,
    SyncClient,
    SyncJobReport,
    SyncLoadedState,
    SyncMainHandle,
    SyncMainOptions,
    SyncNearbyDevice,
} from './SyncContract';

declare global {
    // The mount under test creates its React root directly instead of going
    // through Testing Library's render(), so act() needs the explicit opt-in.
    var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

export const tr = (key: string) => key;

export const handles: SyncMainHandle[] = [];
const containers: HTMLElement[] = [];

afterEach(() => {
    for (const handle of handles.splice(0)) {
        act(() => handle.unmount());
    }
    for (const container of containers.splice(0)) {
        container.remove();
    }
});

function createSnapshot(): SyncLoadedState {
    return {
        status: {
            deviceName: 'My Mac',
            running: true,
            address: 'https://127.0.0.1:4567',
            availableAddresses: ['https://127.0.0.1:4567'],
            syncMode: 'Incremental',
            syncModeOverridden: false,
            overwritePolicy: 'exact',
        },
        datasetCatalog: {
            policyVersion: 1,
            supportedDatasetIds: ['settings.core', 'chat.character.history'],
            defaultDatasetIds: ['settings.core'],
        },
        syncSelection: { policy_version: 1, dataset_ids: ['settings.core'] },
        automationConfig: {
            lanServerAutoStart: false,
            autoSyncEnabled: false,
            intervalMinutes: 30,
            target: null,
            syncMode: 'Incremental',
        },
        automationStatus: {
            running: false,
            nextRunAtMs: null,
            lastAttemptAtMs: null,
            lastSuccessAtMs: 2000,
            lastRequestAcceptedAtMs: 1000,
            lastErrorAtMs: null,
            lastError: '',
        },
        devices: [{
            type: 'lan',
            platform: 'ios',
            alias: '',
            id: 'lan-1',
            name: 'My Phone',
            lastKnownAddress: 'https://192.168.1.2:4567',
            lastSyncMs: null,
        }],
        servers: [{
            type: 'tt',
            id: 'tt-1',
            name: 'Relay',
            alias: '',
            baseUrl: 'https://relay.example.com',
            permissions: { write: true, mirror_delete: false },
            lastSyncMs: null,
        }],
    };
}

export function createFakes() {
    const snapshot = createSnapshot();
    const events: string[] = [];
    const errors: unknown[] = [];
    const reports: SyncJobReport[] = [];
    const automationSaves: Array<{ config: unknown; selection: unknown }> = [];
    const nearbyListeners = new Set<(devices: SyncNearbyDevice[]) => void>();
    let pushReport: SyncJobReport = { result: { status: 'remote_request_accepted' } };

    const client: SyncClient = {
        loadState: () => {
            events.push('loadState');
            return Promise.resolve(structuredClone(snapshot));
        },
        startLanServer: () => Promise.resolve(),
        stopLanServer: () => Promise.resolve(),
        getLanPairingInfo: () => Promise.resolve({
            pairUri: 'tauritavern://lan-sync/pair?v=2&url=https%3A%2F%2F127.0.0.1%3A4567&spki=test-pin',
            qrSvg: '<svg xmlns="http://www.w3.org/2000/svg"/>',
        }),
        discoverLanDevices: () => {
            events.push('discoverLanDevices');
            return Promise.resolve([]);
        },
        subscribeLanDevices: (listener) => {
            events.push('subscribeLanDevices');
            nearbyListeners.add(listener);
            return Promise.resolve(() => { nearbyListeners.delete(listener); });
        },
        pairLanDevice: (id) => {
            events.push(`pairLanDevice:${id}`);
            return Promise.resolve();
        },
        removeLanDevice: () => Promise.resolve(),
        pullLanDevice: (id, options) => {
            events.push(`pullLanDevice:${id}:${options.overwrite_policy}:${options.require_bundle_zstd}`);
            return Promise.resolve({ result: { status: 'completed' } });
        },
        pushLanDevice: (id, options) => {
            events.push(`pushLanDevice:${id}:${options.overwrite_policy}:${options.require_bundle_zstd}`);
            return Promise.resolve(pushReport);
        },
        setOverwritePolicy: () => {
            events.push('setOverwritePolicy');
            return Promise.resolve();
        },
        removeTtSyncServer: () => Promise.resolve(),
        pullTtSyncServer: (id, mode, options) => {
            events.push(`pullTtSyncServer:${id}:${mode}:${options.overwrite_policy}`);
            return Promise.resolve({ result: { status: 'completed' } });
        },
        pushTtSyncServer: (id, mode, options) => {
            events.push(`pushTtSyncServer:${id}:${mode}:${options.overwrite_policy}`);
            return Promise.resolve({ result: { status: 'completed' } });
        },
        updateAutomationConfig: (config, selection) => {
            events.push('updateAutomationConfig');
            automationSaves.push(structuredClone({ config, selection }));
            snapshot.automationConfig = structuredClone(config);
            return Promise.resolve(structuredClone(snapshot.automationConfig));
        },
        getAutomationStatus: () => {
            events.push('getAutomationStatus');
            return Promise.resolve(structuredClone(snapshot.automationStatus));
        },
    };

    const actions: SyncActions = {
        connectLanAddress: () => Promise.resolve(false),
        renameLocalDevice: () => Promise.resolve(false),
        copyText: () => Promise.resolve(),
        scanPairUri: () => Promise.resolve(null),
        changeSyncMode: () => Promise.resolve(false),
        editSyncScope: () => Promise.resolve(null),
        showOverwritePolicyHelp: () => Promise.resolve(),
        renameTarget: () => Promise.resolve(false),
        connectPairUri: () => Promise.resolve(false),
        notifyLanPushRequested: () => {
            events.push('notifyLanPushRequested');
        },
        reportError: (error) => {
            errors.push(error);
        },
        showSyncReportResult: (report) => {
            events.push('showSyncReportResult');
            reports.push(report);
            return Promise.resolve();
        },
    };

    return {
        snapshot,
        events,
        errors,
        reports,
        automationSaves,
        client,
        actions,
        nearbyListeners,
        publishNearby(devices: SyncNearbyDevice[]) {
            for (const listener of nearbyListeners) {
                listener(devices);
            }
        },
        setPushReport(report: SyncJobReport) {
            pushReport = report;
        },
    };
}

type Fakes = ReturnType<typeof createFakes>;

export async function mountMain(fakes: Fakes, options?: Partial<SyncMainOptions>): Promise<{
    container: HTMLElement;
    handle: SyncMainHandle;
}> {
    const container = document.createElement('div');
    document.body.append(container);
    containers.push(container);

    let handle!: SyncMainHandle;
    await act(async () => {
        handle = mountTauriTavernSyncApp(container, {
            client: fakes.client,
            actions: fakes.actions,
            tr,
            ...options,
        });
        // Flush the mount-owned initial refresh's first microtask turn.
        await Promise.resolve();
    });
    handles.push(handle);

    // Settle the mount-owned initial refresh before assertions.
    await waitFor(() => expect(fakes.events).toContain('loadState'));
    await waitFor(() => {
        expect(within(container).queryByText('Running') ?? within(container).queryByText('Stopped')).toBeTruthy();
    });
    return { container, handle };
}
