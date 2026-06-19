import { formatBytesValue } from './format.js';

function normalizePayload(payload) {
    return payload || {};
}

export function createTauriTavernSyncProgressApp(options) {
    const { tr } = options || {};
    if (typeof tr !== 'function') {
        throw new Error('TauriTavern Sync progress translator is required');
    }

    return {
        name: 'TauriTavernSyncProgressApp',
        data() {
            return {
                title: options.title || 'Sync progress',
                payload: normalizePayload(options.payload),
            };
        },
        computed: {
            phaseText() {
                const direction = this.payload.direction || null;
                const phase = this.payload.phase || 'Starting';

                return direction
                    ? `${this.tr('Phase')}: ${this.tr(direction)} / ${this.tr(phase)}`
                    : `${this.tr('Phase')}: ${this.tr(phase)}`;
            },
            countsText() {
                return `${this.tr('Files')}: ${Number(this.payload.files_done) || 0}/${Number(this.payload.files_total) || 0}`;
            },
            bytesText() {
                return `${this.tr('Bytes')}: ${formatBytesValue(this.payload.bytes_done)}/${formatBytesValue(this.payload.bytes_total)}`;
            },
            currentText() {
                const currentPath = this.payload.current_path || '';
                return currentPath ? `${this.tr('Current')}: ${currentPath}` : '';
            },
        },
        methods: {
            tr(key) {
                return tr(key);
            },
            update(next) {
                if (next.title) {
                    this.title = next.title;
                }
                if (next.payload) {
                    this.payload = normalizePayload(next.payload);
                }
            },
        },
        template: `
            <div class="tt-sync-progress-root">
                <b>{{ tr(title) }}</b>
                <div>{{ phaseText }}</div>
                <div>{{ countsText }}</div>
                <div>{{ bytesText }}</div>
                <div class="tt-sync-progress-current">{{ currentText }}</div>
            </div>
        `,
    };
}
