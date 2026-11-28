import { expect, rs, test } from '@rstest/core';
import { frontendText } from '../helpers/frontend.mjs';

const script = await frontendText('tauri/main/compat/android-speech-synthesis.js');

// As on Android WebView, the shim installs into a top window without the Web Speech API.
function installShim(invoke) {
    rs.stubGlobal('speechSynthesis', undefined);
    rs.stubGlobal('SpeechSynthesisUtterance', undefined);
    window.eval(script);
    rs.stubGlobal('__TAURI__', { core: { invoke, Channel: class {} } });
}

function installFrameShim(frame) {
    for (const name of ['speechSynthesis', 'SpeechSynthesisUtterance']) {
        Object.defineProperty(frame, name, { configurable: true, writable: true, value: undefined });
    }
    frame.eval(script);
}

test('Web Speech shares the native queue with same-origin frames and isolates late callbacks after cancellation', async () => {
    const submitted = [];
    const nativeQueue = new Set();
    let events;
    let onSpeak;
    installShim(async (command, args) => {
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
    const voicesChanged = new Promise(resolve => { synth.onvoiceschanged = resolve; });
    expect(synth.getVoices().length).toBe(0);
    await voicesChanged;
    expect(synth.getVoices().length).toBe(1);

    const iframe = window.document.createElement('iframe');
    window.document.body.append(iframe);
    installFrameShim(iframe.contentWindow);
    const a = utterance('first');
    const b = new iframe.contentWindow.SpeechSynthesisUtterance('second');
    b.onerror = event => observed.push(['second', event.error]);
    const bothSubmitted = new Promise(resolve => {
        onSpeak = args => { if (args.text === 'second') resolve(); };
    });
    synth.speak(a);
    iframe.contentWindow.speechSynthesis.speak(b);
    await bothSubmitted;
    expect(synth.pending).toBe(true);
    events.onmessage({ id: submitted[0].id, type: 'start' });
    expect(synth.speaking).toBe(true);

    synth.cancel();
    const replacementSubmitted = new Promise(resolve => { onSpeak = resolve; });
    synth.speak(utterance('replacement'));
    const replacement = await replacementSubmitted;
    expect([...nativeQueue]).toEqual([replacement.id]);
    events.onmessage({ id: submitted[0].id, type: 'end' });
    events.onmessage({ id: submitted[1].id, type: 'error', error: 'canceled' });
    events.onmessage({ id: replacement.id, type: 'start' });
    events.onmessage({ id: replacement.id, type: 'end' });
    expect(observed).toEqual([
        ['first', 'start'], ['first', 'interrupted'], ['second', 'canceled'],
        ['replacement', 'start'], ['replacement', 'end'],
    ]);
    expect(synth.pending).toBe(false);
    expect(synth.speaking).toBe(false);
});

test('cancellation during initialization discards the utterance, and an unavailable engine can be retried', async () => {
    const initialization = Promise.withResolvers();
    const initializing = Promise.withResolvers();
    const stopped = Promise.withResolvers();
    const spoken = Promise.withResolvers();
    let attempts = 0;
    installShim(async (command, args) => {
        if (command.endsWith('|initialize')) {
            if (++attempts === 1) throw { code: 'synthesis-unavailable', message: 'No TTS engine' };
            initializing.resolve();
            return initialization.promise;
        }
        if (command.endsWith('|cancel')) stopped.resolve();
        if (command.endsWith('|speak')) spoken.resolve(args.text);
    });
    const synth = window.speechSynthesis;
    const failed = new window.SpeechSynthesisUtterance('unavailable');
    const failure = new Promise(resolve => { failed.onerror = event => resolve(event.error); });
    synth.speak(failed);
    expect(await failure).toBe('synthesis-unavailable');
    const first = new window.SpeechSynthesisUtterance('discarded');
    const cancelled = new Promise(resolve => { first.onerror = event => resolve(event.error); });
    synth.speak(first);
    await initializing.promise;
    synth.cancel();
    initialization.resolve({ voices: [] });
    expect(await cancelled).toBe('canceled');
    await stopped.promise;
    synth.speak(new window.SpeechSynthesisUtterance('retry'));
    expect(await spoken.promise).toBe('retry');
});
