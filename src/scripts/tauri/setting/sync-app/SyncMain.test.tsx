import { act, fireEvent, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test } from '@rstest/core';

import { mountTauriTavernSyncApp } from './SyncApp';
import { createFakes, handles, mountMain, tr } from './SyncMainTestHarness';

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
    const user = userEvent.setup();
    await user.click(view.getByText('Connection details'));
    const connectionDetails = container.querySelector<HTMLElement>('.tt-sync-connection-details');
    expect(connectionDetails && within(connectionDetails)
        .getByText('https://127.0.0.1:4567')).toBeTruthy();
    expect(view.getByText('Recommended default (1 / 2)')).toBeTruthy();
    expect(container.querySelector('.tt-sync-automation-head .tt-sync-muted')?.textContent)
        .toMatch(/^Off · Last success: /);
    expect(view.getByText('My Phone')).toBeTruthy();
    expect(view.getByText('Relay')).toBeTruthy();

    // Nothing has been edited yet, so there is nothing to save.
    expect(view.getByRole<HTMLButtonElement>('button', { name: 'Save' }).disabled).toBe(true);
    // Rare flows start folded away from the golden zone.
    expect(container.querySelector<HTMLDetailsElement>('.tt-sync-pairing-fold')?.open).toBe(false);
    expect(container.querySelector<HTMLDetailsElement>('.tt-sync-preferences-fold')?.open).toBe(false);
});

test('sync main keeps at most three device cards visible until expanded', async () => {
    const fakes = createFakes();
    for (const id of ['tt-2', 'tt-3']) {
        fakes.snapshot.servers.push({
            type: 'tt',
            alias: '',
            id,
            name: id,
            baseUrl: `https://${id}.example.com`,
            permissions: { write: true },
            lastSyncMs: null,
        });
    }
    const { container } = await mountMain(fakes);
    const view = within(container);

    expect(container.querySelectorAll('.tt-sync-device-card')).toHaveLength(3);
    const user = userEvent.setup();
    await user.click(view.getByRole('button', { name: 'Show all devices (4)' }));
    expect(container.querySelectorAll('.tt-sync-device-card')).toHaveLength(4);
    await user.click(view.getByRole('button', { name: 'Show fewer devices' }));
    expect(container.querySelectorAll('.tt-sync-device-card')).toHaveLength(3);
});

test('sync main guides the empty state into the pairing fold', async () => {
    const fakes = createFakes();
    fakes.snapshot.devices = [];
    fakes.snapshot.servers = [];
    const { container } = await mountMain(fakes);
    const view = within(container);

    expect(view.getByText('No devices yet')).toBeTruthy();
    const pairingFold = container.querySelector<HTMLDetailsElement>('.tt-sync-pairing-fold');
    expect(pairingFold?.open).toBe(false);

    const user = userEvent.setup();
    await user.click(view.getByRole('button', { name: 'Pair via link or QR code' }));
    expect(pairingFold?.open).toBe(true);
});

test('sync main stages first-time auto sync until a target is selected', async () => {
    const fakes = createFakes();
    const { container } = await mountMain(fakes);
    const view = within(container);
    const user = userEvent.setup();
    const track = container.querySelector<HTMLElement>('.tt-sync-automation-head .tt-sync-switch-track');
    if (!track) {
        throw new Error('Auto sync switch track is missing');
    }
    await user.click(track);
    expect(fakes.automationSaves).toHaveLength(0);
    expect(view.getByRole<HTMLInputElement>('checkbox', {
        name: 'Auto upload while app is running',
    }).checked).toBe(true);

    await user.click(view.getByRole('checkbox', { name: 'Start sync port with app startup' }));
    await waitFor(() => expect(fakes.automationSaves).toHaveLength(1));
    expect(fakes.automationSaves[0]?.config).toMatchObject({
        lanServerAutoStart: true, autoSyncEnabled: false, target: null,
    });

    await user.selectOptions(view.getByRole('combobox', { name: 'Target' }), 'lan:lan-1');
    await user.click(view.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(fakes.automationSaves).toHaveLength(2));
    expect(fakes.automationSaves[1]?.config).toMatchObject({
        lanServerAutoStart: true,
        autoSyncEnabled: true,
        target: { type: 'lan', id: 'lan-1' },
    });

    await user.click(track);
    await waitFor(() => expect(fakes.automationSaves).toHaveLength(3));
    expect(fakes.automationSaves[2]?.config).toMatchObject({ autoSyncEnabled: false });
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

    const user = userEvent.setup();
    const view = within(container);
    await user.click(view.getByRole('checkbox', { name: 'Start sync port with app startup' }));
    await waitFor(() => expect(fakes.automationSaves).toHaveLength(1));
    expect(fakes.automationSaves[0]?.config).toMatchObject({ intervalMinutes: 5, lanServerAutoStart: true });
    expect(interval.value).toBe('60');
    await user.click(view.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(fakes.automationSaves).toHaveLength(2));
    expect(fakes.automationSaves[1]?.config).toMatchObject({ intervalMinutes: 60, lanServerAutoStart: true });
});

