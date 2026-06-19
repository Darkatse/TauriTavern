import {
    DevLogButton,
    TextPreviewSection,
} from './components.js';
import { formatTimestamp } from './log-utils.js';

function errorText(error) {
    return error?.message ? String(error.message) : String(error);
}

export const LlmApiLogsPanel = {
    props: {
        initialKeep: { type: Number, required: true },
        initialIndexEntries: { type: Array, default: () => [] },
        initialPreview: { type: Object, default: null },
        client: { type: Object, required: true },
        actions: { type: Object, required: true },
        tr: { type: Function, required: true },
    },
    components: {
        DevLogButton,
        TextPreviewSection,
    },
    data() {
        const indexEntries = this.initialIndexEntries.slice();
        return {
            keep: Number(this.initialKeep) || 1,
            keepInput: String(Number(this.initialKeep) || 1),
            indexEntries,
            index: Math.max(0, indexEntries.length - 1),
            currentPreview: this.initialPreview,
            currentRaw: null,
            rawOpen: false,
            loadingPreview: false,
            loadingRaw: false,
            unsubscribe: null,
        };
    },
    computed: {
        currentIndex() {
            if (!this.indexEntries.length) {
                return 0;
            }
            return Math.max(0, Math.min(this.index, this.indexEntries.length - 1));
        },
        currentEntry() {
            if (!this.indexEntries.length) {
                return null;
            }
            return this.indexEntries[this.currentIndex] || null;
        },
        currentId() {
            return this.currentEntry?.id ?? 0;
        },
        positionText() {
            if (!this.indexEntries.length) {
                return this.tr('No entries');
            }
            return `${this.currentIndex + 1}/${this.indexEntries.length}`;
        },
        activePreview() {
            return this.currentPreview && this.currentPreview.id === this.currentId
                ? this.currentPreview
                : null;
        },
        activeRaw() {
            return this.currentRaw && this.currentRaw.id === this.currentId
                ? this.currentRaw
                : null;
        },
        metaText() {
            const source = this.activePreview || this.currentEntry;
            if (!source) {
                return '';
            }
            if (source.error) {
                return String(source.error);
            }

            const modelSuffix = source.model ? ` (${source.model})` : '';
            return `${source.source}${modelSuffix}\n${source.endpoint}\n${this.tr('Duration')}: ${source.durationMs}ms    ok: ${source.ok}\n${formatTimestamp(source.timestampMs)}`;
        },
        requestReadable() {
            const preview = this.activePreview;
            if (!this.currentId) {
                return '';
            }
            if (!preview || this.loadingPreview) {
                return this.tr('Loading...');
            }
            if (preview.error) {
                return '';
            }
            return preview.requestReadable || '';
        },
        responseReadable() {
            const preview = this.activePreview;
            if (!this.currentId) {
                return '';
            }
            if (!preview || this.loadingPreview) {
                return this.tr('Loading...');
            }
            if (preview.error) {
                return '';
            }
            return preview.responseReadable || '';
        },
        requestRaw() {
            if (!this.rawOpen) {
                return '';
            }
            const raw = this.activeRaw;
            if (!raw || this.loadingRaw) {
                return this.tr('Loading...');
            }
            if (raw.error) {
                return String(raw.error);
            }
            return raw.requestRaw || '';
        },
        responseRaw() {
            if (!this.rawOpen) {
                return '';
            }
            const raw = this.activeRaw;
            if (!raw || this.loadingRaw) {
                return this.tr('Loading...');
            }
            if (raw.error) {
                return '';
            }
            return raw.responseRaw || '';
        },
        rawRequestViewerTitle() {
            return `${this.tr('Raw JSON/SSE')} - ${this.tr('Request body')}`;
        },
        rawResponseViewerTitle() {
            return `${this.tr('Raw JSON/SSE')} - ${this.tr('Response body')}`;
        },
    },
    async mounted() {
        try {
            if (this.currentId && !this.activePreview) {
                await this.loadPreview(this.currentId);
            }
            this.unsubscribe = await this.client.subscribeIndex((entry) => {
                this.handleIndexEntry(entry);
            });
        } catch (error) {
            this.reportError(error);
        }
    },
    unmounted() {
        void this.unsubscribe?.();
    },
    methods: {
        formatTimestamp,
        reportError(error) {
            void this.actions.reportError(error);
        },
        async loadPreview(id) {
            if (!id) {
                this.currentPreview = null;
                return;
            }

            this.loadingPreview = true;
            this.currentPreview = null;
            try {
                this.currentPreview = await this.client.getPreview(id);
            } catch (error) {
                this.currentPreview = {
                    id,
                    error: errorText(error),
                };
            } finally {
                this.loadingPreview = false;
            }
        },
        async loadRaw(id) {
            if (!id) {
                this.currentRaw = null;
                return;
            }

            this.loadingRaw = true;
            this.currentRaw = null;
            try {
                this.currentRaw = await this.client.getRaw(id);
            } catch (error) {
                this.currentRaw = {
                    id,
                    error: errorText(error),
                };
            } finally {
                this.loadingRaw = false;
            }
        },
        async ensurePreviewLoaded() {
            if (!this.currentId || this.activePreview) {
                return;
            }
            await this.loadPreview(this.currentId);
        },
        async ensureRawLoaded() {
            if (!this.currentId || this.activeRaw) {
                return;
            }
            await this.loadRaw(this.currentId);
        },
        async setCurrentIndex(next) {
            if (!this.indexEntries.length) {
                return;
            }
            this.index = Math.max(0, Math.min(next, this.indexEntries.length - 1));
            this.currentRaw = null;
            await this.loadPreview(this.currentId);
            if (this.rawOpen) {
                await this.loadRaw(this.currentId);
            }
        },
        async reloadCurrent() {
            if (!this.currentId) {
                return;
            }
            await this.loadPreview(this.currentId);
            if (this.rawOpen) {
                await this.loadRaw(this.currentId);
            }
        },
        async setRawOpen(open) {
            this.rawOpen = open;
            if (!open) {
                this.currentRaw = null;
                return;
            }
            await this.ensureRawLoaded();
        },
        async copyText(text) {
            await this.actions.copyText(text);
        },
        async copyReadableRequest() {
            await this.ensurePreviewLoaded();
            await this.copyText(this.requestReadable);
        },
        async copyReadableResponse() {
            await this.ensurePreviewLoaded();
            await this.copyText(this.responseReadable);
        },
        async copyRawRequest() {
            await this.ensureRawLoaded();
            await this.copyText(this.requestRaw);
        },
        async copyRawResponse() {
            await this.ensureRawLoaded();
            await this.copyText(this.responseRaw);
        },
        async expandReadableRequest() {
            await this.ensurePreviewLoaded();
            await this.actions.openTextViewer({
                title: this.tr('Request body'),
                text: this.requestReadable,
                wrap: 'soft',
            });
        },
        async expandReadableResponse() {
            await this.ensurePreviewLoaded();
            await this.actions.openTextViewer({
                title: this.tr('Response body'),
                text: this.responseReadable,
                wrap: 'soft',
            });
        },
        async expandRawRequest() {
            await this.ensureRawLoaded();
            await this.actions.openTextViewer({
                title: this.rawRequestViewerTitle,
                text: this.requestRaw,
                wrap: 'off',
            });
        },
        async expandRawResponse() {
            await this.ensureRawLoaded();
            await this.actions.openTextViewer({
                title: this.rawResponseViewerTitle,
                text: this.responseRaw,
                wrap: 'off',
            });
        },
        async applyKeep() {
            const nextKeepRaw = Number(this.keepInput);
            if (!Number.isFinite(nextKeepRaw) || nextKeepRaw <= 0) {
                throw new Error(this.tr('LLM API keep must be a positive number'));
            }

            this.keep = Math.floor(nextKeepRaw);
            this.keepInput = String(this.keep);

            await this.client.setKeep(this.keep);
            this.indexEntries = await this.client.index({ limit: this.keep });
            this.index = Math.max(0, this.indexEntries.length - 1);
            this.currentRaw = null;
            await this.loadPreview(this.currentId);
        },
        applyKeepOrReport() {
            void this.applyKeep().catch((error) => this.reportError(error));
        },
        handleIndexEntry(entry) {
            const shouldFollowLatest = this.index >= this.indexEntries.length - 1;
            this.indexEntries.push(entry);
            if (this.indexEntries.length > this.keep) {
                this.indexEntries.splice(0, this.indexEntries.length - this.keep);
            }

            if (!shouldFollowLatest) {
                return;
            }

            this.index = Math.max(0, this.indexEntries.length - 1);
            void this.loadPreview(this.currentId);
        },
    },
    template: `
        <div class="tt-dev-logs-root tt-dev-llm-root">
            <header class="tt-dev-log-toolbar">
                <b>{{ tr('LLM API Logs') }}</b>
                <DevLogButton :label="tr('Prev')" icon="fa-chevron-left" @click="setCurrentIndex(index - 1)" />
                <DevLogButton :label="tr('Next')" icon="fa-chevron-right" @click="setCurrentIndex(index + 1)" />
                <DevLogButton :label="tr('Reload')" icon="fa-rotate" @click="reloadCurrent" />
                <small class="tt-dev-log-position">{{ positionText }}</small>
                <DevLogButton :label="tr('Copy Request')" icon="fa-copy" @click="copyReadableRequest" />
                <DevLogButton :label="tr('Copy Response')" icon="fa-copy" @click="copyReadableResponse" />
            </header>

            <div class="tt-dev-log-settings-row">
                <span>{{ tr('LLM API keep') }}</span>
                <input
                    v-model="keepInput"
                    class="text_pole tt-dev-log-keep-input"
                    type="number"
                    min="1"
                    step="1"
                />
                <DevLogButton :label="tr('Apply')" icon="fa-check" @click="applyKeepOrReport" />
            </div>

            <small class="tt-dev-log-note">{{ tr('LLM API logs capture prompt/response bodies.') }}</small>
            <div class="tt-dev-log-meta">{{ metaText }}</div>

            <TextPreviewSection
                :title="tr('Request body')"
                :text="requestReadable"
                :placeholder="tr('Request body')"
                :rows="10"
                @expand="expandReadableRequest"
            />
            <TextPreviewSection
                :title="tr('Response body')"
                :text="responseReadable"
                :placeholder="tr('Response body')"
                :rows="14"
                @expand="expandReadableResponse"
            />

            <details class="tt-dev-log-raw" :open="rawOpen" @toggle="setRawOpen($event.currentTarget.open)">
                <summary>{{ tr('Raw JSON/SSE') }}</summary>
                <div class="tt-dev-log-raw-body">
                    <div class="tt-dev-log-toolbar compact">
                        <DevLogButton :label="tr('Copy Raw Request')" icon="fa-copy" @click="copyRawRequest" />
                        <DevLogButton :label="tr('Copy Raw Response')" icon="fa-copy" @click="copyRawResponse" />
                    </div>
                    <TextPreviewSection
                        :title="tr('Request body')"
                        :viewer-title="rawRequestViewerTitle"
                        :text="requestRaw"
                        :placeholder="tr('Request body')"
                        :rows="10"
                        wrap="off"
                        @expand="expandRawRequest"
                    />
                    <TextPreviewSection
                        :title="tr('Response body')"
                        :viewer-title="rawResponseViewerTitle"
                        :text="responseRaw"
                        :placeholder="tr('Response body')"
                        :rows="14"
                        wrap="off"
                        @expand="expandRawResponse"
                    />
                </div>
            </details>
        </div>
    `,
};
