import { LiveLogPanel } from './LiveLogPanel.js';
import { LlmApiLogsPanel } from './LlmApiLogsPanel.js';

export function createTauriTavernDevLogsApp(options) {
    const {
        kind,
        title = '',
        initialEntries = [],
        client,
        actions,
        tr,
    } = options || {};

    if (typeof tr !== 'function') {
        throw new Error('TauriTavern dev logs translator is required');
    }
    if (!client || typeof client !== 'object') {
        throw new Error('TauriTavern dev logs client is required');
    }
    if (!actions || typeof actions !== 'object') {
        throw new Error('TauriTavern dev logs actions are required');
    }
    if (kind !== 'live' && kind !== 'llm-api') {
        throw new Error(`Unsupported TauriTavern dev logs panel: ${kind}`);
    }

    return {
        name: 'TauriTavernDevLogsApp',
        components: {
            LiveLogPanel,
            LlmApiLogsPanel,
        },
        data() {
            return {
                kind,
                title,
                initialEntries,
                client,
                actions,
                tr,
                showConsoleCapture: Boolean(options.showConsoleCapture),
                consoleCaptureEnabled: Boolean(options.consoleCaptureEnabled),
                trimEntriesInPlace: options.trimEntriesInPlace || null,
                initialKeep: Number(options.initialKeep) || 1,
                initialIndexEntries: Array.isArray(options.initialIndexEntries)
                    ? options.initialIndexEntries
                    : [],
                initialPreview: options.initialPreview || null,
            };
        },
        template: `
            <LiveLogPanel
                v-if="kind === 'live'"
                :title="title"
                :initial-entries="initialEntries"
                :client="client"
                :actions="actions"
                :tr="tr"
                :show-console-capture="showConsoleCapture"
                :console-capture-enabled="consoleCaptureEnabled"
                :trim-entries-in-place="trimEntriesInPlace"
            />
            <LlmApiLogsPanel
                v-else-if="kind === 'llm-api'"
                :initial-keep="initialKeep"
                :initial-index-entries="initialIndexEntries"
                :initial-preview="initialPreview"
                :client="client"
                :actions="actions"
                :tr="tr"
            />
        `,
    };
}
