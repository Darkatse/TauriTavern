import assert from 'node:assert/strict';
import test from 'node:test';

import { jsonResponse } from '../src/tauri/main/http-utils.js';
import { createRouteRegistry } from '../src/tauri/main/router.js';
import { registerExtensionRoutes } from '../src/tauri/main/routes/extensions-routes.js';

function createExtensionRouter(context) {
    const router = createRouteRegistry();
    registerExtensionRoutes(router, context, { jsonResponse });
    return router;
}

function withTemporaryNavigator(value, callback) {
    const original = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
    Object.defineProperty(globalThis, 'navigator', {
        value,
        configurable: true,
    });
    try {
        return callback();
    } finally {
        if (original) {
            Object.defineProperty(globalThis, 'navigator', original);
        } else {
            delete globalThis.navigator;
        }
    }
}

function createIosExtensionRouter(context) {
    return withTemporaryNavigator({
        userAgent: 'iPhone',
        maxTouchPoints: 5,
        platform: 'iPhone',
    }, () => createExtensionRouter(context));
}

function completedExportStatus(result = {}) {
    return {
        kind: 'export',
        state: 'completed',
        result: {
            file_name: 'tauritavern-data.zip',
            archive_path: '/tmp/export-job.zip',
            artifact_state: 'available',
            saved_path: null,
            ...result,
        },
    };
}

test('/api/extensions/data-migration/export/android/save finalizes delivered artifacts', async () => {
    const calls = [];
    const router = createExtensionRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'get_data_archive_job_status') {
                return completedExportStatus();
            }
            if (command === 'finalize_export_data_archive_delivery') {
                return 'cleanup warning';
            }
            throw new Error(`Unexpected command: ${command}`);
        },
        saveAndroidExportArchive: async (archivePath, fileName) => {
            assert.equal(archivePath, '/tmp/export-job.zip');
            assert.equal(fileName, 'tauritavern-data.zip');
            return { savedTarget: 'content://saved-export' };
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/extensions/data-migration/export/android/save',
        body: { job_id: 'job-1' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
        ok: true,
        saved_target: 'content://saved-export',
        cleanup_error: 'cleanup warning',
    });
    assert.deepEqual(calls, [
        {
            command: 'get_data_archive_job_status',
            args: { job_id: 'job-1' },
        },
        {
            command: 'finalize_export_data_archive_delivery',
            args: {
                job_id: 'job-1',
                saved_path: 'content://saved-export',
            },
        },
    ]);
});

test('/api/extensions/data-migration/export/android/save finalizes cancelled destination picks', async () => {
    const calls = [];
    const router = createExtensionRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'get_data_archive_job_status') {
                return completedExportStatus();
            }
            if (command === 'finalize_export_data_archive_delivery') {
                return null;
            }
            throw new Error(`Unexpected command: ${command}`);
        },
        saveAndroidExportArchive: async () => {
            throw new Error('Export archive destination selection cancelled');
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/extensions/data-migration/export/android/save',
        body: { job_id: 'job-1' },
    });

    assert.ok(response);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
        ok: true,
        cancelled: true,
        saved_target: '',
        cleanup_error: null,
    });
    assert.deepEqual(calls, [
        {
            command: 'get_data_archive_job_status',
            args: { job_id: 'job-1' },
        },
        {
            command: 'finalize_export_data_archive_delivery',
            args: {
                job_id: 'job-1',
                saved_path: null,
            },
        },
    ]);
});

test('/api/extensions/data-migration/export/android/save keeps artifacts available on non-cancel save errors', async () => {
    const calls = [];
    const router = createExtensionRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'get_data_archive_job_status') {
                return completedExportStatus();
            }
            throw new Error(`Unexpected command: ${command}`);
        },
        saveAndroidExportArchive: async () => {
            throw new Error('Failed to copy export archive to destination URI');
        },
    });

    await assert.rejects(
        router.handle({
            method: 'POST',
            path: '/api/extensions/data-migration/export/android/save',
            body: { job_id: 'job-1' },
        }),
        /Failed to copy export archive/,
    );
    assert.deepEqual(calls, [
        {
            command: 'get_data_archive_job_status',
            args: { job_id: 'job-1' },
        },
    ]);
});

