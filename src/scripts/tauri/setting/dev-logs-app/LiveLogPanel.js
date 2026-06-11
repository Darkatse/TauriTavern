import {
    DevLogButton,
    DevLogToggle,
    LogRow,
} from './components.js';
import {
    LOG_LEVEL_OPTIONS,
    LIVE_LOG_PANEL_BUFFER_LIMIT,
    LIVE_LOG_PANEL_DEFAULT_WINDOW_SIZE,
    LIVE_LOG_PANEL_MAX_WINDOW_SIZE,
    LIVE_LOG_PANEL_WINDOW_GROW_STEP,
    entryMatchesLevel,
    formatEntryLine,
    levelClass,
} from './log-utils.js';

function isNearBottom(container) {
    if (!container) {
        return true;
    }
    return container.scrollHeight - container.scrollTop - container.clientHeight < 24;
}

export const LiveLogPanel = {
    props: {
        title: { type: String, required: true },
        initialEntries: { type: Array, default: () => [] },
        client: { type: Object, required: true },
        actions: { type: Object, required: true },
        tr: { type: Function, required: true },
        showConsoleCapture: { type: Boolean, default: false },
        consoleCaptureEnabled: { type: Boolean, default: false },
        trimEntriesInPlace: { type: Function, default: null },
    },
    components: {
        DevLogButton,
        DevLogToggle,
        LogRow,
    },
    data() {
        return {
            entries: this.initialEntries.slice(),
            renderedEntries: [],
            filter: 'ALL',
            paused: false,
            windowSize: LIVE_LOG_PANEL_DEFAULT_WINDOW_SIZE,
            consoleCapture: this.consoleCaptureEnabled,
            wasNearBottom: true,
            unsubscribe: null,
        };
    },
    computed: {
        levelOptions() {
            return LOG_LEVEL_OPTIONS;
        },
        canGrowWindow() {
            return this.windowSize < LIVE_LOG_PANEL_MAX_WINDOW_SIZE;
        },
    },
    async mounted() {
        this.renderTail();
        try {
            this.unsubscribe = await this.client.subscribe((entry) => {
                this.handleEntry(entry);
            });
        } catch (error) {
            this.reportError(error);
        }
    },
    unmounted() {
        void this.unsubscribe?.();
    },
    methods: {
        levelClass,
        reportError(error) {
            void this.actions.reportError(error);
        },
        entryKey(entry, index) {
            return entry.id ?? `${entry.timestampMs}-${index}`;
        },
        trimEntries() {
            if (this.trimEntriesInPlace) {
                this.trimEntriesInPlace(this.entries);
                return;
            }
            if (this.entries.length > LIVE_LOG_PANEL_BUFFER_LIMIT) {
                this.entries.splice(0, this.entries.length - LIVE_LOG_PANEL_BUFFER_LIMIT);
            }
        },
        scrollToTail() {
            this.$nextTick(() => {
                const container = this.$refs.logContainer;
                if (container instanceof HTMLElement) {
                    container.scrollTop = container.scrollHeight;
                }
            });
        },
        renderTail() {
            this.trimEntries();
            this.windowSize = Math.min(this.windowSize, LIVE_LOG_PANEL_MAX_WINDOW_SIZE);
            this.renderedEntries = this.entries
                .filter((entry) => entryMatchesLevel(entry, this.filter))
                .slice(-this.windowSize);
            this.wasNearBottom = true;
            this.scrollToTail();
        },
        handleEntry(entry) {
            const shouldFollow = !this.paused && isNearBottom(this.$refs.logContainer);
            this.entries.push(entry);
            this.trimEntries();

            if (!shouldFollow) {
                return;
            }
            if (!entryMatchesLevel(entry, this.filter)) {
                return;
            }

            this.renderedEntries.push(entry);
            while (this.renderedEntries.length > this.windowSize) {
                this.renderedEntries.shift();
            }
            this.scrollToTail();
        },
        setFilter(next) {
            this.filter = next;
            this.renderTail();
        },
        setPaused(paused) {
            this.paused = paused;
            if (!paused) {
                this.renderTail();
            }
        },
        handleScroll() {
            const nearBottom = isNearBottom(this.$refs.logContainer);
            if (!this.paused && nearBottom && !this.wasNearBottom) {
                this.renderTail();
                return;
            }
            this.wasNearBottom = nearBottom;
        },
        showMore() {
            if (!this.canGrowWindow) {
                return;
            }
            this.windowSize = Math.min(
                this.windowSize + LIVE_LOG_PANEL_WINDOW_GROW_STEP,
                LIVE_LOG_PANEL_MAX_WINDOW_SIZE,
            );
            this.renderTail();
        },
        copyRendered() {
            const text = this.renderedEntries.map(formatEntryLine).join('\n');
            void this.actions.copyText(text);
        },
        clearRendered() {
            this.entries = [];
            this.renderedEntries = [];
        },
        async setConsoleCapture(enabled) {
            this.consoleCapture = enabled;
            try {
                await this.client.setConsoleCaptureEnabled(enabled);
            } catch (error) {
                this.consoleCapture = !enabled;
                this.reportError(error);
            }
        },
    },
    template: `
        <div class="tt-dev-logs-root">
            <header class="tt-dev-log-toolbar">
                <b>{{ tr(title) }}</b>
                <DevLogToggle
                    v-if="showConsoleCapture"
                    :model-value="consoleCapture"
                    :label="tr('Capture full console logs')"
                    @update:model-value="setConsoleCapture"
                />
                <DevLogToggle
                    :model-value="paused"
                    :label="tr('Pause')"
                    @update:model-value="setPaused"
                />
                <DevLogButton :label="tr('Jump to tail')" icon="fa-arrow-down" @click="renderTail" />
                <DevLogButton :label="tr('More')" icon="fa-plus" :disabled="!canGrowWindow" @click="showMore" />
                <DevLogButton :label="tr('Copy')" icon="fa-copy" @click="copyRendered" />
                <DevLogButton :label="tr('Clear')" icon="fa-trash" @click="clearRendered" />
            </header>

            <div class="tt-dev-log-levels">
                <button
                    v-for="level in levelOptions"
                    :key="level"
                    type="button"
                    class="tt-dev-log-chip"
                    :class="[levelClass(level), { active: level === filter }]"
                    @click="setFilter(level)"
                >
                    {{ level }}
                </button>
            </div>

            <div ref="logContainer" class="tt-dev-log-list" @scroll="handleScroll">
                <LogRow
                    v-for="(entry, entryIndex) in renderedEntries"
                    :key="entryKey(entry, entryIndex)"
                    :entry="entry"
                />
            </div>
        </div>
    `,
};
