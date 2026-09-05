import { act, fireEvent, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from '@rstest/core';

import { mountTauriTavernSyncApp } from './SyncApp';
import type {
    SyncActions,
    SyncClient,
    SyncJobReport,
    SyncLoadedState,
    SyncMainHandle,
    SyncMainOptions,
} from './SyncContract';

declare global {
    // The mount under test creates its React root directly instead of going
    // through Testing Library's render(), so act() needs the explicit opt-in.
    var IS_REACT_ACT_ENVIRONMENT: boolean | undefined;
}
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const tr = (key: string) => key;

const handles: SyncMainHandle[] = [];
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
            running: true,
            address: 'http://127.0.0.1:4567',
            availableAddresses: ['http://127.0.0.1:4567'],
            pairingEnabled: false,
            pairingExpiresAtMs: null,
            syncMode: 'Incremental',
            syncModeOverridden: false,
            overwritePolicy: 'exact',
        },
        selectedAddress: 'http://127.0.0.1:4567',
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
            selection: { policy_version: 1, dataset_ids: ['settings.core'] },
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
            id: 'lan-1',
            name: 'My Phone',
            displayName: 'My Phone',
            lastKnownAddress: 'http://192.168.1.2:4567',
            pairedAtMs: null,
            lastSyncMs: null,
        }],
        servers: [{
            type: 'tt',
            id: 'tt-1',
            name: 'Relay',
            displayName: 'Relay',
            baseUrl: 'https://relay.example.com',
            spkiSha256: '',
            permissions: { write: true, mirror_delete: false },
            pairedAtMs: null,
            lastSyncMs: null,
        }],
    };
}

function createFakes() {
    const snapshot = createSnapshot();
    const events: string[] = [];
    const errors: unknown[] = [];
    const reports: SyncJobReport[] = [];
    const automationSaves: Array<{ config: unknown; selection: unknown }> = [];
    let pushReport: SyncJobReport = { result: { status: 'remote_request_accepted' } };

    const client: SyncClient = {
        loadState: () => {
            events.push('loadState');
            return Promise.resolve(snapshot);
        },
        setAdvertiseAddress: () => {
            events.push('setAdvertiseAddress');
        },
        startLanServer: () => Promise.resolve(),
        stopLanServer: () => Promise.resolve(),
        enableLanPairing: () => Promise.resolve(null),
        getLanPairingInfo: () => Promise.resolve(null),
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
            automationSaves.push({ config: { ...config }, selection });
            return Promise.resolve(config);
        },
        getAutomationStatus: () => {
            events.push('getAutomationStatus');
            return Promise.resolve(snapshot.automationStatus);
        },
    };

    const actions: SyncActions = {
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
        setPushReport(report: SyncJobReport) {
            pushReport = report;
        },
    };
}

type Fakes = ReturnType<typeof createFakes>;

