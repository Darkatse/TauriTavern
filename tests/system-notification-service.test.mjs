import test from 'node:test';
import assert from 'node:assert/strict';

import { createSystemNotificationService } from '../src/tauri/main/services/notifications/system-notification-service.js';

test('declining one rationale for concurrent generations turns notifications off once and suppresses completion alerts', async () => {
    let enabled = true;
    let rationales = 0;
    let offNotices = 0;
    let shownNotifications = 0;
    const settingsWrites = [];
    let releaseRationale;
    let notifyRationaleReady;
    const rationaleReady = new Promise((resolve) => {
        notifyRationaleReady = resolve;
    });

    const service = createSystemNotificationService({
        async safeInvoke(command, args) {
            if (command === 'get_generation_notification_state') {
                return { enabled, backgroundShowsProgressUi: false };
            }
            if (command === 'get_notification_permission_state') {
                return 'prompt';
            }
            if (command === 'update_tauritavern_settings') {
                settingsWrites.push(args.dto);
                enabled = args.dto.generation_notifications_enabled;
                return {};
            }
            if (command === 'show_system_notification') {
                shownNotifications += 1;
                return null;
            }

            throw new Error(`Unexpected command: ${command}`);
        },
        async confirmPermissionRationale() {
            rationales += 1;
            notifyRationaleReady();
            return new Promise((resolve) => {
                releaseRationale = resolve;
            });
        },
        showNotificationsOffNotice() {
            offNotices += 1;
        },
    });

    const firstAttempt = service.prepareForGeneration();
    const secondAttempt = service.prepareForGeneration();
    await rationaleReady;
    releaseRationale(false);

    assert.deepEqual(await Promise.all([firstAttempt, secondAttempt]), [false, false]);

    await service.notifyIfAllowed({ title: 'AI reply is ready', body: 'Tap to return' });

    assert.equal(rationales, 1);
    assert.deepEqual(settingsWrites, [{ generation_notifications_enabled: false }]);
    assert.equal(offNotices, 1);
    assert.equal(shownNotifications, 0);
});
