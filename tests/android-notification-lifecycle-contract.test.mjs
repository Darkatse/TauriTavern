import test from 'node:test';
import assert from 'node:assert/strict';

import { createGenerationLifecycleService } from '../src/tauri/main/services/ai/generation-lifecycle-service.js';
import { createGenerationStatusBridge } from '../src/tauri/main/services/ai/generation-status-bridge.js';

test('Android native completion is independent from live progress updates', () => {
    const calls = [];
    const bridge = {
        has(methodName) {
            return ['supportsLiveUpdates', 'supportsNativeCompletion'].includes(methodName);
        },
        get(methodName) {
            if (methodName === 'supportsLiveUpdates') {
                return false;
            }
            if (methodName === 'supportsNativeCompletion') {
                return true;
            }
            throw new Error(`Unexpected get: ${methodName}`);
        },
        call(methodName, ...args) {
            calls.push([methodName, ...args]);
            return true;
        },
    };

    const statusBridge = createGenerationStatusBridge({ bridge });

    assert.equal(statusBridge.supportsProgressUpdates(), false);
    assert.equal(statusBridge.handlesCompletion(), true);
    assert.deepEqual(calls, []);
});
test('completion notifications never delay generation completion', async () => {
    let markNotificationStarted;
    let releaseNotification;
    const notificationStarted = new Promise((resolve) => {
        markNotificationStarted = resolve;
    });
    const notificationBlocked = new Promise((resolve) => {
        releaseNotification = resolve;
    });
    const service = createGenerationLifecycleService({
        notificationService: {
            getPermissionState: async () => 'granted',
            preparePermission: async () => 'granted',
            show: async () => {
                markNotificationStarted();
                await notificationBlocked;
            },
        },
        statusBridge: {
            supportsProgressUpdates: () => false,
            reportProgress: () => false,
            handlesCompletion: () => false,
        },
        shouldNotifyCompletion: () => true,
        getNotificationTexts: () => ({
            successTitle: 'done',
            successBody: 'done',
            failureTitle: 'failed',
            failureBody: 'failed',
        }),
        normalizeFailureNotificationBody: (message) => message,
        estimateTokenCount: () => 0,
        progressThrottleMs: 1,
        progressMinCharsDelta: 1,
    });
    const lifecycle = service.createLifecycle({ quiet: false });
    lifecycle.begin();

    const completion = lifecycle.finish({ success: true });
    let completed = false;
    Promise.resolve(completion).then(() => { completed = true; });

    try {
        await notificationStarted;
        await Promise.resolve();
        assert.equal(completed, true);
    } finally {
        releaseNotification();
    }
});
