export const SettingsSection = {
    props: {
        title: { type: String, required: true },
        icon: { type: String, default: '' },
    },
    template: `
        <section class="tt-settings-section">
            <div class="tt-settings-section-title">
                <i v-if="icon" class="fa-solid" :class="icon" aria-hidden="true"></i>
                <b>{{ title }}</b>
            </div>
            <div class="tt-settings-section-body">
                <slot></slot>
            </div>
        </section>
    `,
};

export const SettingRow = {
    props: {
        label: { type: String, required: true },
        hint: { type: String, default: '' },
        helpTopic: { type: String, default: '' },
        helpTitle: { type: String, default: '' },
    },
    emits: ['help'],
    template: `
        <div class="tt-settings-row">
            <div class="tt-settings-row-copy">
                <div class="tt-settings-label-line">
                    <span>{{ label }}</span>
                    <button
                        v-if="helpTopic"
                        type="button"
                        class="tt-settings-icon-button"
                        :title="helpTitle"
                        @click="$emit('help', helpTopic)"
                    >
                        <i class="fa-solid fa-circle-question" aria-hidden="true"></i>
                    </button>
                </div>
                <small v-if="hint" class="tt-settings-hint">{{ hint }}</small>
            </div>
            <div class="tt-settings-control">
                <slot></slot>
            </div>
        </div>
    `,
};

export const ToggleSwitch = {
    props: {
        modelValue: { type: Boolean, required: true },
        disabled: { type: Boolean, default: false },
    },
    emits: ['update:modelValue'],
    template: `
        <label class="tt-settings-switch">
            <input
                type="checkbox"
                :checked="modelValue"
                :disabled="disabled"
                @change="$emit('update:modelValue', $event.target.checked)"
            />
            <span aria-hidden="true"></span>
        </label>
    `,
};

export const SelectField = {
    props: {
        modelValue: { type: String, required: true },
        options: { type: Array, required: true },
        disabled: { type: Boolean, default: false },
    },
    emits: ['update:modelValue'],
    template: `
        <select
            class="text_pole tt-settings-select"
            :value="modelValue"
            :disabled="disabled"
            @change="$emit('update:modelValue', $event.target.value)"
        >
            <option v-for="option in options" :key="option.value" :value="option.value">
                {{ option.label }}
            </option>
        </select>
    `,
};

export const ActionButton = {
    props: {
        label: { type: String, required: true },
        icon: { type: String, default: '' },
        title: { type: String, default: '' },
        disabled: { type: Boolean, default: false },
    },
    emits: ['click'],
    template: `
        <button
            type="button"
            class="menu_button menu_button_icon tt-settings-action-button"
            :title="title || label"
            :disabled="disabled"
            @click="$emit('click')"
        >
            <i v-if="icon" class="fa-solid" :class="icon" aria-hidden="true"></i>
            <span>{{ label }}</span>
        </button>
    `,
};
