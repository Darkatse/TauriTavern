// Installed at document start by the Android speech-synthesis plugin.
(() => {
    if (window.speechSynthesis || window.SpeechSynthesisUtterance) return;

    if (window !== window.top) {
        // Same-origin frames share the host's native queue and IPC connection.
        try {
            window.speechSynthesis = window.top.speechSynthesis;
            window.SpeechSynthesisUtterance = window.top.SpeechSynthesisUtterance;
        } catch {
            // Cross-origin frames do not have access to host capabilities.
        }
        return;
    }

    const utterances = new Map();
    let voices = [];
    let initialization;
    let events;
    let commands = Promise.resolve();

    // The standard objects are installed before Tauri's global JS API.
    function invoke(command, args) {
        return window.__TAURI__.core.invoke(`plugin:speech-synthesis|${command}`, args);
    }

    // Serialize IPC submission, not playback: Android owns the speech queue.
    function submit(operation) {
        const result = commands.then(operation);
        commands = result.catch(() => {});
        return result;
    }

    function dispatch(id, type, error, message) {
        const entry = utterances.get(id);
        if (!entry) return;
        if (type === 'start') entry.started = true;
        else utterances.delete(id);
        const event = Object.assign(new Event(type), { utterance: entry.utterance });
        if (error) Object.assign(event, { error, message });
        entry.utterance.dispatchEvent(event);
    }

    function initialize() {
        if (!initialization) {
            events ??= new window.__TAURI__.core.Channel();
            events.onmessage = ({ id, type, error, message }) => dispatch(id, type, error, message);
            initialization = invoke('initialize', { events })
                .then(result => {
                    voices = result.voices;
                    synth.dispatchEvent(new Event('voiceschanged'));
                })
                .catch(error => {
                    initialization = undefined;
                    throw error;
                });
        }
        return initialization;
    }

    class SpeechSynthesisUtterance extends EventTarget {
        constructor(text = '') {
            super();
            this.text = String(text);
            this.lang = '';
            this.voice = null;
            this.rate = 1;
            this.pitch = 1;
            this.volume = 1;
        }
    }

    class SpeechSynthesis extends EventTarget {
        get pending() { return [...utterances.values()].some(entry => !entry.started); }
        get speaking() { return [...utterances.values()].some(entry => entry.started); }
        get paused() { return false; }

        getVoices() {
            void initialize().catch(error => console.warn('System TTS:', error));
            return voices.slice();
        }

        speak(utterance) {
            if (!(utterance instanceof SpeechSynthesisUtterance)) {
                throw new TypeError('Expected a SpeechSynthesisUtterance');
            }
            const id = crypto.randomUUID();
            const args = {
                id,
                text: String(utterance.text),
                lang: utterance.lang || document.documentElement.lang || navigator.language,
                voice: utterance.voice?.voiceURI ?? null,
                rate: utterance.rate,
                pitch: utterance.pitch,
                volume: utterance.volume,
            };
            utterances.set(id, { utterance, started: false });
            void submit(async () => {
                await initialize();
                if (utterances.has(id)) await invoke('speak', args);
            }).catch(error => dispatch(id, 'error', error.code || 'synthesis-unavailable', error.message));
        }

        cancel() {
            const cancelled = [...utterances.values()];
            utterances.clear();
            void submit(() => invoke('cancel'))
                .catch(error => console.warn('Failed to stop system TTS:', error));
            queueMicrotask(() => {
                for (const { utterance, started } of cancelled) {
                    utterance.dispatchEvent(Object.assign(new Event('error'), {
                        utterance, error: started ? 'interrupted' : 'canceled',
                    }));
                }
            });
        }

        pause() { throw new DOMException('System TTS pause is not supported on Android', 'NotSupportedError'); }
        resume() { throw new DOMException('System TTS resume is not supported on Android', 'NotSupportedError'); }
    }

    // Event-handler attributes live on the prototype, like native Web APIs.
    function defineEventHandler(prototype, type) {
        const handlers = new WeakMap();
        Object.defineProperty(prototype, `on${type}`, {
            configurable: true,
            get() { return handlers.get(this) ?? null; },
            set(handler) {
                const previous = handlers.get(this);
                if (previous) this.removeEventListener(type, previous);
                handlers.delete(this);
                if (typeof handler === 'function') {
                    handlers.set(this, handler);
                    this.addEventListener(type, handler);
                }
            },
        });
    }
    for (const type of ['start', 'end', 'error']) {
        defineEventHandler(SpeechSynthesisUtterance.prototype, type);
    }
    defineEventHandler(SpeechSynthesis.prototype, 'voiceschanged');

    const synth = new SpeechSynthesis();
    window.SpeechSynthesisUtterance = SpeechSynthesisUtterance;
    window.speechSynthesis = synth;
    window.addEventListener('pagehide', () => synth.cancel());
})();
