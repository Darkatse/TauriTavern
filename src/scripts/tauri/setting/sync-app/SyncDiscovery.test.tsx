import { act, fireEvent, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test } from '@rstest/core';

import type { SyncNearbyDevice } from './SyncContract';
import { createFakes, handles, mountMain } from './SyncMainTestHarness';

test('nearby updates keep the draft and pair a device by identity', async () => {
    const fakes = createFakes();
    fakes.snapshot.status.running = false;
    const device: SyncNearbyDevice = {
        id: 'nearby-tablet', name: 'My Tablet', platform: 'android', baseUrls: ['https://192.168.1.3:4567'],
    };
    const pairedIds: string[] = [];
    fakes.client.pairLanDevice = (id) => {
        pairedIds.push(id);
        fakes.snapshot.devices.push({
            type: 'lan', platform: 'android', alias: '', id, name: device.name,
            lastKnownAddress: 'https://192.168.1.3:4567', lastSyncMs: null,
        });
        fakes.snapshot.status.running = true;
        return Promise.resolve();
    };
    const { container } = await mountMain(fakes);
    const view = within(container);
    expect(view.queryByText('Online')).toBeNull();
    const interval = view.getByRole<HTMLSelectElement>('combobox', { name: 'Interval' });
    fireEvent.change(interval, { target: { value: '60' } });

    act(() => fakes.publishNearby([
        { id: 'lan-1', name: 'My Phone', platform: 'android', baseUrls: ['https://192.168.1.2:4567'] },
        device,
    ]));
    expect(interval.value).toBe('60');
    expect(fakes.events.filter(event => event === 'loadState')).toHaveLength(1);
    expect(view.getAllByText('My Phone')).toHaveLength(1);
    expect(view.getByText('Online')).toBeTruthy();

    await userEvent.setup().click(view.getByRole('button', { name: 'Pair My Tablet' }));
    await waitFor(() => expect(pairedIds).toEqual(['nearby-tablet']));
    await waitFor(() => expect(view.queryByRole('button', { name: 'Pair My Tablet' })).toBeNull());
    expect(view.getByText('Running')).toBeTruthy();
    expect(view.getAllByText('My Tablet')).toHaveLength(1);
    expect(interval.value).toBe('60');
});

test('discovery failures stay in the nearby section and can be retried', async () => {
    const fakes = createFakes();
    let attempts = 0;
    fakes.client.discoverLanDevices = () => {
        attempts += 1;
        return attempts === 1
            // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- Tauri rejects with a serialized CommandError.
            ? Promise.reject({ InternalServerError: 'Local network permission is unavailable' })
            : Promise.resolve([{ id: 'nearby-1', name: 'Laptop', platform: 'android', baseUrls: ['https://192.168.1.3:4567'] }]);
    };
    const { container } = await mountMain(fakes);
    const view = within(container);

    await waitFor(() => expect(view.getByText('Device discovery failed. Refresh to try again.')).toBeTruthy());
    expect(view.getByText('Local network permission is unavailable')).toBeTruthy();
    expect(fakes.errors).toHaveLength(0);
    expect(view.getByRole<HTMLButtonElement>('button', { name: 'Download (pull from this device)' }).disabled).toBe(false);
    await userEvent.setup().click(view.getByRole('button', { name: 'Refresh devices' }));
    await waitFor(() => expect(view.getByRole('button', { name: 'Pair Laptop' })).toBeTruthy());
    expect(view.queryByText('Device discovery failed. Refresh to try again.')).toBeNull();
    expect(fakes.events.filter(event => event === 'loadState')).toHaveLength(1);
});

test.each(['nearby', 'link'])('a rejected %s pairing still refreshes the started server', async entry => {
    const fakes = createFakes();
    fakes.snapshot.status.running = false;
    const error = new Error('Pairing rejected');
    const rejectAfterStarting = () => {
        fakes.snapshot.status = { ...fakes.snapshot.status, running: true };
        return Promise.reject(error);
    };
    fakes.client.pairLanDevice = rejectAfterStarting;
    fakes.actions.connectPairUri = rejectAfterStarting;
    const { container } = await mountMain(fakes);
    const view = within(container);
    const user = userEvent.setup();
    if (entry === 'nearby') {
        act(() => fakes.publishNearby([{ id: 'nearby-1', name: 'Laptop', platform: '', baseUrls: [] }]));
        await user.click(view.getByRole('button', { name: 'Pair Laptop' }));
    } else {
        const { pairUri } = await fakes.client.getLanPairingInfo();
        await user.click(view.getByText('QR code or pairing link'));
        fireEvent.change(view.getByPlaceholderText('Paste Pair URI here'), { target: { value: pairUri } });
        await user.click(view.getByRole('button', { name: 'Connect' }));
        expect(view.getByPlaceholderText<HTMLTextAreaElement>('Paste Pair URI here').value).toBe(pairUri);
    }
    await waitFor(() => expect(view.getByText('Running')).toBeTruthy());
    expect(fakes.errors).toEqual([error]);
    expect(fakes.events.filter(event => event === 'loadState')).toHaveLength(2);
});

