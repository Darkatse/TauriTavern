import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
    getStreamingRenderInterval,
    normalizeStreamingFps,
    shouldCommitStreamingMessage,
} from '../src/scripts/tauri/perf/streaming-render-policy.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('desktop streaming preserves the configured FPS', () => {
    assert.equal(getStreamingRenderInterval({ configuredFps: 30, hidden: false }), 1000 / 30);
    assert.equal(getStreamingRenderInterval({ configuredFps: 5, hidden: false }), 200);
});

test('hidden streaming caps expensive preview renders at 4 FPS', () => {
    assert.equal(getStreamingRenderInterval({ configuredFps: 30, hidden: true }), 250);
    assert.equal(getStreamingRenderInterval({ configuredFps: 2, hidden: true }), 500);
});

test('invalid FPS warns and falls back to the explicit 30 FPS default', () => {
    const warnings = [];
    const originalWarn = console.warn;
    console.warn = (...args) => warnings.push(args.join(' '));

    try {
        assert.equal(normalizeStreamingFps(0), 30);
        assert.equal(normalizeStreamingFps(Number.NaN), 30);
        assert.equal(normalizeStreamingFps(Number.POSITIVE_INFINITY), 30);
    } finally {
        console.warn = originalWarn;
    }

    assert.equal(warnings.length, 3);
    assert.ok(warnings.every(message => message.includes('30 FPS')));
});

test('valid FPS is normalized without warning', () => {
    const warnings = [];
    const originalWarn = console.warn;
    console.warn = (...args) => warnings.push(args.join(' '));

    try {
        assert.equal(normalizeStreamingFps(30), 30);
        assert.equal(normalizeStreamingFps('5'), 5);
    } finally {
        console.warn = originalWarn;
    }

    assert.deepEqual(warnings, []);
});

test('streaming DOM commits skip unchanged HTML but always commit final state', () => {
    assert.equal(shouldCommitStreamingMessage({ lastCommittedHtml: '', nextHtml: '', final: false, fadeIn: false }), false);
    assert.equal(shouldCommitStreamingMessage({ lastCommittedHtml: '<p>old</p>', nextHtml: '<p>new</p>', final: false, fadeIn: false }), true);
    assert.equal(shouldCommitStreamingMessage({ lastCommittedHtml: '<p>same</p>', nextHtml: '<p>same</p>', final: true, fadeIn: false }), true);
    assert.equal(shouldCommitStreamingMessage({ lastCommittedHtml: '<p>same</p>', nextHtml: '<p>same</p>', final: false, fadeIn: true }), true);
});
