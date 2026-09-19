import { expect, test } from '@rstest/core';

import { deviceCardList } from './SyncDevices';
import type {
    SyncLanDevice,
    SyncNearbyDevice,
    SyncTtSyncServer,
} from './SyncContract';

function lanDevice(id: string, name: string, lastSyncMs: number | null = null): SyncLanDevice {
    return {
        type: 'lan',
        platform: '',
        alias: '',
        id,
        name,
        lastKnownAddress: `https://192.168.1.10:4567`,
        lastSyncMs,
    };
}

function ttServer(id: string, name: string, lastSyncMs: number | null = null): SyncTtSyncServer {
    return {
        type: 'tt',
        id,
        name,
        alias: '',
        baseUrl: `https://${id}.example.com`,
        permissions: {},
        lastSyncMs,
    };
}

function nearby(id: string, name: string): SyncNearbyDevice {
    return { id, name, baseUrls: [], platform: '' };
}

test('device cards put unpaired nearby devices first, then paired targets by recency', () => {
    const cards = deviceCardList({
        devices: [
            lanDevice('lan-old', 'Old phone', 1000),
            lanDevice('lan-never-b', 'Beta'),
            lanDevice('lan-never-a', 'Alpha'),
            lanDevice('lan-new', 'Desktop', 2000),
        ],
        servers: [ttServer('tt-1', 'Relay', 3000)],
        nearbyDevices: [nearby('nb-1', 'Nearby one'), nearby('nb-2', 'Nearby two')],
    });

    expect(cards.map(card => card.id)).toEqual([
        // Unpaired nearby devices keep discovery order and lead the list.
        'nb-1',
        'nb-2',
        'tt-1',
        'lan-new',
        'lan-old',
        // Never synced: recency ties at 0, the name breaks the tie.
        'lan-never-a',
        'lan-never-b',
    ]);
});
