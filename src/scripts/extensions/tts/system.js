import { isMobile } from '../../RossAscends-mods.js';
import { getPreviewString } from './index.js';
import { saveTtsProviderSettings } from './index.js';
export { SystemTtsProvider };
import { t } from '../../i18n.js';

const SPEECH_SYNTHESIS_UNSUPPORTED_MESSAGE = 'Speech synthesis API is not supported in this WebView';

function getSpeechSynthesisApi() {
    const synth = window.speechSynthesis;
    const Utterance = window.SpeechSynthesisUtterance;

    if (
        typeof synth?.speak !== 'function' ||
        typeof synth?.getVoices !== 'function' ||
        typeof synth?.cancel !== 'function' ||
        typeof Utterance !== 'function'
    ) {
        return null;
    }

    return { synth, Utterance };
}

function createSpeechSynthesisUnsupportedError() {
    const error = new Error(SPEECH_SYNTHESIS_UNSUPPORTED_MESSAGE);
    error.severity = 'warning';
    return error;
}

function getRequiredSpeechSynthesisApi() {
    const api = getSpeechSynthesisApi();
    if (!api) {
        throw createSpeechSynthesisUnsupportedError();
    }

    return api;
}

/**
 * Chunkify
 * Google Chrome Speech Synthesis Chunking Pattern
 * Fixes inconsistencies with speaking long texts in speechUtterance objects
 * Licensed under the MIT License
 *
 * Peter Woolley and Brett Zamir
 * Modified by Haaris for bug fixes
 */

var speechUtteranceChunker = function (utt, settings, callback) {
    settings = settings || {};
    var newUtt;
    var txt = (settings && settings.offset !== undefined ? utt.text.substring(settings.offset) : utt.text);
    if (utt.voice && utt.voice.voiceURI === 'native') { // Not part of the spec
        newUtt = utt;
        newUtt.text = txt;
        newUtt.addEventListener('end', function () {
            if (speechUtteranceChunker.cancel) {
                speechUtteranceChunker.cancel = false;
            }
            if (callback !== undefined) {
                callback();
            }
        });
    }
    else {
        var chunkLength = (settings && settings.chunkLength) || 160;
        var pattRegex = new RegExp('^[\\s\\S]{' + Math.floor(chunkLength / 2) + ',' + chunkLength + '}[.!?,]{1}|^[\\s\\S]{1,' + chunkLength + '}$|^[\\s\\S]{1,' + chunkLength + '} ');
        var chunkArr = txt.match(pattRegex);

        if (chunkArr == null || chunkArr[0] === undefined || chunkArr[0].length <= 2) {
            //call once all text has been spoken...
            if (callback !== undefined) {
                callback();
            }
            return;
        }
        var chunk = chunkArr[0];
        const { Utterance } = getRequiredSpeechSynthesisApi();
        newUtt = new Utterance(chunk);
        var x;
        for (x in utt) {
            if (Object.hasOwn(utt, x) && x !== 'text') {
                newUtt[x] = utt[x];
            }
        }
        newUtt.lang = utt.lang;
        newUtt.voice = utt.voice;
        newUtt.rate = utt.rate;
        newUtt.pitch = utt.pitch;
        newUtt.addEventListener('end', function () {
            if (speechUtteranceChunker.cancel) {
                speechUtteranceChunker.cancel = false;
                return;
            }
            settings.offset = settings.offset || 0;
            settings.offset += chunk.length;
            speechUtteranceChunker(utt, settings, callback);
        });
    }

    if (settings.modifier) {
        settings.modifier(newUtt);
    }
    console.log(newUtt); //IMPORTANT!! Do not remove: Logging the object out fixes some onend firing issues.
    //placing the speak invocation inside a callback fixes ordering and onend issues.
    const { synth } = getRequiredSpeechSynthesisApi();
    setTimeout(function () {
        synth.speak(newUtt);
    }, 0);
};

class SystemTtsProvider {
    //########//
    // Config //
    //########//

    // Static constants for the simulated default voice
    static BROWSER_DEFAULT_VOICE_ID = '__browser_default__';
    static BROWSER_DEFAULT_VOICE_NAME = 'System Default Voice';

    settings;
    ready = false;
    voices = [];
    separator = ' ... ';

    defaultSettings = {
        voiceMap: {},
        rate: 1,
        pitch: 1,
    };

    get settingsHtml() {
        if (!getSpeechSynthesisApi()) {
            return t`Your browser or operating system doesn't support speech synthesis`;
        }

        return '<p>' + t`Uses the voices provided by your operating system` + `</p>
        <label for="system_tts_rate">` + t`Rate:` + ` <span id="system_tts_rate_output"></span></label>
        <input id="system_tts_rate" type="range" value="${this.defaultSettings.rate}" min="0.1" max="2" step="0.01" />
        <label for="system_tts_pitch">` + t`Pitch:` + ` <span id="system_tts_pitch_output"></span></label>
        <input id="system_tts_pitch" type="range" value="${this.defaultSettings.pitch}" min="0" max="2" step="0.01" />`;
    }

    onSettingsChange() {
        this.settings.rate = Number($('#system_tts_rate').val());
        this.settings.pitch = Number($('#system_tts_pitch').val());
        $('#system_tts_pitch_output').text(this.settings.pitch);
        $('#system_tts_rate_output').text(this.settings.rate);
        saveTtsProviderSettings();
    }

