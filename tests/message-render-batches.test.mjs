import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
    getMessageRenderBatches,
} from '../src/scripts/tauri/perf/message-render-batches.js';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('message prepend rendering is split into stable contiguous batches', () => {
    assert.deepEqual(getMessageRenderBatches(12, 5), [
        { start: 0, end: 5 },
        { start: 5, end: 10 },
        { start: 10, end: 12 },
    ]);
    assert.deepEqual(getMessageRenderBatches(0), []);
});

test('message insertion and generation delegate scrolling to one controller', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/script.js'), 'utf8');

    assert.match(source, /const chatScrollController = createChatScrollController\(/);
    assert.match(source, /const chatScrollIntent = createChatScrollIntentTracker\(\)/);
    assert.match(source, /const chatProgrammaticScroll = createChatProgrammaticScrollTracker\(/);
    assert.match(source, /function setChatElementScrollTop\(position\) \{\s*chatElement\.scrollTop\(position\);\s*chatProgrammaticScroll\.mark\(chatElement\[0\]\.scrollTop\);\s*\}/);
    assert.match(source, /chatScrollController\.captureGenerationIntent\(\);\s*hideSwipeButtons\(\)/);
    assert.match(source, /finally \{\s*chatScrollController\.clearGenerationIntent\(\);\s*showSwipeButtons\(\)/);
    assert.match(source, /async function GenerateInternal\([^]*chatScrollController\.beginGeneration\(\)/);
    assert.match(source, /finally \{\s*chatScrollController\.endGeneration\(\)/);
    assert.match(source, /const programmatic = chatProgrammaticScroll\.consumeIfMatches\(chatElementScroll\.scrollTop\)/);
    assert.match(source, /const userInitiated = !programmatic && chatScrollIntent\.isActive\(\)/);
    assert.match(source, /chatScrollController\.onViewportChanged\(\{ userInitiated \}\)/);
    assert.match(source, /if \(scrollIsAtBottom\) \{\s*chatScrollIntent\.clear\(\);\s*\}/);
    assert.match(source, /chatElementScroll\.addEventListener\('wheel', markChatScrollIntent/);
    assert.match(source, /chatElementScroll\.addEventListener\('touchmove', markChatScrollIntent/);
    assert.match(source, /chatElementScroll\.addEventListener\('pointermove', event =>/);
    assert.match(source, /chatScrollController\.requestScroll\(\{ waitForFrame, force \}\)/);
    assert.match(source, /let position = chatElement\[0\]\.scrollHeight;\s*if \(power_user\.waifuMode\) \{\s*const lastMessage = chatElement\.find\('\.mes'\)\.last\(\);/);
    assert.match(source, /setChatElementScrollTop\(position\);/);
    assert.match(source, /setChatElementScrollTop\(newHeight - prevHeight\);/);
    assert.match(source, /setChatElementScrollTop\(scrollPosition\);/);
    assert.match(source, /setChatElementScrollTop\(scrollPosition \+ diff\);/);
    assert.match(source, /const shouldScroll = scroll;/);
    assert.match(source, /if \(!insertAfter && !insertBefore && shouldScroll\) \{\s*scrollChatToBottom\(\{ waitForFrame: true, force: true \}\)/);
    assert.match(source, /mediaScrollBehavior = chatScrollController\.shouldFollowOutput\(\)/);
    assert.match(source, /if \(!chatScrollController\.shouldFollowOutput\(\)\) \{\s*return;/);
    assert.match(source, /if \(this\.type == 'impersonate'\) \{\s*scrollChatToBottom\(\{ waitForFrame: true \}\);\s*\}/);
    assert.doesNotMatch(source, /followStreamingOutput/);
    assert.doesNotMatch(source, /scrollViewportToBottom/);
    assert.doesNotMatch(source, /let programmaticChatScroll = false/);
    assert.doesNotMatch(source, /\bscrollLock\b/);
});
