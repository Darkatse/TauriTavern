import { getCommandErrorMessage } from '../../../util/command-error-utils.js';
import type { SyncClient, SyncNearbyDevice } from './SyncContract';

export type SyncDiscoveryState = {
    nearbyDevices: SyncNearbyDevice[];
    discovering: boolean;
    discoveryError: string;
};

export function createSyncDiscovery(
    client: Pick<SyncClient, 'discoverLanDevices' | 'subscribeLanDevices'>,
    publish: (patch: Partial<SyncDiscoveryState>) => void,
) {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    let refreshing: Promise<void> | null = null;
    let version = 0;

    async function readSnapshot(): Promise<void> {
        if (!unlisten) {
            const unsubscribe = await client.subscribeLanDevices(devices => {
                version += 1;
                if (!disposed) {
                    publish({ nearbyDevices: devices, discoveryError: '' });
                }
            });
            if (disposed) {
                unsubscribe();
                return;
            }
            unlisten = unsubscribe;
        }
        const beforeRead = version;
        const devices = await client.discoverLanDevices();
        // A live update must not be overwritten by an earlier in-flight read.
        if (!disposed && version === beforeRead) {
            publish({ nearbyDevices: devices });
        }
    }

    function refresh(): Promise<void> {
        if (disposed) {
            return Promise.resolve();
        }
        if (refreshing !== null) {
            return refreshing;
        }
        publish({ discovering: true, discoveryError: '' });
        refreshing = readSnapshot().catch((error: unknown) => {
            if (!disposed) {
                publish({ discoveryError: getCommandErrorMessage(error) || 'Device discovery failed.' });
            }
        }).finally(() => {
            refreshing = null;
            if (!disposed) {
                publish({ discovering: false });
            }
        });
        return refreshing;
    }

    function dispose(): void {
        disposed = true;
        unlisten?.();
        unlisten = null;
    }

    return { refresh, dispose };
}