async function mountMain(fakes: Fakes, options?: Partial<SyncMainOptions>): Promise<{
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

test('sync main mount validates its boundary arguments', () => {
    const fakes = createFakes();
    expect(() => mountTauriTavernSyncApp(null, { client: fakes.client, actions: fakes.actions, tr }))
        .toThrow('TauriTavern Sync mount element is required');
    expect(() => mountTauriTavernSyncApp(document.createElement('div'), {}))
        .toThrow('TauriTavern Sync translator is required');
    expect(() => mountTauriTavernSyncApp(document.createElement('div'), {
        client: {},
        actions: fakes.actions,
        tr,
    }))
        .toThrow('TauriTavern Sync client method is unavailable: loadState');
    expect(() => mountTauriTavernSyncApp(document.createElement('div'), {
        client: fakes.client,
        actions: {},
        tr,
    }))
        .toThrow('TauriTavern Sync action is unavailable: copyText');
});

test('sync main renders the initial snapshot and loads exactly once', async () => {
    const fakes = createFakes();
    const { container } = await mountMain(fakes);

    // StrictMode renders twice in development; the mount-owned initial load
    // must still happen exactly once.
    expect(fakes.events.filter(event => event === 'loadState')).toHaveLength(1);

    const view = within(container);
    expect(view.getByText('Running')).toBeTruthy();
    expect(view.getByRole<HTMLSelectElement>('combobox', { name: 'Address' }).value)
        .toBe('http://127.0.0.1:4567');
    expect(container.querySelector('.tt-sync-preference-copy .tt-sync-muted')?.textContent)
        .toBe('Recommended default (1 / 2)');
    expect(container.querySelector('.tt-sync-automation-summary-meta small')?.textContent)
        .toMatch(/^Off · Last success: /);
    expect(view.getByText('My Phone')).toBeTruthy();
    expect(view.getByText('Relay')).toBeTruthy();

    // Nothing has been edited yet, so there is nothing to save.
    expect(view.getByRole<HTMLButtonElement>('button', { name: 'Save' }).disabled).toBe(true);
    // Pairing is inactive: the section offers the action, not a dead QR block.
    expect(view.getByRole('button', { name: 'Enable Pairing' })).toBeTruthy();
    expect(view.queryByText('No QR')).toBeNull();
});

test('sync main stages first-time auto sync until a target is selected', async () => {
    const fakes = createFakes();
    const { container } = await mountMain(fakes);
    const view = within(container);
    const user = userEvent.setup();
    const disclosure = container.querySelector<HTMLDetailsElement>('.tt-sync-automation-disclosure');
    const track = container.querySelector<HTMLElement>('.tt-sync-automation-switch-wrap .tt-sync-switch-track');
    expect(disclosure?.open).toBe(false);
    if (!track) {
        throw new Error('Auto sync switch track is missing');
    }
    await user.click(track);
    expect(fakes.automationSaves).toHaveLength(0);
    expect(view.getByRole<HTMLInputElement>('checkbox', {
        name: 'Auto upload while app is running',
    }).checked).toBe(true);
    expect(disclosure?.open).toBe(true);

    await user.selectOptions(view.getByRole('combobox', { name: 'Target' }), 'lan:lan-1');
    await user.click(view.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(fakes.automationSaves).toHaveLength(1));
    expect(fakes.automationSaves[0]?.config).toMatchObject({
        autoSyncEnabled: true,
        target: { type: 'lan', id: 'lan-1' },
    });

    await user.click(track);
    await waitFor(() => expect(fakes.automationSaves).toHaveLength(2));
    expect(fakes.automationSaves[1]?.config).toMatchObject({ autoSyncEnabled: false });
    expect(disclosure?.open).toBe(true);
});

test('sync main public refresh reloads but keeps an unsaved automation draft', async () => {
    const fakes = createFakes();
    const { container, handle } = await mountMain(fakes);

    const interval = within(container).getByRole<HTMLSelectElement>('combobox', { name: 'Interval' });
    fireEvent.change(interval, { target: { value: '60' } });
    expect(interval.value).toBe('60');

    fakes.snapshot.automationConfig = {
        ...fakes.snapshot.automationConfig,
        intervalMinutes: 5,
    };
    await act(async () => {
        await handle.refresh();
    });

    // The dirty draft survives the background refresh.
    expect(interval.value).toBe('60');
    expect(fakes.events.filter(event => event === 'loadState')).toHaveLength(2);
});

test('sync main public refreshAutomationStatus only reloads the automation status', async () => {
    const fakes = createFakes();
    const { container, handle } = await mountMain(fakes);

    const statusLine = container.querySelector('.tt-sync-scope-current .tt-sync-muted');
    expect(statusLine?.textContent).toContain('Last success:');

    fakes.snapshot.automationStatus = {
        ...fakes.snapshot.automationStatus,
        lastSuccessAtMs: 1000,
        lastRequestAcceptedAtMs: 2000,
    };
    await act(async () => {
        await handle.refreshAutomationStatus();
    });

    expect(statusLine?.textContent).toContain('Last request accepted:');
    expect(fakes.events.filter(event => event === 'loadState')).toHaveLength(1);
});

test('sync main rolls back the overwrite policy when persisting fails', async () => {
    const fakes = createFakes();
    let rejectPersist!: (error: Error) => void;
    fakes.client.setOverwritePolicy = () => new Promise((_, reject) => {
        rejectPersist = reject;
    });
    const { container } = await mountMain(fakes);
    const view = within(container);

    const exact = view.getByRole<HTMLInputElement>('radio', { name: 'Initiator wins (default)' });
    const newer = view.getByRole<HTMLInputElement>('radio', { name: 'Newer copy wins' });
    expect(exact.checked).toBe(true);

    const user = userEvent.setup();
    await user.click(newer);
    // Optimistic: the UI flips before the host answers.
    expect(newer.checked).toBe(true);
    expect(exact.checked).toBe(false);

    await act(async () => {
        rejectPersist(new Error('nope'));
        await Promise.resolve();
    });
    await waitFor(() => expect(fakes.errors).toHaveLength(1));
    await waitFor(() => expect(exact.checked).toBe(true));
    expect(newer.checked).toBe(false);
});

test('sync main saves automation drafts with the current selection', async () => {
    const fakes = createFakes();
    const { container } = await mountMain(fakes);
    const view = within(container);

    fireEvent.change(view.getByRole('combobox', { name: 'Interval' }), { target: { value: '60' } });
    fireEvent.change(view.getByRole('combobox', { name: 'Sync mode' }), { target: { value: 'Mirror' } });
    fireEvent.change(view.getByRole('combobox', { name: 'Target' }), { target: { value: 'tt:tt-1' } });

    const save = view.getByRole<HTMLButtonElement>('button', { name: 'Save' });
    expect(save.disabled).toBe(false);
    const user = userEvent.setup();
    await user.click(save);

    await waitFor(() => expect(fakes.automationSaves).toHaveLength(1));
    expect(fakes.automationSaves[0]?.config).toMatchObject({
        intervalMinutes: 60,
        syncMode: 'Mirror',
        target: { type: 'tt', id: 'tt-1' },
    });
    expect(fakes.automationSaves[0]?.selection).toEqual(fakes.snapshot.syncSelection);
    // The save refreshes the automation status afterwards.
    expect(fakes.events.lastIndexOf('getAutomationStatus')).toBeGreaterThan(
        fakes.events.indexOf('updateAutomationConfig'),
    );
    // The draft is clean again, so the affordance goes away.
    await waitFor(() => expect(save.disabled).toBe(true));
});

test('sync main applies a new scope selection to the UI and the automation config', async () => {
    const fakes = createFakes();
    const nextSelection = {
        policy_version: 1,
        dataset_ids: ['settings.core', 'chat.character.history'],
    };
    fakes.actions.editSyncScope = () => Promise.resolve(nextSelection);
    const { container } = await mountMain(fakes);

    const user = userEvent.setup();
    await user.click(within(container).getByRole('button', { name: 'Choose' }));

    await waitFor(() => {
        expect(container.querySelector('.tt-sync-preference-copy .tt-sync-muted')?.textContent)
            .toBe('2 / 2 datasets selected');
    });
    await waitFor(() => expect(fakes.automationSaves).toHaveLength(1));
    expect(fakes.automationSaves[0]?.selection).toEqual(nextSelection);
});

test('sync main enables pairing and reveals the QR affordances', async () => {
    const fakes = createFakes();
    fakes.client.enableLanPairing = () => Promise.resolve({
        address: 'http://127.0.0.1:4567',
        pairUri: 'tauritavern://lan-sync/pair?v=2&token=abc',
        qrSvg: '<svg xmlns="http://www.w3.org/2000/svg"/>',
        expiresAtMs: 1760000000000,
    });
    const { container } = await mountMain(fakes);
    const view = within(container);
    const user = userEvent.setup();

    await user.click(view.getByRole('button', { name: 'Enable Pairing' }));

    await waitFor(() => expect(container.querySelector('.tt-sync-qr-wrap img')).toBeTruthy());
    expect(container.querySelector<HTMLTextAreaElement>('.tt-sync-pair-fields textarea')?.value)
        .toBe('tauritavern://lan-sync/pair?v=2&token=abc');
    expect(view.queryByRole('button', { name: 'Enable Pairing' })).toBeNull();
    expect(view.getByRole('button', { name: 'Regenerate' })).toBeTruthy();
    expect(view.getByRole('button', { name: 'Copy URI' })).toBeTruthy();
});

test('sync main routes pull/push with operation options and report feedback order', async () => {
    const fakes = createFakes();
    // The selected overwrite policy must reach every sync operation unchanged.
    fakes.snapshot.status.overwritePolicy = 'prefer-newer';
    const { container } = await mountMain(fakes);
    const view = within(container);
    const user = userEvent.setup();

    await user.click(view.getByRole('button', { name: 'Download (pull from this device)' }));
    await waitFor(() => expect(fakes.events).toContain('pullLanDevice:lan-1:prefer-newer:true'));
    expect(fakes.events.indexOf('pullLanDevice:lan-1:prefer-newer:true'))
        .toBeLessThan(fakes.events.indexOf('showSyncReportResult'));

    await user.click(view.getByRole('button', { name: 'Upload (push to this server)' }));
    await waitFor(() => expect(fakes.events).toContain('pushTtSyncServer:tt-1:Incremental:prefer-newer'));
});

test('sync main only toasts a LAN push request that was actually accepted', async () => {
    const fakes = createFakes();
    const { container } = await mountMain(fakes);
    const view = within(container);
    const user = userEvent.setup();
    const upload = () => view.getByRole('button', { name: 'Upload (request device to pull from you)' });

    fakes.setPushReport({ result: { status: 'failed' } });
    await user.click(upload());
    await waitFor(() => expect(fakes.events).toContain('showSyncReportResult'));
    expect(fakes.events).not.toContain('notifyLanPushRequested');

    fakes.setPushReport({ result: { status: 'remote_request_accepted' } });
    await user.click(upload());
    await waitFor(() => expect(fakes.events).toContain('notifyLanPushRequested'));
});

test('sync main unmount clears the mount element', async () => {
    const fakes = createFakes();
    const { container, handle } = await mountMain(fakes);
    expect(container.innerHTML).not.toBe('');

    act(() => handle.unmount());
    handles.splice(handles.indexOf(handle), 1);
    expect(container.innerHTML).toBe('');
});
