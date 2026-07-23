import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
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
    assert.match(finishSource, /if \(this\.state !== ReasoningState\.None\) \{[\s\S]*?\n        \}\n\n        this\.updateDom\(messageId, \{ final: true \}\);/);
    assert.match(updateSource, /lastCommittedHtml: this\.lastCommittedHtml/);
    assert.match(updateSource, /this\.lastCommittedHtml = displayReasoning;/);
    assert.doesNotMatch(updateSource, /currentHtml|\.innerHTML,/);
    assert.equal(shouldCommitStreamingMessage({ lastCommittedHtml: '<p>same</p>', nextHtml: '<p>same</p>', final: false, fadeIn: false }), false);
    assert.equal(shouldCommitStreamingMessage({ lastCommittedHtml: '<p>same</p>', nextHtml: '<p>same</p>', final: true, fadeIn: false }), true);
});

test('StreamingProcessor owns canonical HTML and refreshes hidden interval before every tick', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');
    const classStart = source.indexOf('class StreamingProcessor {');
    const classEnd = source.indexOf('\n/**\n * Constructs a prompt', classStart);
    const classSource = source.slice(classStart, classEnd);
    const generateStart = classSource.indexOf('    async generate() {');
    const generateSource = classSource.slice(generateStart);
    const normalizeMatches = generateSource.match(/normalizeStreamingFps\(power_user\.streaming_fps\)/g) ?? [];
    const streamLoop = generateSource.indexOf('for await (const { text, swipes, logprobs, toolCalls, state } of this.generator())');
    const intervalUpdate = generateSource.indexOf('sw.interval = getStreamingRenderInterval({');
    const tick = generateSource.indexOf('await sw.tick(');

    assert.ok(classStart >= 0 && classEnd > classStart);
    assert.match(classSource, /this\.lastCommittedHtml = null;/);
    assert.match(classSource, /lastCommittedHtml: this\.lastCommittedHtml/);
    assert.match(classSource, /this\.lastCommittedHtml = formattedText;/);
    assert.match(classSource, /replaceTransientMesTextHtmlWithRuntimePolicy\(this\.messageDom, formattedText/);
    assert.match(classSource, /replaceMesTextHtmlWithRuntimePolicy\(this\.messageDom, formattedText\)/);
    assert.doesNotMatch(classSource, /this\.messageTextDom\.innerHTML|applyStreamFadeIn/);
    assert.match(classSource, /if \(this\.type !== 'impersonate'\) \{\s*await this\.reasoningHandler\.finish\(messageId\);\s*\}/);
    assert.match(generateSource, /const streamingFps = normalizeStreamingFps\(power_user\.streaming_fps\);/);
    assert.equal(normalizeMatches.length, 1);
    assert.ok(streamLoop > generateSource.indexOf('normalizeStreamingFps(power_user.streaming_fps)'));
    assert.ok(intervalUpdate >= 0 && tick > intervalUpdate);
    assert.match(generateSource.slice(intervalUpdate, tick), /hidden: document\.hidden/);
    assert.doesNotMatch(generateSource, /mobile:/);
});
