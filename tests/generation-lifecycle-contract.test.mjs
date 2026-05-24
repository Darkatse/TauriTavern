import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('Generate wrapper uses an in-flight lifecycle gate around GenerateInternal()', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    assert.match(source, /const generationIdleGate = createGenerationIdleGate\(\);/);
    assert.match(source, /let generationInFlightCount = 0;/);
    assert.match(source, /export function waitForGenerationIdle\(\)\s*{\s*return generationIdleGate\.wait\(\);\s*}/s);
    assert.match(source, /function enterGeneration\(\)\s*{\s*if \(generationInFlightCount === 0\) {\s*generationIdleGate\.markBusy\(\);\s*}\s*generationInFlightCount \+= 1;\s*}/s);
    assert.match(source, /function exitGeneration\(\)\s*{\s*if \(generationInFlightCount <= 0\) {\s*throw new Error\('Generation in-flight counter underflow'\);\s*}\s*generationInFlightCount -= 1;\s*if \(generationInFlightCount === 0\) {\s*generationIdleGate\.markIdle\(\);\s*}\s*}/s);
    assert.match(source, /export async function Generate\(type, options = \{\}, dryRun = false\)\s*{\s*enterGeneration\(\);\s*try {\s*return await GenerateInternal\(type, options, dryRun\);\s*} catch \(error\) {\s*cleanupGenerationAfterUnhandledError\(type, dryRun\);\s*throw error;\s*} finally {\s*exitGeneration\(\);\s*}\s*}/s);
    assert.match(source, /async function GenerateInternal\(/);
});

test('Unhandled foreground Generate errors reuse the legacy unblock path', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    assert.match(source, /function cleanupGenerationAfterUnhandledError\(type, dryRun\)\s*{/);
    assert.match(source, /shouldUnblockGenerationAfterUnhandledError\(\{\s*dryRun,\s*isSendPress: is_send_press,\s*isBodyGenerating: document\.body\.dataset\.generating === 'true',\s*isGroupGenerating: is_group_generating,\s*}\)/s);
    assert.match(source, /cleanupGenerationAfterUnhandledError[\s\S]*unblockGeneration\(type\);/);
});

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
