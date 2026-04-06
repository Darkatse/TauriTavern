import test from 'node:test';
import assert from 'node:assert/strict';

import { createSystemNotificationService } from '../src/tauri/main/services/notifications/system-notification-service.js';

function createMemoryStorage(initialEntries = []) {
    const store = new Map(initialEntries);

    return {
        getItem(key) {
            return store.has(key) ? store.get(key) : null;
        },
        setItem(key, value) {
            store.set(key, String(value));
        },
        removeItem(key) {
            store.delete(key);
        },
    };
}

test('stops showing permission rationale after three rejections', async () => {
    const storage = createMemoryStorage();
    const commands = [];
    let rationaleCalls = 0;

    const service = createSystemNotificationService({
        storage,
        async safeInvoke(command) {
            commands.push(command);
            if (command === 'get_notification_permission_state') {
                return 'prompt';
            }

            throw new Error(`Unexpected command: ${command}`);
        },
        async confirmPermissionRationale() {
            rationaleCalls += 1;
            return false;
        },
    });

    for (let index = 0; index < 4; index += 1) {
        const state = await service.preparePermission();
        assert.equal(state, 'prompt');
    }

    assert.equal(rationaleCalls, 3);
    assert.equal(storage.getItem('tt:notification-permission-rejection-count'), '3');
    assert.deepEqual(commands, [
        'get_notification_permission_state',
        'get_notification_permission_state',
        'get_notification_permission_state',
        'get_notification_permission_state',
    ]);
});

test('shows the original permission rationale popup when state is prompt', async () => {
    const storage = createMemoryStorage();
    const commands = [];
    let rationaleCalls = 0;

    const service = createSystemNotificationService({
        storage,
        async safeInvoke(command) {
            commands.push(command);
            if (command === 'get_notification_permission_state') {
                return 'prompt';
            }

            if (command === 'request_notification_permission') {
                return 'granted';
            }

            throw new Error(`Unexpected command: ${command}`);
        },
        async confirmPermissionRationale() {
            rationaleCalls += 1;
            return true;
        },
    });

    const state = await service.preparePermission();

    assert.equal(state, 'granted');
    assert.equal(rationaleCalls, 1);
    assert.deepEqual(commands, [
        'get_notification_permission_state',
        'request_notification_permission',
    ]);
    assert.equal(storage.getItem('tt:notification-permission-rejection-count'), null);
});

test('still shows the original permission rationale popup before reaching the rejection limit', async () => {
    const storage = createMemoryStorage([
        ['tt:notification-permission-rejection-count', '2'],
    ]);
    const commands = [];
    let rationaleCalls = 0;

    const service = createSystemNotificationService({
        storage,
        async safeInvoke(command) {
            commands.push(command);
            if (command === 'get_notification_permission_state') {
                return 'prompt';
            }

            if (command === 'request_notification_permission') {
                return 'granted';
            }

            throw new Error(`Unexpected command: ${command}`);
        },
        async confirmPermissionRationale() {
            rationaleCalls += 1;
            return true;
        },
    });

    const state = await service.preparePermission();

    assert.equal(state, 'granted');
    assert.equal(rationaleCalls, 1);
    assert.deepEqual(commands, [
        'get_notification_permission_state',
        'request_notification_permission',
    ]);
    assert.equal(storage.getItem('tt:notification-permission-rejection-count'), null);
});

test('does not show the permission rationale popup when permission is no longer prompt', async () => {
    const storage = createMemoryStorage();
    let rationaleCalls = 0;

    const service = createSystemNotificationService({
        storage,
        async safeInvoke(command) {
            if (command === 'get_notification_permission_state') {
                return 'denied';
            }

            throw new Error(`Unexpected command: ${command}`);
        },
        async confirmPermissionRationale() {
            rationaleCalls += 1;
            return true;
        },
    });

    const state = await service.preparePermission();

    assert.equal(state, 'denied');
    assert.equal(rationaleCalls, 0);
});

test('counts denied system permission requests and suppresses future reminders', async () => {
    const storage = createMemoryStorage();
    const commands = [];
    let rationaleCalls = 0;

    const service = createSystemNotificationService({
        storage,
        async safeInvoke(command) {
            commands.push(command);
            if (command === 'get_notification_permission_state') {
                return 'prompt';
            }

            if (command === 'request_notification_permission') {
                return 'prompt';
            }

            throw new Error(`Unexpected command: ${command}`);
        },
        async confirmPermissionRationale() {
            rationaleCalls += 1;
            return true;
        },
    });

    for (let index = 0; index < 4; index += 1) {
        const state = await service.preparePermission();
        assert.equal(state, 'prompt');
    }

    assert.equal(rationaleCalls, 3);
    assert.deepEqual(commands, [
        'get_notification_permission_state',
        'request_notification_permission',
        'get_notification_permission_state',
        'request_notification_permission',
        'get_notification_permission_state',
        'request_notification_permission',
        'get_notification_permission_state',
    ]);
    assert.equal(storage.getItem('tt:notification-permission-rejection-count'), '3');
});

test('resets the rejection counter once notification permission is granted', async () => {
    const storage = createMemoryStorage([
        ['tt:notification-permission-rejection-count', '3'],
    ]);

    const service = createSystemNotificationService({
        storage,
        async safeInvoke(command) {
            if (command === 'get_notification_permission_state') {
                return 'granted';
            }

            throw new Error(`Unexpected command: ${command}`);
        },
        async confirmPermissionRationale() {
            throw new Error('Rationale popup should not be shown when already granted');
        },
    });

    const state = await service.preparePermission();

    assert.equal(state, 'granted');
    assert.equal(storage.getItem('tt:notification-permission-rejection-count'), null);
});

test('deduplicates concurrent rationale prompts and only counts one rejection', async () => {
    const storage = createMemoryStorage();
    let rationaleCalls = 0;
    let releaseRationale;
    let notifyRationaleReady;
    const rationaleReady = new Promise((resolve) => {
        notifyRationaleReady = resolve;
    });

    const service = createSystemNotificationService({
        storage,
        async safeInvoke(command) {
            if (command === 'get_notification_permission_state') {
                return 'prompt';
            }

            throw new Error(`Unexpected command: ${command}`);
        },
        async confirmPermissionRationale() {
            rationaleCalls += 1;
            notifyRationaleReady();
            return new Promise((resolve) => {
                releaseRationale = resolve;
            });
        },
    });

    const firstAttempt = service.preparePermission();
    const secondAttempt = service.preparePermission();
    await rationaleReady;
    releaseRationale(false);

    const [firstState, secondState] = await Promise.all([firstAttempt, secondAttempt]);

    assert.equal(firstState, 'prompt');
    assert.equal(secondState, 'prompt');
    assert.equal(rationaleCalls, 1);
    assert.equal(storage.getItem('tt:notification-permission-rejection-count'), '1');
});
