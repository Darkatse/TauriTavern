import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { getStreamingRenderInterval, shouldCommitStreamingMessage } from '../src/scripts/tauri/perf/streaming-render-policy.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('desktop streaming preserves the configured FPS', () => {
    assert.equal(getStreamingRenderInterval({ configuredFps: 30, hidden: false }), 1000 / 30);
    assert.equal(getStreamingRenderInterval({ configuredFps: 5, hidden: false }), 200);
});

test('visible mobile streaming preserves the configured FPS', () => {
    assert.equal(getStreamingRenderInterval({ configuredFps: 30, mobile: true, hidden: false }), 1000 / 30);
    assert.equal(getStreamingRenderInterval({ configuredFps: 5, mobile: true, hidden: false }), 200);
});

test('hidden streaming caps expensive preview renders at 4 FPS', () => {
    assert.equal(getStreamingRenderInterval({ configuredFps: 30, hidden: true }), 250);
    assert.equal(getStreamingRenderInterval({ configuredFps: 2, hidden: true }), 500);
});

test('invalid FPS falls back to a safe one-second interval', () => {
    assert.equal(getStreamingRenderInterval({ configuredFps: 0, hidden: false }), 1000);
    assert.equal(getStreamingRenderInterval({ configuredFps: Number.NaN, hidden: false }), 1000);
});

test('streaming DOM commits skip unchanged HTML but always commit final state', () => {
    assert.equal(shouldCommitStreamingMessage({ currentHtml: '', nextHtml: '', final: false, fadeIn: false }), false);
    assert.equal(shouldCommitStreamingMessage({ currentHtml: '<p>old</p>', nextHtml: '<p>new</p>', final: false, fadeIn: false }), true);
    assert.equal(shouldCommitStreamingMessage({ currentHtml: '<p>same</p>', nextHtml: '<p>same</p>', final: true, fadeIn: false }), true);
    assert.equal(shouldCommitStreamingMessage({ currentHtml: '<p>same</p>', nextHtml: '<p>same</p>', final: false, fadeIn: true }), true);
});

test('ReasoningHandler skips preview no-ops but forces the final reasoning commit', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/reasoning.js'), 'utf8');
    const finishStart = source.indexOf('    async finish(messageId) {');
    const finishEnd = source.indexOf('    /**\n     * Updates the reasoning UI elements', finishStart);
    const updateStart = source.indexOf('    updateDom(messageId, { final = false } = {}) {');
    const updateEnd = source.indexOf('    #checkDomElements(messageId) {', updateStart);
    const finishSource = source.slice(finishStart, finishEnd);
    const updateSource = source.slice(updateStart, updateEnd);

    assert.ok(finishStart >= 0 && finishEnd > finishStart);
    assert.ok(updateStart >= 0 && updateEnd > updateStart);
    assert.match(finishSource, /this\.updateDom\(messageId, \{ final: true \}\);/);
    assert.match(updateSource, /shouldCommitStreamingMessage\(\{[\s\S]*final,[\s\S]*fadeIn: power_user\.stream_fade_in/);
    assert.equal(shouldCommitStreamingMessage({ currentHtml: '<p>same</p>', nextHtml: '<p>same</p>', final: false, fadeIn: false }), false);
    assert.equal(shouldCommitStreamingMessage({ currentHtml: '<p>same</p>', nextHtml: '<p>same</p>', final: true, fadeIn: false }), true);
});
