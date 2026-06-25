import { formatTimestampValue } from './format.js';

export const SyncButton = {
    props: {
        label: { type: String, required: true },
        icon: { type: String, default: '' },
        title: { type: String, default: '' },
        disabled: { type: Boolean, default: false },
        danger: { type: Boolean, default: false },
        iconOnly: { type: Boolean, default: false },
    },
    emits: ['click'],
    template: `
        <button
            type="button"
            class="menu_button margin0 tt-sync-button"
            :class="{ menu_button_icon: icon, red_button: danger }"
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

export const SyncSection = {
    props: {
        title: { type: String, required: true },
    },
    template: `
        <section class="tt-sync-section">
            <div class="tt-sync-section-header">
                <b>{{ title }}</b>
                <slot name="actions"></slot>
            </div>
            <slot></slot>
        </section>
    `,
};

export const SyncSwitch = {
    props: {
        modelValue: { type: Boolean, required: true },
        disabled: { type: Boolean, default: false },
        label: { type: String, default: '' },
        title: { type: String, default: '' },
    },
    emits: ['update:modelValue'],
    template: `
        <label class="tt-sync-switch" :class="{ 'is-disabled': disabled }" :title="title || label">
            <input
                type="checkbox"
                :checked="modelValue"
                :disabled="disabled"
                :aria-label="title || label"
                @change="$emit('update:modelValue', $event.target.checked)"
            />
            <span class="tt-sync-switch-track" aria-hidden="true"></span>
            <span v-if="label" class="tt-sync-switch-label">{{ label }}</span>
        </label>
    `,
};

export const SyncTargetRow = {
    props: {
        target: { type: Object, required: true },
        running: { type: Boolean, required: true },
        tr: { type: Function, required: true },
        disabled: { type: Boolean, default: false },
    },
    emits: ['rename', 'pull', 'push', 'remove'],
    components: {
        SyncButton,
    },
    computed: {
        isLan() {
            return this.target.type === 'lan';
        },
        protocolLabel() {
            if (!this.isLan) {
                return 'TT-Sync';
            }
            return 'LAN';
        },
        lastSyncText() {
            return this.target.lastSyncMs
                ? formatTimestampValue(this.target.lastSyncMs, this.tr)
                : this.tr('Never');
        },
        secondaryLine() {
            if (!this.isLan) {
                return this.target.baseUrl;
            }
            return this.target.lastKnownAddress || this.tr('Address: N/A (reconnect needed)');
        },
        pullDisabled() {
            return this.disabled || (this.isLan && !this.target.lastKnownAddress);
        },
        pushDisabled() {
            return this.disabled || (this.isLan && (!this.target.lastKnownAddress || !this.running));
        },
        pullTitle() {
            if (this.isLan && !this.target.lastKnownAddress) {
                return this.tr('Address missing. Reconnect using Pair URI.');
            }
            return this.isLan
                ? this.tr('Download (pull from this device)')
                : this.tr('Download (pull from this server)');
        },
        pushTitle() {
            if (this.isLan && !this.target.lastKnownAddress) {
                return this.tr('Address missing. Reconnect using Pair URI.');
            }
            if (this.isLan && !this.running) {
                return this.tr('Start LAN Sync server first (peer needs to download from you).');
            }
            return this.isLan
                ? this.tr('Upload (request device to pull from you)')
                : this.tr('Upload (push to this server)');
        },
        removeTitle() {
            return this.isLan ? this.tr('Remove device') : this.tr('Remove server');
        },
    },
    template: `
        <div class="tt-sync-target-row" :class="isLan ? 'tt-sync-target-lan' : 'tt-sync-target-tt'">
            <div class="tt-sync-target-main">
                <button
                    type="button"
                    class="tt-sync-target-name"
                    :title="tr('Click to rename')"
                    :disabled="disabled"
                    @click="$emit('rename', target)"
                >
                    <b>{{ target.displayName }}</b>
                    <i class="fa-solid fa-pen-to-square" aria-hidden="true"></i>
                </button>
                <div class="tt-sync-target-muted">{{ target.id }}</div>
                <div class="tt-sync-target-muted tt-sync-target-address">
                    <span>{{ secondaryLine }}</span>
                    <code>{{ protocolLabel }}</code>
                </div>
                <div class="tt-sync-target-muted">{{ tr('Last sync') }}: {{ lastSyncText }}</div>
            </div>
            <div class="tt-sync-target-actions">
                <SyncButton
                    :label="tr('Download')"
                    icon="fa-download"
                    icon-only
                    :title="pullTitle"
                    :disabled="pullDisabled"
                    @click="$emit('pull', target)"
                />
                <SyncButton
                    :label="tr('Upload')"
                    icon="fa-upload"
                    icon-only
                    :title="pushTitle"
                    :disabled="pushDisabled"
                    @click="$emit('push', target)"
                />
                <SyncButton
                    :label="tr('Remove')"
                    icon="fa-trash-can"
                    icon-only
                    :title="removeTitle"
                    :disabled="disabled"
                    @click="$emit('remove', target)"
                />
            </div>
        </div>
    `,
};