    async loadSettings(settings) {
        // Populate Provider UI given input settings
        if (Object.keys(settings).length == 0) {
            console.info('Using default TTS Provider settings');
        }

        // iOS should only allows speech synthesis trigged by user interaction
        const speechSynthesisApi = getSpeechSynthesisApi();
        if (isMobile() && speechSynthesisApi) {
            const { synth, Utterance } = speechSynthesisApi;
            let hasEnabledVoice = false;

            document.addEventListener('click', () => {
                if (hasEnabledVoice) {
                    return;
                }
                const utterance = new Utterance(' . ');
                utterance.volume = 0;
                synth.speak(utterance);
                hasEnabledVoice = true;
            });
        }

        // Only accept keys defined in defaultSettings
        this.settings = this.defaultSettings;

        for (const key in settings) {
            if (key in this.settings) {
                this.settings[key] = settings[key];
            } else {
                throw `Invalid setting passed to TTS Provider: ${key}`;
            }
        }

        $('#system_tts_rate').val(this.settings.rate || this.defaultSettings.rate);
        $('#system_tts_pitch').val(this.settings.pitch || this.defaultSettings.pitch);

        // Trigger updates
        $('#system_tts_rate').on('input', () => { this.onSettingsChange(); });
        $('#system_tts_pitch').on('input', () => { this.onSettingsChange(); });

        $('#system_tts_pitch_output').text(this.settings.pitch);
        $('#system_tts_rate_output').text(this.settings.rate);
        console.debug('SystemTTS: Settings loaded');
    }

    // Perform a simple readiness check by trying to fetch voiceIds
    async checkReady() {
        getRequiredSpeechSynthesisApi();
    }

    async onRefreshClick() {
        return;
    }

    //#################//
    //  TTS Interfaces //
    //#################//
    fetchTtsVoiceObjects() {
        const { synth } = getRequiredSpeechSynthesisApi();

        return new Promise((resolve) => {
            setTimeout(() => {
                let voices = synth.getVoices();

                if (voices.length === 0) {
                    // Edge compat: Provide default when voices empty
                    console.warn('SystemTTS: getVoices() returned empty list. Providing browser default option.');
                    const defaultVoice = {
                        name: SystemTtsProvider.BROWSER_DEFAULT_VOICE_NAME,
                        voice_id: SystemTtsProvider.BROWSER_DEFAULT_VOICE_ID,
                        preview_url: false,
                        lang: navigator.language || 'en-US',
                    };
                    resolve([defaultVoice]);
                } else {
                    const mappedVoices = voices
                        .sort((a, b) => a.lang.localeCompare(b.lang) || a.name.localeCompare(b.name))
                        .map(x => ({ name: x.name, voice_id: x.voiceURI, preview_url: false, lang: x.lang }));
                    resolve(mappedVoices);
                }
            }, 50);
        });
    }

    previewTtsVoice(voiceId) {
        const { synth, Utterance } = getRequiredSpeechSynthesisApi();

        let voice = null;
        if (voiceId !== SystemTtsProvider.BROWSER_DEFAULT_VOICE_ID) {
            const voices = synth.getVoices();
            voice = voices.find(x => x.voiceURI === voiceId);

            if (!voice && voices.length > 0) {
                console.warn(`SystemTTS Preview: Voice ID "${voiceId}" not found among available voices. Using browser default.`);
            } else if (!voice && voices.length === 0) {
                console.warn('SystemTTS Preview: Voice list is empty. Using browser default.');
            }
        } else {
            console.log('SystemTTS Preview: Using browser default voice as requested.');
        }

        synth.cancel();
        const langForPreview = voice ? voice.lang : (navigator.language || 'en-US');
        const text = getPreviewString(langForPreview);
        const utterance = new Utterance(text);

        if (voice) {
            utterance.voice = voice;
        }

        utterance.rate = this.settings.rate || 1;
        utterance.pitch = this.settings.pitch || 1;

        utterance.onerror = (event) => {
            console.error(`SystemTTS Preview Error: ${event.error}`, event);
        };

        synth.speak(utterance);
    }

    async getVoice(voiceName) {
        const { synth } = getRequiredSpeechSynthesisApi();

        if (voiceName === SystemTtsProvider.BROWSER_DEFAULT_VOICE_NAME) {
            return {
                voice_id: SystemTtsProvider.BROWSER_DEFAULT_VOICE_ID,
                name: SystemTtsProvider.BROWSER_DEFAULT_VOICE_NAME,
            };
        }

        const voices = synth.getVoices();

        if (voices.length === 0) {
            console.warn('SystemTTS: Empty voice list, using default fallback');
            return {
                voice_id: SystemTtsProvider.BROWSER_DEFAULT_VOICE_ID,
                name: SystemTtsProvider.BROWSER_DEFAULT_VOICE_NAME,
            };
        }

        const match = voices.find(x => x.name == voiceName);

        if (!match) {
            throw new Error(`SystemTTS getVoice: TTS Voice name "${voiceName}" not found`);
        }

        return { voice_id: match.voiceURI, name: match.name };
    }

    async generateTts(text, voiceId) {
        const { synth, Utterance } = getRequiredSpeechSynthesisApi();

        const silence = await fetch('/sounds/silence.mp3');

        return new Promise((resolve, reject) => {
            const voices = synth.getVoices();
            const voice = voices.find(x => x.voiceURI === voiceId);
            const utterance = new Utterance(text);
            utterance.voice = voice;
            utterance.rate = this.settings.rate || 1;
            utterance.pitch = this.settings.pitch || 1;
            utterance.onend = () => resolve(silence);
            utterance.onerror = () => reject();
            speechUtteranceChunker(utterance, {
                chunkLength: 200,
            }, function () {
                resolve(silence);
                console.log('System TTS done');
            });
        });
    }
}