test('live device names and platform icons update without replacing a local alias', async () => {
    const fakes = createFakes();
    fakes.snapshot.servers = [];
    fakes.snapshot.devices.push(...fakes.snapshot.devices.map(device => ({
        ...device, id: 'aliased', alias: 'Family tablet',
    })));
    const { container } = await mountMain(fakes);
    act(() => fakes.publishNearby([
        { id: 'lan-1', name: 'Travel phone', platform: 'android', baseUrls: [] },
        { id: 'aliased', name: 'Changed by owner', platform: 'ios', baseUrls: [] },
    ]));
    const view = within(container);
    expect(view.getByText('Travel phone')).toBeTruthy();
    expect(view.getByText('Family tablet')).toBeTruthy();
    expect(view.getByRole('option', { name: 'LAN · Travel phone' })).toBeTruthy();
    expect(view.getByRole('option', { name: 'LAN · Family tablet' })).toBeTruthy();
    expect(view.queryByText('Changed by owner')).toBeNull();
    expect(view.getByRole('img', { name: 'Android' })).toBeTruthy();
    expect(view.getByRole('img', { name: 'iOS' })).toBeTruthy();
});

test('manual connection and local rename refresh device information after completion', async () => {
    const fakes = createFakes();
    let complete!: (saved: boolean) => void;
    fakes.actions.connectLanAddress = () => new Promise(resolve => { complete = resolve; });
    fakes.actions.renameLocalDevice = name => {
        expect(name).toBe('My Mac');
        fakes.snapshot.status.deviceName = 'Study Mac';
        return Promise.resolve(true);
    };
    const { container } = await mountMain(fakes);
    const view = within(container);
    const user = userEvent.setup();
    await user.click(view.getByRole('button', { name: 'Connect manually' }));
    expect(fakes.events.filter(event => event === 'loadState')).toHaveLength(1);
    expect(view.getByRole<HTMLButtonElement>('button', { name: 'Refresh devices' }).disabled).toBe(true);
    await act(async () => { complete(true); await Promise.resolve(); });
    expect(fakes.events.filter(event => event === 'loadState')).toHaveLength(2);
    await user.click(view.getByText('Sync preferences'));
    await user.click(view.getByRole('button', { name: 'Rename this device' }));
    await waitFor(() => expect(view.getByText('Study Mac')).toBeTruthy());
});

test('a newer discovery event wins over an outstanding snapshot', async () => {
    const fakes = createFakes();
    let finishDiscovery!: (devices: SyncNearbyDevice[]) => void;
    fakes.client.discoverLanDevices = () => {
        fakes.events.push('discoverLanDevices');
        return new Promise(resolve => { finishDiscovery = resolve; });
    };
    const { container } = await mountMain(fakes);
    expect(fakes.events.indexOf('subscribeLanDevices')).toBeLessThan(fakes.events.indexOf('discoverLanDevices'));
    expect(fakes.nearbyListeners.size).toBe(1);

    await act(async () => {
        fakes.publishNearby([{ id: 'nearby-1', name: 'Laptop', platform: 'android', baseUrls: ['https://192.168.1.3:4567'] }]);
        finishDiscovery([]);
        await Promise.resolve();
    });
    expect(within(container).getByRole('button', { name: 'Pair Laptop' })).toBeTruthy();
});

test('a discovery subscription that finishes after unmount is released', async () => {
    const fakes = createFakes();
    let finishSubscription!: (unlisten: () => void) => void;
    let unlistenCount = 0;
    fakes.client.subscribeLanDevices = () => new Promise(resolve => { finishSubscription = resolve; });
    const { handle } = await mountMain(fakes);
    await act(async () => {
        handle.unmount();
        handles.splice(handles.indexOf(handle), 1);
        finishSubscription(() => { unlistenCount += 1; });
        await Promise.resolve();
    });
    expect(unlistenCount).toBe(1);
    expect(fakes.events).not.toContain('discoverLanDevices');
});

test('a paired device without an address remains a download and automation target', async () => {
    const fakes = createFakes();
    fakes.snapshot.status.running = false;
    fakes.snapshot.devices = fakes.snapshot.devices.map(device => ({ ...device, lastKnownAddress: '' }));
    const { container } = await mountMain(fakes);
    const view = within(container);

    const target = view.getByRole<HTMLOptionElement>('option', { name: 'LAN · My Phone' });
    expect(target.disabled).toBe(false);
    await userEvent.setup().click(view.getByRole('button', { name: 'Download (pull from this device)' }));
    await waitFor(() => expect(fakes.events).toContain('pullLanDevice:lan-1:exact:true'));
    expect(view.getByRole<HTMLButtonElement>('button', {
        name: 'Start LAN Sync server first (peer needs to download from you).',
    }).disabled).toBe(true);
});
