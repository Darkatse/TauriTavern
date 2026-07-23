import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function readProjectFile(relativePath) {
    return readFile(path.join(REPO_ROOT, relativePath), 'utf8');
}

test('Gemini 3.6 Flash is selectable in both direct Google providers', async () => {
    const [indexHtml, openaiSource] = await Promise.all([
        readProjectFile('src/index.html'),
        readProjectFile('src/scripts/openai.js'),
    ]);

    assert.equal(indexHtml.match(/<option value="gemini-3\.6-flash">gemini-3\.6-flash<\/option>/g)?.length, 2);
    assert.match(indexHtml, /<div id="continue_prefill_block" class="range-block">/);
    assert.match(openaiSource, /\$\('#continue_prefill_block'\)\.toggle\(!isDirectGeminiSource\(\) \|\| !\['gemini-3\.5-flash-lite', 'gemini-3\.6-flash'\]\.includes\(model\)\)/);
});

test('Gemini Smooth Streaming preserves final signatures and text part boundaries', async () => {
    const [sseSource, openaiSource] = await Promise.all([
        readProjectFile('src/scripts/sse-stream.js'),
        readProjectFile('src/scripts/openai.js'),
    ]);
    const executableSource = sseSource
        .replace(/^import .*;$/gm, '')
        .replace(/^export default .*;$/gm, '')
        .replace(/\bexport (?=(?:class|function)\b)/g, '');
    const context = vm.createContext({
        Boolean,
        JSON,
        MessageEvent,
        TextDecoderStream,
        TransformStream,
        console,
        document: { hasFocus: () => false },
        power_user: { smooth_streaming_no_think: false, smooth_streaming_speed: 50 },
        structuredClone,
        delay: async () => {},
    });
    vm.runInContext(`${executableSource}\nglobalThis.SmoothEventSourceStream = SmoothEventSourceStream;`, context);

    const streamParts = async (parts) => {
        const stream = new context.SmoothEventSourceStream();
        const eventsPromise = (async () => {
            const reader = stream.readable.getReader();
            const events = [];
            for (;;) {
                const { done, value } = await reader.read();
                if (done) {
                    return events;
                }
                events.push(JSON.parse(value.data));
            }
        })();
        const writer = stream.writable.getWriter();
        const payload = { candidates: [{ index: 0, content: { role: 'model', parts } }] };
        await writer.write(new TextEncoder().encode(`data: ${JSON.stringify(payload)}\n\n`));
        await writer.close();
        return eventsPromise;
    };

    const signatureOnly = await streamParts([{ text: '', thoughtSignature: 'sig' }]);
    assert.equal(signatureOnly.length, 1);
    assert.deepEqual(signatureOnly[0].candidates[0].content.parts[0], { text: '', thoughtSignature: 'sig' });

    const events = await streamParts([
        { text: 'A' },
        { text: 'B' },
        { text: '', thoughtSignature: 'sig' },
    ]);
    const emittedParts = events.map(event => event.candidates[0].content.parts[0]);
    assert.equal(emittedParts.map(part => part.text).join(''), 'A\n\nB');
    assert.equal(emittedParts.filter(part => part.thoughtSignature === 'sig').length, 1);

    const geminiStreamBranch = openaiSource.match(/else if \(\[chat_completion_sources\.MAKERSUITE, chat_completion_sources\.VERTEXAI\][\s\S]*?else if \(chat_completion_source === chat_completion_sources\.COHERE\)/)?.[0] ?? '';
    assert.match(geminiStreamBranch, /parts\.filter\(x => !x\.thought && x\.text\)\.map\(x => x\.text\)\.join\('\\n\\n'\)/);
});
