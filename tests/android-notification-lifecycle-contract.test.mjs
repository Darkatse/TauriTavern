import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

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

test('mobile background protection never owns generation termination or backend startup', async () => {
    const platform = await readFile(new URL(
        '../src-tauri/crates/tauritavern/src/platform/generation_background.rs',
        import.meta.url,
    ), 'utf8');
    const application = await readFile(new URL(
        '../src-tauri/crates/tt-application/src/services/chat_completion_service/mod.rs',
        import.meta.url,
    ), 'utf8');
    const composition = await readFile(new URL(
        '../src-tauri/crates/tauritavern/src/app/composition/services/mod.rs',
        import.meta.url,
    ), 'utf8');

    assert.match(platform, /endBackgroundTask\(identifier\)/);
    assert.doesNotMatch(platform, /mpsc::sync_channel|receiver\.recv\(\)/);
    assert.doesNotMatch(platform, /expiration\.cancel\(\)/);
    assert.doesNotMatch(application, /background_expiration|background\.expiration\(\)/);
    assert.doesNotMatch(composition, /generation_background::runtime\(app_handle\)\?/);
});

test('Android completion notification lifecycle stays scoped to the completion slot', async () => {
    const root = new URL('../src-tauri/crates/tauritavern/gen/android/app/src/main/java/com/tauritavern/client/', import.meta.url);
    const service = await readFile(new URL('AiGenerationForegroundService.kt', root), 'utf8');
    const notifier = await readFile(new URL('AndroidAiGenerationNotifier.kt', root), 'utf8');
    const plugin = await readFile(new URL('AiGenerationPlugin.kt', root), 'utf8');
    const activity = await readFile(new URL('MainActivity.kt', root), 'utf8');
    const presence = await readFile(new URL('AndroidAppPresence.kt', root), 'utf8');
    const completionBuilderStart = service.indexOf('private fun buildCompletionSuccessNotification');
    const completedProgressStyleStart = service.indexOf('private fun buildCompletedProgressStyle');
    const resumedMethodStart = presence.indexOf('fun setActivityResumed');
    const focusedMethodStart = presence.indexOf('fun setWindowFocused');

    assert.notEqual(completionBuilderStart, -1);
    assert.notEqual(completedProgressStyleStart, -1);
    assert.notEqual(resumedMethodStart, -1);
    assert.notEqual(focusedMethodStart, -1);

    const completionBuilders = service.slice(completionBuilderStart, completedProgressStyleStart);
    const resumedMethod = presence.slice(resumedMethodStart, focusedMethodStart);

    assert.match(service, /AndroidAppPresence\.isForegroundInteractive\(\)[\s\S]*return/);
    assert.match(service, /return START_NOT_STICKY/);
    assert.match(service, /override fun onTimeout\([\s\S]*stopForegroundAndSelf\(startId\)/);
    assert.match(service, /activeTaskIds\.add\(taskId\)/);
    assert.match(service, /activeTaskIds\.remove\(taskId\)/);
    assert.match(service, /notificationManager\.cancel\(\s*COMPLETION_NOTIFICATION_ID\s*\)[\s\S]*notificationManager\.notify\(\s*COMPLETION_NOTIFICATION_ID/);
    assert.doesNotMatch(completionBuilders, /setOnlyAlertOnce\(true\)/);
    assert.match(resumedMethod, /if \(!value\) \{\s*windowFocused = false\s*\}/);
    assert.match(notifier, /cancel\(AiGenerationForegroundService\.COMPLETION_NOTIFICATION_ID\)/);
    assert.match(notifier, /ContextCompat\.startForegroundService\([\s\S]*ACTION_GENERATION_START/);
    assert.match(notifier, /context\.startService\([\s\S]*ACTION_GENERATION_FINISH/);
    assert.doesNotMatch(notifier, /cancelAll\(/);
    assert.match(plugin, /@TauriPlugin[\s\S]*class AiGenerationPlugin/);
    assert.match(plugin, /mainHandler\.post\s*\{[\s\S]*notifier\.onGenerationStart/);
    assert.match(plugin, /mainHandler\.post\s*\{[\s\S]*notifier\.onGenerationFinish/);
    assert.doesNotMatch(activity, /ensureKeepAliveService\(/);
    assert.match(activity, /onWindowFocusChanged\(hasFocus: Boolean\)[\s\S]*super\.onWindowFocusChanged\(hasFocus\)[\s\S]*AndroidAppPresence\.setWindowFocused\(hasFocus\)/);
    assert.match(activity, /onResume\(\)[\s\S]*AndroidAppPresence\.setActivityResumed\(true\)/);
    assert.match(activity, /onPause\(\)[\s\S]*AndroidAppPresence\.setActivityResumed\(false\)/);
});

test('stream cancellation releases native background work before an upstream response', async () => {
    const service = await readFile(new URL(
        '../src-tauri/crates/tt-application/src/services/chat_completion_service/mod.rs',
        import.meta.url,
    ), 'utf8');
    const runStream = service.slice(
        service.indexOf('async fn run_stream'),
        service.indexOf('fn is_quiet_request'),
    );

    assert.match(runStream, /let mut lifecycle_cancel = cancel\.clone\(\)/);
    assert.match(runStream, /lifecycle_cancel\.changed\(\)[\s\S]*generation_task\.abort\(\)[\s\S]*generation_cancelled_by_user/);
});
