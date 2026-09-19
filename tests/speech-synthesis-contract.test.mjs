import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { Window } from 'happy-dom';

const script = readFileSync(new URL('../src/tauri/main/compat/android-speech-synthesis.js', import.meta.url), 'utf8');

function createWindow(invoke) {
    const window = new Window({ url: 'http://tauri.localhost/' });
    window.eval(script);
    window.__TAURI__ = { core: { invoke, Channel: class {} } };
    return window;
}

test('Web Speech shares the native queue with same-origin frames and isolates late callbacks after cancellation', async () => {
    const submitted = [];
    const nativeQueue = new Set();
    let events;
    let onSpeak;
    const window = createWindow(async (command, args) => {
        if (command.endsWith('|initialize')) {
            events = args.events;
            return { voices: [{ voiceURI: 'test-voice', name: 'Test voice', lang: 'en-US' }] };
        }
        if (command.endsWith('|speak')) {
            nativeQueue.add(args.id);
            submitted.push(args);
            onSpeak?.(args);
        }
        if (command.endsWith('|cancel')) nativeQueue.clear();
    });
    const synth = window.speechSynthesis;
    const observed = [];
    const utterance = text => {
        const value = new window.SpeechSynthesisUtterance(text);
        value.onstart = () => observed.push([text, 'start']);
        value.onend = () => observed.push([text, 'end']);
        value.addEventListener('error', event => observed.push([text, event.error]));
        return value;
    };
    try {
        const voicesChanged = new Promise(resolve => { synth.onvoiceschanged = resolve; });
        assert.equal(synth.getVoices().length, 0);
        await voicesChanged;
        assert.equal(synth.getVoices().length, 1);

        const iframe = window.document.createElement('iframe');
        window.document.body.append(iframe);
        iframe.contentWindow.eval(script);
        const a = utterance('first');
        const b = new iframe.contentWindow.SpeechSynthesisUtterance('second');
        b.onerror = event => observed.push(['second', event.error]);
        const bothSubmitted = new Promise(resolve => {
            onSpeak = args => { if (args.text === 'second') resolve(); };
        });
        synth.speak(a);
        iframe.contentWindow.speechSynthesis.speak(b);
        await bothSubmitted;
        assert.equal(synth.pending, true);
        events.onmessage({ id: submitted[0].id, type: 'start' });
        assert.equal(synth.speaking, true);

        synth.cancel();
        const replacementSubmitted = new Promise(resolve => { onSpeak = resolve; });
        synth.speak(utterance('replacement'));
        const replacement = await replacementSubmitted;
        assert.deepEqual([...nativeQueue], [replacement.id]);
        events.onmessage({ id: submitted[0].id, type: 'end' });
        events.onmessage({ id: submitted[1].id, type: 'error', error: 'canceled' });
        events.onmessage({ id: replacement.id, type: 'start' });
        events.onmessage({ id: replacement.id, type: 'end' });
        assert.deepEqual(observed, [
            ['first', 'start'], ['first', 'interrupted'], ['second', 'canceled'],
            ['replacement', 'start'], ['replacement', 'end'],
        ]);
        assert.equal(synth.pending, false);
        assert.equal(synth.speaking, false);
    } finally {
        await window.happyDOM.close();
    }
});

test('cancellation during initialization discards the utterance, and an unavailable engine can be retried', async () => {
    const initialization = Promise.withResolvers();
    const initializing = Promise.withResolvers();
    const stopped = Promise.withResolvers();
    const spoken = Promise.withResolvers();
    let attempts = 0;
    const window = createWindow(async (command, args) => {
        if (command.endsWith('|initialize')) {
            if (++attempts === 1) throw { code: 'synthesis-unavailable', message: 'No TTS engine' };
            initializing.resolve();
            return initialization.promise;
        }
        if (command.endsWith('|cancel')) stopped.resolve();
        if (command.endsWith('|speak')) spoken.resolve(args.text);
    });
    try {
        const synth = window.speechSynthesis;
        const failed = new window.SpeechSynthesisUtterance('unavailable');
        const failure = new Promise(resolve => { failed.onerror = event => resolve(event.error); });
        synth.speak(failed);
        assert.equal(await failure, 'synthesis-unavailable');
        const first = new window.SpeechSynthesisUtterance('discarded');
        const cancelled = new Promise(resolve => { first.onerror = event => resolve(event.error); });
        synth.speak(first);
        await initializing.promise;
        synth.cancel();
        initialization.resolve({ voices: [] });
        assert.equal(await cancelled, 'canceled');
        await stopped.promise;
        synth.speak(new window.SpeechSynthesisUtterance('retry'));
        assert.equal(await spoken.promise, 'retry');
    } finally {
        await window.happyDOM.close();
    }
});
