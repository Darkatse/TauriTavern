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

test('tool-only Assistant messages do not emit legacy character events', async () => {
    const { shouldEmitCharacterMessageEvents } = await import('../src/scripts/util/generation-lifecycle.js');
    const message = (mes = '', extra = {}) => ({ mes, extra });

    assert.equal(shouldEmitCharacterMessageEvents(message(), false), true);
    assert.equal(shouldEmitCharacterMessageEvents(message(), true), false);
    assert.equal(shouldEmitCharacterMessageEvents(message(' ... '), true), false);
    assert.equal(shouldEmitCharacterMessageEvents(message('Calling a tool'), true), true);
    assert.equal(shouldEmitCharacterMessageEvents(message('', { reasoning: 'Thinking' }), true), true);
    assert.equal(shouldEmitCharacterMessageEvents(message('', { media: [{ url: 'result.png' }] }), true), true);
});
