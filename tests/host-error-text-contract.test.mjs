import test from 'node:test';
import assert from 'node:assert/strict';

import {
    commandErrorStatus,
    extractErrorText,
    getCommandErrorMessage,
    isCommandCancelled,
} from '../src/scripts/util/command-error-utils.js';
import { GITHUB_RATE_LIMIT_MESSAGE, isGitHubRateLimitMessage } from '../src/scripts/util/github-rate-limit.js';
import { createInvokeService } from '../src/tauri/main/services/invokes/invoke-service.js';

globalThis.window ??= {};

function rejectionThroughSafeInvoke(raw) {
    const service = createInvokeService({ invoke: async () => { throw raw; }, policies: {} });
    return service.safeInvoke('get_chat_completions_generate', {}).then(
        () => assert.fail('expected a rejection'),
        error => error,
    );
}

test('a host error has one text, status and cancellation, raw or through safeInvoke', async () => {
    const upstream = {
        code: 'network.timeout',
        category: 'network',
        endpoint: 'https://api.example',
        message_key: 'tauritavern.error.network.timeout',
    };
    for (const [raw, text, status] of [
        [{ NotFound: 'Chat missing' }, 'Not found: Chat missing', 404],
        [{ Forbidden: 'Admin key required' }, 'Forbidden: Admin key required', 403],
        [{ TooManyRequests: 'Slow down' }, 'Too many requests: Slow down', 429],
        [{ Cancelled: 'Generation cancelled by user' }, 'Generation cancelled by user', 500],
        [{ Unsupported: 'Web Host does not support x' }, 'Web Host does not support x', 500],
        [{ UpstreamFailure: upstream }, 'The request timed out before the target service responded. (https://api.example)', 500],
    ]) {
        const error = await rejectionThroughSafeInvoke(raw);
        for (const value of [raw, error]) {
            assert.equal(extractErrorText(value), text);
            assert.equal(commandErrorStatus(value), status);
            assert.equal(isCommandCancelled(value), Object.hasOwn(raw, 'Cancelled'));
        }
    }
});

test('display text drops every kind prefix, including the rate limit', () => {
    assert.equal(getCommandErrorMessage({ InternalServerError: 'Internal error: disk full' }), 'disk full');
    assert.equal(
        getCommandErrorMessage({ Forbidden: 'Permission denied: iOS policy disabled capability' }),
        'iOS policy disabled capability',
    );
    assert.ok(isGitHubRateLimitMessage(getCommandErrorMessage(new Error(`Too many requests: ${GITHUB_RATE_LIMIT_MESSAGE}`))));
});