test('/api/extensions/data-migration/export/android/save rejects disposed artifacts without a 500', async () => {
    const router = createExtensionRouter({
        safeInvoke: async (command) => {
            assert.equal(command, 'get_data_archive_job_status');
            return completedExportStatus({
                artifact_state: 'disposed',
                saved_path: '/Downloads/tauritavern-data.zip',
            });
        },
        saveAndroidExportArchive: async () => {
            throw new Error('disposed artifact should not be saved again');
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/extensions/data-migration/export/android/save',
        body: { job_id: 'job-1' },
    });

    assert.ok(response);
    assert.equal(response.status, 409);
    assert.deepEqual(await response.json(), {
        error: 'Export archive has already been handled',
        saved_target: '/Downloads/tauritavern-data.zip',
    });
});

test('/api/extensions/data-migration/export/android/save rejects missing artifacts without a 500', async () => {
    const router = createExtensionRouter({
        safeInvoke: async (command) => {
            assert.equal(command, 'get_data_archive_job_status');
            return completedExportStatus({ artifact_state: 'missing' });
        },
        saveAndroidExportArchive: async () => {
            throw new Error('missing artifact should not be saved');
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/extensions/data-migration/export/android/save',
        body: { job_id: 'job-1' },
    });

    assert.ok(response);
    assert.equal(response.status, 410);
    assert.deepEqual(await response.json(), {
        error: 'Export archive is missing',
    });
});

test('/api/extensions/data-migration/export/save rejects disposed artifacts before native save', async () => {
    const calls = [];
    const router = createExtensionRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'get_data_archive_job_status') {
                return completedExportStatus({
                    artifact_state: 'disposed',
                    saved_path: '/Downloads/tauritavern-data.zip',
                });
            }
            throw new Error(`Unexpected command: ${command}`);
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/extensions/data-migration/export/save',
        body: { job_id: 'job-1' },
    });

    assert.ok(response);
    assert.equal(response.status, 409);
    assert.deepEqual(await response.json(), {
        error: 'Export archive has already been handled',
        saved_target: '/Downloads/tauritavern-data.zip',
    });
    assert.deepEqual(calls, [
        {
            command: 'get_data_archive_job_status',
            args: { job_id: 'job-1' },
        },
    ]);
});

test('/api/extensions/data-migration/export/ios/share rejects missing artifacts before native share', async () => {
    const calls = [];
    const router = createIosExtensionRouter({
        safeInvoke: async (command, args) => {
            calls.push({ command, args });
            if (command === 'get_data_archive_job_status') {
                return completedExportStatus({ artifact_state: 'missing' });
            }
            throw new Error(`Unexpected command: ${command}`);
        },
    });

    const response = await router.handle({
        method: 'POST',
        path: '/api/extensions/data-migration/export/ios/share',
        body: { job_id: 'job-1' },
    });

    assert.ok(response);
    assert.equal(response.status, 410);
    assert.deepEqual(await response.json(), {
        error: 'Export archive is missing',
    });
    assert.deepEqual(calls, [
        {
            command: 'get_data_archive_job_status',
            args: { job_id: 'job-1' },
        },
    ]);
});

test('/api/extensions/data-migration/export/android/save rejects concurrent saves for one job', async () => {
    let releaseSave;
    let markSaveEntered;
    const saveEntered = new Promise((resolve) => {
        markSaveEntered = resolve;
    });
    const release = new Promise((resolve) => {
        releaseSave = resolve;
    });
    const router = createExtensionRouter({
        safeInvoke: async (command) => {
            if (command === 'get_data_archive_job_status') {
                return completedExportStatus();
            }
            return null;
        },
        saveAndroidExportArchive: async () => {
            markSaveEntered();
            await release;
            return { savedTarget: 'content://saved-export' };
        },
    });

    const first = router.handle({
        method: 'POST',
        path: '/api/extensions/data-migration/export/android/save',
        body: { job_id: 'job-1' },
    });
    await saveEntered;

    const second = await router.handle({
        method: 'POST',
        path: '/api/extensions/data-migration/export/android/save',
        body: { job_id: 'job-1' },
    });

    assert.ok(second);
    assert.equal(second.status, 409);
    assert.deepEqual(await second.json(), {
        error: 'Export archive is already being saved',
    });

    releaseSave();
    const firstResponse = await first;
    assert.ok(firstResponse);
    assert.equal(firstResponse.status, 200);
});
