import type {
    SyncLanDevice,
    SyncNearbyDevice,
    SyncTarget,
    SyncTtSyncServer,
} from './SyncContract';

/**
 * Pure projection for the Devices section: one ordered card list merging
 * unpaired nearby devices with paired targets (LAN devices and TT-Sync
 * servers). Ordering decides what deserves the golden zone:
 *
 *   1. Unpaired nearby devices, in discovery order — they represent a pending
 *      decision and vanish once the device leaves the network.
 *   2. Paired targets — most recently synced first; never synced last, with
 *      the name as the stable tiebreak. Paired cards stay reachable via their
 *      stored address even when the peer is not currently discovered.
 *
 * The view renders at most `VISIBLE_DEVICE_CARD_LIMIT` cards until the user
 * expands the list. No React, no state, no host access.
 */

export const VISIBLE_DEVICE_CARD_LIMIT = 3;

export type SyncNearbyDeviceCard = {
    kind: 'nearby';
    id: string;
    device: SyncNearbyDevice;
    /** Primary reachability hint shown under the device name. */
    address: string;
};

export type SyncPairedDeviceCard = {
    kind: 'paired';
    id: string;
    target: SyncTarget;
    /** The peer currently announces itself on the LAN (live discovery). */
    online: boolean;
};

export type SyncDeviceCard = SyncNearbyDeviceCard | SyncPairedDeviceCard;

export function deviceCardList(state: {
    devices: SyncLanDevice[];
    servers: SyncTtSyncServer[];
    nearbyDevices: SyncNearbyDevice[];
}): SyncDeviceCard[] {
    const pairedIds = new Set(state.devices.map(device => device.id));
    const nearby: SyncNearbyDeviceCard[] = state.nearbyDevices
        .filter(device => !pairedIds.has(device.id))
        .map(device => ({
            kind: 'nearby',
            id: device.id,
            device,
            address: device.baseUrls[0] || '',
        }));
    const discovered = new Map(state.nearbyDevices.map(device => [device.id, device]));
    const devices = state.devices.map(device => {
        const nearby = discovered.get(device.id);
        return nearby ? {
            ...device,
            name: nearby.name,
            platform: nearby.platform || device.platform,
        } : device;
    });
    const paired: SyncPairedDeviceCard[] = [...devices, ...state.servers]
        .sort(compareTargets)
        .map(target => ({
            kind: 'paired',
            id: target.id,
            target,
            online: target.type === 'lan' && discovered.has(target.id),
        }));
    return [...nearby, ...paired];
}

function compareTargets(a: SyncTarget, b: SyncTarget): number {
    const byRecency = (b.lastSyncMs ?? 0) - (a.lastSyncMs ?? 0);
    return byRecency !== 0 ? byRecency : a.name.localeCompare(b.name);
}
