import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');




test('Unhandled error cleanup only targets foreground UI lifecycle leaks', async () => {
    const { shouldUnblockGenerationAfterUnhandledError } = await import('../src/scripts/util/generation-lifecycle.js');

    assert.equal(shouldUnblockGenerationAfterUnhandledError({
        dryRun: true,
        isSendPress: true,
        isBodyGenerating: true,
        isGroupGenerating: false,
    }), false);
    assert.equal(shouldUnblockGenerationAfterUnhandledError({
        dryRun: false,
        isSendPress: true,
        isBodyGenerating: false,
        isGroupGenerating: true,
    }), false);
    assert.equal(shouldUnblockGenerationAfterUnhandledError({
        dryRun: false,
        isSendPress: true,
        isBodyGenerating: false,
        isGroupGenerating: false,
    }), true);
    assert.equal(shouldUnblockGenerationAfterUnhandledError({
        dryRun: false,
        isSendPress: false,
        isBodyGenerating: true,
        isGroupGenerating: false,
    }), true);
    assert.equal(shouldUnblockGenerationAfterUnhandledError({
        dryRun: false,
        isSendPress: false,
        isBodyGenerating: false,
        isGroupGenerating: false,
    }), false);
});
