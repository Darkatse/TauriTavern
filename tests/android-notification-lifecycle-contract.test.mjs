import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

import { createGenerationStatusBridge } from '../src/tauri/main/services/ai/generation-status-bridge.js';

test('Android native completion is independent from live progress updates', () => {
    const calls = [];
    const bridge = {
        has(methodName) {
            return ['supportsLiveUpdates', 'supportsNativeCompletion', 'onGenerationFinish'].includes(methodName);
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
    assert.equal(statusBridge.finish({
        success: true,
        statusCode: 0,
        showCompletionNotification: true,
    }), true);
    assert.deepEqual(calls, [
        [
            'onGenerationFinish',
            JSON.stringify({
                success: true,
                status_code: 0,
                show_completion_notification: true,
            }),
        ],
    ]);
});

test('Android completion notification lifecycle stays scoped to the completion slot', async () => {
    const root = new URL('../src-tauri/crates/tauritavern/gen/android/app/src/main/java/com/tauritavern/client/', import.meta.url);
    const service = await readFile(new URL('AiGenerationForegroundService.kt', root), 'utf8');
    const notifier = await readFile(new URL('AndroidAiGenerationNotifier.kt', root), 'utf8');
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
    assert.match(service, /notificationManager\.cancel\(\s*COMPLETION_NOTIFICATION_ID\s*\)[\s\S]*notificationManager\.notify\(\s*COMPLETION_NOTIFICATION_ID/);
    assert.doesNotMatch(completionBuilders, /setOnlyAlertOnce\(true\)/);
    assert.match(resumedMethod, /if \(!value\) \{\s*windowFocused = false\s*\}/);
    assert.match(notifier, /cancel\(AiGenerationForegroundService\.COMPLETION_NOTIFICATION_ID\)/);
    assert.doesNotMatch(notifier, /cancelAll\(/);
    assert.match(activity, /onWindowFocusChanged\(hasFocus: Boolean\)[\s\S]*super\.onWindowFocusChanged\(hasFocus\)[\s\S]*AndroidAppPresence\.setWindowFocused\(hasFocus\)/);
    assert.match(activity, /onResume\(\)[\s\S]*AndroidAppPresence\.setActivityResumed\(true\)/);
    assert.match(activity, /onPause\(\)[\s\S]*AndroidAppPresence\.setActivityResumed\(false\)/);
});