test('automation rolls back failed writes but keeps successful saves when status refresh fails', async () => {
    const fakes = createFakes();
    const persist = fakes.client.updateAutomationConfig;
    fakes.client.updateAutomationConfig = () => Promise.reject(new Error('write failed'));
    fakes.client.getAutomationStatus = () => Promise.reject(new Error('status unavailable'));
    const { container } = await mountMain(fakes);
    const view = within(container);
    const port = view.getByRole<HTMLInputElement>('checkbox', { name: 'Start sync port with app startup' });
    const interval = view.getByRole<HTMLSelectElement>('combobox', { name: 'Interval' });
    fireEvent.change(interval, { target: { value: '60' } });
    const user = userEvent.setup();

    await user.click(port);
    await waitFor(() => expect(fakes.errors).toHaveLength(1));
    expect(port.checked).toBe(false);
    expect(fakes.automationSaves).toHaveLength(0);
    expect(interval.value).toBe('60');

    fakes.client.updateAutomationConfig = persist;
    await user.click(port);
    await waitFor(() => expect(fakes.errors).toHaveLength(2));
    expect(port.checked).toBe(true);
    expect(fakes.snapshot.automationConfig.lanServerAutoStart).toBe(true);
    expect(interval.value).toBe('60');
    expect(view.getByRole<HTMLButtonElement>('button', { name: 'Save' }).disabled).toBe(false);
});

test('sync main public refreshAutomationStatus only reloads the automation status', async () => {
    const fakes = createFakes();
    const { container, handle } = await mountMain(fakes);

    const statusLine = container.querySelector('.tt-sync-automation-foot .tt-sync-muted');
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
    const interval = within(container).getByRole<HTMLSelectElement>('combobox', { name: 'Interval' });
    fireEvent.change(interval, { target: { value: '60' } });
    await user.click(within(container).getByRole('button', { name: 'Choose' }));

    await waitFor(() => {
        expect(within(container).getByText('2 / 2 datasets selected')).toBeTruthy();
    });
    await waitFor(() => expect(fakes.automationSaves).toHaveLength(1));
    expect(fakes.automationSaves[0]?.config).toMatchObject({ intervalMinutes: 30 });
    expect(fakes.automationSaves[0]?.selection).toEqual(nextSelection);
    expect(interval.value).toBe('60');
});

test('sync main shows the optional QR link without changing server state', async () => {
    const fakes = createFakes();
    const { container } = await mountMain(fakes);
    const view = within(container);
    const user = userEvent.setup();

    await user.click(view.getByText('QR code or pairing link'));
    await user.click(view.getByRole('button', { name: 'Show QR code' }));

    await waitFor(() => expect(container.querySelector('.tt-sync-qr-wrap img')).toBeTruthy());
    expect(container.querySelector<HTMLTextAreaElement>('.tt-sync-pair-fields textarea')?.value)
        .toBe('tauritavern://lan-sync/pair?v=2&url=https%3A%2F%2F127.0.0.1%3A4567&spki=test-pin');
    expect(view.getByText('Running')).toBeTruthy();
    expect(view.getByRole('button', { name: 'Refresh QR code' })).toBeTruthy();
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
    const upload = () => view.getByRole('button', { name: 'Upload (request device to pull from you)' });
    const user = userEvent.setup();

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
    expect(fakes.nearbyListeners.size).toBe(0);
});
