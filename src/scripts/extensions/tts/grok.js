import { event_types, eventSource, getRequestHeaders } from '../../../script.js';
import { SECRET_KEYS, secret_state } from '../../secrets.js';
import { getPreviewString, saveTtsProviderSettings } from './index.js';

export { GrokTtsProvider };

class GrokTtsProvider {
    static voices = [
        { name: 'Ara', voice_id: 'ara', lang: 'en-US' },
        { name: 'Eve', voice_id: 'eve', lang: 'en-US' },
        { name: 'Leo', voice_id: 'leo', lang: 'en-US' },
        { name: 'Rex', voice_id: 'rex', lang: 'en-US' },
        { name: 'Sal', voice_id: 'sal', lang: 'en-US' },
    ];

    static languages = [
        { value: 'auto', label: 'Auto Detect' },
        { value: 'en', label: 'English' },
        { value: 'ar-EG', label: 'Arabic (Egypt)' },
        { value: 'ar-SA', label: 'Arabic (Saudi Arabia)' },
        { value: 'ar-AE', label: 'Arabic (UAE)' },
        { value: 'bn', label: 'Bengali' },
        { value: 'zh', label: 'Chinese (Simplified)' },
        { value: 'fr', label: 'French' },
        { value: 'de', label: 'German' },
        { value: 'hi', label: 'Hindi' },
        { value: 'id', label: 'Indonesian' },
        { value: 'it', label: 'Italian' },
        { value: 'ja', label: 'Japanese' },
        { value: 'ko', label: 'Korean' },
        { value: 'pt-BR', label: 'Portuguese (Brazil)' },
        { value: 'pt-PT', label: 'Portuguese (Portugal)' },
        { value: 'ru', label: 'Russian' },
        { value: 'es-MX', label: 'Spanish (Mexico)' },
        { value: 'es-ES', label: 'Spanish (Spain)' },
        { value: 'tr', label: 'Turkish' },
        { value: 'vi', label: 'Vietnamese' },
    ];

    settings;
    voices = [];
    separator = ' . ';
    audioElement = document.createElement('audio');

    defaultSettings = {
        voiceMap: {},
        language: 'auto',
        codec: 'mp3',
        sampleRate: 24000,
        bitRate: 128000,
    };

    get settingsHtml() {
        return `
        <div>Use xAI's Grok TTS endpoint.</div>
        <small>Hint: save your xAI API key in the key manager to use Grok TTS here.</small>
        <div class="flex-container alignItemsCenter">
            <div class="flex1"></div>
            <div id="grok_tts_key" class="menu_button menu_button_icon manage-api-keys" data-key="api_key_xai">
                <i class="fa-solid fa-key"></i>
                <span>API Key</span>
            </div>
        </div>
        <div>
            <label for="grok_tts_language">Language:</label>
            <select id="grok_tts_language" class="text_pole"></select>
        </div>`;
    }

    constructor() {
        this.handler = async function (key) {
            if (key !== SECRET_KEYS.XAI) return;
            $('#grok_tts_key').toggleClass('success', !!secret_state[SECRET_KEYS.XAI]);
            await this.onRefreshClick();
        }.bind(this);
    }

    dispose() {
        [event_types.SECRET_WRITTEN, event_types.SECRET_DELETED, event_types.SECRET_ROTATED].forEach(event => {
            eventSource.removeListener(event, this.handler);
        });
    }

    async loadSettings(settings) {
        this.settings = { ...this.defaultSettings };

        for (const key in settings) {
            if (key in this.settings) {
                this.settings[key] = settings[key];
            } else {
                throw `Invalid setting passed to TTS Provider: ${key}`;
            }
        }

        $('#grok_tts_language').empty();
        for (const language of GrokTtsProvider.languages) {
            $('#grok_tts_language').append($('<option />').val(language.value).text(language.label));
        }

        $('#grok_tts_language').val(this.settings.language);
        $('#grok_tts_language').on('change', () => {
            this.onSettingsChange();
        });

        $('#grok_tts_key').toggleClass('success', !!secret_state[SECRET_KEYS.XAI]);
        [event_types.SECRET_WRITTEN, event_types.SECRET_DELETED, event_types.SECRET_ROTATED].forEach(event => {
            eventSource.on(event, this.handler);
        });

        await this.checkReady();
    }

