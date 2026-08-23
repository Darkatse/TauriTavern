import assert from 'node:assert/strict';
import test from 'node:test';

const importSyncJobEvents = () => import(new URL(
    '../src/scripts/tauri/setting/setting-panel/sync-job-events.js',
    import.meta.url,
).href);
const importSyncState = () => import(new URL(
    '../src/scripts/tauri/setting/setting-panel/sync-state.js',
    import.meta.url,
).href);

function installLocalStorage(initial = {}) {
    const store = new Map(Object.entries(initial));
    globalThis.localStorage = {
        getItem: key => store.get(key) ?? null,
        setItem: (key, value) => store.set(key, String(value)),
        removeItem: key => store.delete(key),
    };
    return globalThis.localStorage;
}

async function withMutedWarnings(task) {
    const warn = console.warn;
    console.warn = () => {};
    try {
        return await task();
    } finally {
        console.warn = warn;
    }
}

test('sync job events expose only user-relevant progress and completion', async () => {
    const { resolveSyncJobEventAction, syncFailureRequiresReload } = await importSyncJobEvents();
    const progress = {
        direction: 'Push',
        phase: 'Uploading',
        files_done: 1,
        files_total: 2,
        bytes_done: 3,
        bytes_total: 4,
        current_path: 'characters/a.png',
    };

    assert.deepEqual(resolveSyncJobEventAction({
        status: 'progress',
        job: {
            endpoint: { type: 'remote_server', server_device_id: 'server-1' },
            origin: { type: 'manual' },
        },
        progress,
    }), {
        type: 'progress',
        title: 'TT-Sync progress',
        payload: progress,
    });

    const remoteRequest = {
        status: 'completed',
        job: {
            endpoint: { type: 'lan_peer', device_id: 'peer-1' },
            intent: 'pull_to_local',
            origin: { type: 'remote_request', peer_id: 'peer-1' },
        },
        result: {
            status: 'completed',
            summary: { files_total: 1, bytes_total: 2, files_deleted: 0 },
        },
    };
    assert.deepEqual(resolveSyncJobEventAction(remoteRequest), {
        type: 'report',
        report: {
            job: remoteRequest.job,
            result: remoteRequest.result,
        },
    });
    assert.deepEqual(resolveSyncJobEventAction({
        ...remoteRequest,
        job: {
            endpoint: { type: 'remote_server', server_device_id: 'server-1' },
            origin: { type: 'scheduled' },
        },
    }), { type: 'ignore' });

    assert.equal(syncFailureRequiresReload({
        status: 'failed',
        failure_kind: 'after_partial_local_mutation',
    }), true);
    assert.equal(syncFailureRequiresReload({
        status: 'failed',
        failure_kind: 'without_local_mutation',
    }), false);
});

test('sync dataset selection migrates once and rejects corrupt current state', async () => {
    const { getSyncDatasetSelection } = await importSyncState();
    const catalog = {
        policyVersion: 1,
        supportedDatasetIds: ['characters', 'chats'],
        defaultDatasetIds: ['characters'],
    };
    const legacy = JSON.stringify({ policy_version: 1, dataset_ids: ['chats'] });
    const storage = installLocalStorage({
        'tauritavern:sync_v2_dataset_selection': legacy,
    });

    assert.deepEqual(await withMutedWarnings(() => getSyncDatasetSelection(catalog)), {
        policy_version: 1,
        dataset_ids: ['chats'],
    });
    assert.equal(storage.getItem('tauritavern:sync_v2_dataset_selection'), null);
    assert.equal(storage.getItem('tauritavern:sync_dataset_selection'), legacy);

    installLocalStorage({
        'tauritavern:sync_dataset_selection': '{bad',
        'tauritavern:sync_v2_dataset_selection': legacy,
    });
    assert.throws(
        () => getSyncDatasetSelection(catalog),
        /Stored sync content selection is invalid/,
    );
});
