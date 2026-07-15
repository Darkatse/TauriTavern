import test from 'node:test';
import assert from 'node:assert/strict';

import { getStreamingRenderInterval, shouldCommitStreamingMessage } from '../src/scripts/tauri/perf/streaming-render-policy.js';

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
