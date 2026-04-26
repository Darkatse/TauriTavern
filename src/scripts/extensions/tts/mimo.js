import { event_types, eventSource, getRequestHeaders } from '../../../script.js';
import { SECRET_KEYS, secret_state } from '../../secrets.js';
import { getPreviewString, initVoiceMap, saveTtsProviderSettings } from './index.js';

export { MimoTtsProvider };

class MimoTtsProvider {
    static models = [
        { id: 'mimo-v2-tts', name: 'MiMo-V2-TTS' },
        { id: 'mimo-v2.5-tts', name: 'MiMo-V2.5-TTS' },
    ];

    static modelVoices = {
        'mimo-v2-tts': [
            { name: 'MiMo Default', voice_id: 'mimo_default', lang: 'zh-CN' },
            { name: 'Chinese Female', voice_id: 'default_zh', lang: 'zh-CN' },
            { name: 'English Female', voice_id: 'default_en', lang: 'en-US' },
        ],
        'mimo-v2.5-tts': [
            { name: 'MiMo Default', voice_id: 'mimo_default', lang: 'zh-CN' },
            { name: 'Bingtang', voice_id: '冰糖', lang: 'zh-CN' },
            { name: 'Moli', voice_id: '茉莉', lang: 'zh-CN' },
            { name: 'Suda', voice_id: '苏打', lang: 'zh-CN' },
            { name: 'Baihua', voice_id: '白桦', lang: 'zh-CN' },
            { name: 'Mia', voice_id: 'Mia', lang: 'en-US' },
            { name: 'Chloe', voice_id: 'Chloe', lang: 'en-US' },
            { name: 'Milo', voice_id: 'Milo', lang: 'en-US' },
            { name: 'Dean', voice_id: 'Dean', lang: 'en-US' },
        ],
    };

    settings;
    voices = [];
    separator = ' . ';
    audioElement = document.createElement('audio');

    defaultSettings = {
        voiceMap: {},
        model: 'mimo-v2-tts',
        format: 'wav',
        instructions: '',
    };

    get settingsHtml() {
        return `
        <div>Use Xiaomi MiMo's speech synthesis API.</div>
        <small>Hint: for MiMo V2.5, natural-language style instructions go in the prompt below; inline tags in the text itself also work.</small>
        <div class="flex-container alignItemsCenter">
            <div class="flex1"></div>
            <div id="mimo_tts_key" class="menu_button menu_button_icon manage-api-keys" data-key="api_key_mimo">
                <i class="fa-solid fa-key"></i>
                <span>API Key</span>
            </div>
        </div>
        <div>
            <label for="mimo_tts_model">Model:</label>
            <select id="mimo_tts_model" class="text_pole"></select>
        </div>
        <div>
            <label for="mimo_tts_format">Output Format:</label>
            <select id="mimo_tts_format" class="text_pole">
                <option value="wav">WAV</option>
                <option value="mp3">MP3</option>
            </select>
        </div>
        <div>
            <label for="mimo_tts_instructions">Optional Style Prompt:</label>
            <textarea id="mimo_tts_instructions" class="textarea_compact autoSetHeight" placeholder="Example: Bright, excited tone with a slightly faster pace."></textarea>
        </div>`;
    }

    constructor() {
        this.handler = async function (key) {
            if (key !== SECRET_KEYS.MIMO) return;
            $('#mimo_tts_key').toggleClass('success', !!secret_state[SECRET_KEYS.MIMO]);
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

        $('#mimo_tts_model').empty();
        for (const model of MimoTtsProvider.models) {
            $('#mimo_tts_model').append($('<option />').val(model.id).text(model.name));
        }

        $('#mimo_tts_model').val(this.settings.model);
        $('#mimo_tts_model').on('change', async () => {
            await this.onSettingsChange();
        });

        $('#mimo_tts_format').val(this.settings.format);
        $('#mimo_tts_format').on('change', () => {
            this.onSettingsChange();
        });

        $('#mimo_tts_instructions').val(this.settings.instructions);
        $('#mimo_tts_instructions').on('input', () => {
            this.onSettingsChange();
        });

        $('#mimo_tts_key').toggleClass('success', !!secret_state[SECRET_KEYS.MIMO]);
        [event_types.SECRET_WRITTEN, event_types.SECRET_DELETED, event_types.SECRET_ROTATED].forEach(event => {
            eventSource.on(event, this.handler);
        });

        await this.checkReady();
    }

    async onSettingsChange() {
        const previousModel = this.settings.model;
        this.settings.model = String($('#mimo_tts_model').find(':selected').val() || this.defaultSettings.model);
        this.settings.format = String($('#mimo_tts_format').find(':selected').val() || this.defaultSettings.format);
        this.settings.instructions = String($('#mimo_tts_instructions').val() || '');
        saveTtsProviderSettings();

        if (previousModel !== this.settings.model) {
            this.voices = this.getVoicesForCurrentModel();
            await initVoiceMap();
        }
    }

    async checkReady() {
        this.voices = await this.fetchTtsVoiceObjects();
    }

    async onRefreshClick() {
        this.voices = await this.fetchTtsVoiceObjects();
        await initVoiceMap();
    }

    getVoicesForCurrentModel() {
        return MimoTtsProvider.modelVoices[this.settings.model] || MimoTtsProvider.modelVoices[this.defaultSettings.model];
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
        return this.getVoicesForCurrentModel();
    }

    async previewTtsVoice(voiceId) {
        this.audioElement.pause();
        this.audioElement.currentTime = 0;

        const voice = await this.getVoice(voiceId);
        const response = await this.fetchTtsGeneration(getPreviewString(voice.lang || 'en-US'), voiceId);

        if (!response.ok) {
            throw new Error(`HTTP ${response.status}`);
        }

        const audio = await response.blob();
        const url = URL.createObjectURL(audio);
        this.audioElement.src = url;
        this.audioElement.play();
        this.audioElement.onended = () => URL.revokeObjectURL(url);
    }

    async fetchTtsGeneration(inputText, voiceId) {
        const response = await fetch('/api/tts/mimo/generate', {
            method: 'POST',
            headers: getRequestHeaders(),
            body: JSON.stringify({
                text: inputText,
                voiceId: voiceId || 'mimo_default',
                model: this.settings.model,
                format: this.settings.format,
                instructions: this.settings.instructions,
            }),
        });

        if (!response.ok) {
            toastr.error(response.statusText, 'TTS Generation Failed');
            throw new Error(`HTTP ${response.status}: ${await response.text()}`);
        }

        return response;
    }
}