    onSettingsChange() {
        this.settings.language = String($('#grok_tts_language').find(':selected').val() || 'auto');
        saveTtsProviderSettings();
    }

    async checkReady() {
        this.voices = await this.fetchTtsVoiceObjects();
    }

    async onRefreshClick() {
        this.voices = await this.fetchTtsVoiceObjects();
    }

    async getVoice(voiceName) {
        if (this.voices.length === 0) {
            this.voices = await this.fetchTtsVoiceObjects();
        }

        const voice = this.voices.find(voice => voice.voice_id === voiceName || voice.name === voiceName);

        if (!voice) {
            throw `TTS Voice not found: ${voiceName}`;
        }

        return voice;
    }

    async generateTts(text, voiceId) {
        return await this.fetchTtsGeneration(text, voiceId);
    }

    async fetchTtsVoiceObjects() {
        if (!secret_state[SECRET_KEYS.XAI]) {
            return GrokTtsProvider.voices;
        }

        const response = await fetch('/api/tts/grok/voices', {
            method: 'POST',
            headers: getRequestHeaders(),
        });

        if (!response.ok) {
            throw new Error(`HTTP ${response.status}: ${await response.text()}`);
        }

        const payload = await response.json();
        const voices = Array.isArray(payload?.voices) ? payload.voices : [];
        const mappedVoices = voices.map(voice => ({
            name: String(voice?.name || voice?.voice_id || 'Unknown'),
            voice_id: String(voice?.voice_id || '').toLowerCase(),
            lang: 'en-US',
        })).filter(voice => voice.voice_id);

        if (!mappedVoices.length) {
            throw new Error('Grok voice list response did not include any voices');
        }

        return mappedVoices;
    }

    async previewTtsVoice(voiceId) {
        this.audioElement.pause();
        this.audioElement.currentTime = 0;

        const previewLang = this.mapXaiLanguageToPreviewLanguage(this.settings.language);
        const response = await this.fetchTtsGeneration(getPreviewString(previewLang), voiceId);

        if (!response.ok) {
            throw new Error(`HTTP ${response.status}`);
        }

        const audio = await response.blob();
        const url = URL.createObjectURL(audio);
        this.audioElement.src = url;
        this.audioElement.play();
        this.audioElement.onended = () => URL.revokeObjectURL(url);
    }

    mapXaiLanguageToPreviewLanguage(language) {
        const previewLanguageMap = {
            auto: 'en-US',
            en: 'en-US',
            'ar-EG': 'ar-SA',
            'ar-SA': 'ar-SA',
            'ar-AE': 'ar-SA',
            bn: 'en-US',
            zh: 'zh-CN',
            fr: 'fr-FR',
            de: 'de-DE',
            hi: 'hi-IN',
            id: 'id-ID',
            it: 'it-IT',
            ja: 'ja-JP',
            ko: 'ko-KR',
            'pt-BR': 'pt-BR',
            'pt-PT': 'pt-PR',
            ru: 'ru-RU',
            'es-MX': 'es-MX',
            'es-ES': 'es-ES',
            tr: 'tr-TR',
            vi: 'vi-VN',
        };

        return previewLanguageMap[language] || 'en-US';
    }

    async fetchTtsGeneration(inputText, voiceId) {
        const response = await fetch('/api/tts/grok/generate', {
            method: 'POST',
            headers: getRequestHeaders(),
            body: JSON.stringify({
                text: inputText,
                voiceId: String(voiceId || 'eve').toLowerCase(),
                language: this.settings.language || 'auto',
                outputFormat: {
                    codec: this.settings.codec,
                    sampleRate: this.settings.sampleRate,
                    bitRate: this.settings.bitRate,
                },
            }),
        });

        if (!response.ok) {
            toastr.error(response.statusText, 'TTS Generation Failed');
            throw new Error(`HTTP ${response.status}: ${await response.text()}`);
        }

        return response;
    }
}
