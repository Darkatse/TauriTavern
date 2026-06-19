import {
    formatTimestamp,
    levelClass,
    normalizeLevel,
} from './log-utils.js';

export const DevLogButton = {
    props: {
        label: { type: String, required: true },
        icon: { type: String, default: '' },
        disabled: { type: Boolean, default: false },
        title: { type: String, default: '' },
        iconOnly: { type: Boolean, default: false },
    },
    emits: ['click'],
    template: `
        <button
            type="button"
            class="menu_button menu_button_icon tt-dev-log-button"
            :title="title || label"
            :aria-label="title || label"
            :disabled="disabled"
            @click="$emit('click')"
        >
            <i v-if="icon" class="fa-solid" :class="icon" aria-hidden="true"></i>
            <span v-if="!iconOnly">{{ label }}</span>
        </button>
    `,
};

export const DevLogToggle = {
    props: {
        modelValue: { type: Boolean, required: true },
        label: { type: String, required: true },
    },
    emits: ['update:modelValue'],
    template: `
        <label class="tt-dev-log-toggle">
            <input
                type="checkbox"
                :checked="modelValue"
                @change="$emit('update:modelValue', $event.target.checked)"
            />
            <span>{{ label }}</span>
        </label>
    `,
};

export const LogRow = {
    props: {
        entry: { type: Object, required: true },
    },
    methods: {
        formatTimestamp,
        normalizeLevel,
        levelClass,
    },
    template: `
        <div class="tt-dev-log-row" :class="levelClass(entry.level)">
            <div class="tt-dev-log-prefix">
                <span class="tt-dev-log-time">{{ formatTimestamp(entry.timestampMs) }}</span>
                <span class="tt-dev-log-badge">{{ normalizeLevel(entry.level) }}</span>
                <span v-if="entry.target" class="tt-dev-log-target">{{ entry.target }}</span>
            </div>
            <span class="tt-dev-log-message">{{ entry.message }}</span>
        </div>
    `,
};

export const TextPreviewSection = {
    props: {
        title: { type: String, required: true },
        text: { type: String, default: '' },
        placeholder: { type: String, default: '' },
        rows: { type: Number, default: 10 },
        viewerTitle: { type: String, default: '' },
        wrap: { type: String, default: 'soft' },
    },
    emits: ['expand'],
    template: `
        <section class="tt-dev-log-text-section">
            <div class="tt-dev-log-text-header">
                <span>{{ title }}</span>
                <DevLogButton
                    :label="title"
                    icon="fa-expand"
                    icon-only
                    :title="viewerTitle || title"
                    @click="$emit('expand')"
                />
            </div>
            <textarea
                class="text_pole tt-dev-log-textarea"
                :rows="rows"
                readonly
                spellcheck="false"
                :placeholder="placeholder"
                :wrap="wrap"
                :value="text"
            ></textarea>
        </section>
    `,
    components: {
        DevLogButton,
    },
};
