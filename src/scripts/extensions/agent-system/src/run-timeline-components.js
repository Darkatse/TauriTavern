import { translateAgentSystem as tr } from './i18n.js';
import {
    subAgentStatusLabel,
    subAgentTaskStyle,
    subAgentTaskTone,
    timelineItemShortLabel,
    timelineItemTime,
    timelineItemTitle,
} from './run-timeline-display.js';
import { timelineItemHeightPx, timelineItemRowSpan } from './run-timeline-virtual-list.js';

const HISTORY_TOP_LOAD_THRESHOLD_PX = 72;

export const RunTimelineEventList = {
    props: {
        ariaLabel: { type: String, required: true },
        surfaceClass: { type: String, required: true },
        listClass: { type: String, default: '' },
        loading: { type: Boolean, default: false },
        loadingOlder: { type: Boolean, default: false },
        emptyText: { type: String, required: true },
        items: { type: Array, default: () => [] },
        virtualItems: { type: Object, required: true },
        selectedSeq: { type: Number, default: null },
        latestSeq: { type: Number, default: null },
        activeSeq: { type: Number, default: null },
        itemKeyPrefix: { type: String, default: '' },
        markLatest: { type: Boolean, default: true },
    },
    emits: ['select', 'top-reached', 'viewport'],
    methods: {
        tr(key, params) {
            return tr(key, params);
        },
        itemTitle(item) {
            return timelineItemTitle(item);
        },
        itemShortLabel(item) {
            return timelineItemShortLabel(item);
        },
        itemTime(item) {
            return timelineItemTime(item);
        },
        isSelected(item) {
            return this.selectedSeq != null && this.selectedSeq === item.seq;
        },
        isLatest(item) {
            return this.markLatest && this.latestSeq != null && this.latestSeq === item.seq;
        },
        isActive(item) {
            return this.activeSeq != null && this.activeSeq === item.seq;
        },
        itemKey(item) {
            return `${this.itemKeyPrefix}${item.id}`;
        },
        itemRowSpan(item) {
            return timelineItemRowSpan(item);
        },
        itemRowStyle(item) {
            return {
                '--ttas-run-event-item-height': `${timelineItemHeightPx(item)}px`,
            };
        },
        captureScrollAnchor() {
            const scroller = this.$refs.scroller;
            if (!(scroller instanceof HTMLElement)) {
                return null;
            }
            return {
                scrollHeight: scroller.scrollHeight,
                scrollTop: scroller.scrollTop,
            };
        },
        restoreScrollAnchor(anchor) {
            const scroller = this.$refs.scroller;
            if (!(scroller instanceof HTMLElement) || !anchor) {
                return;
            }
            const delta = scroller.scrollHeight - anchor.scrollHeight;
            scroller.scrollTop = anchor.scrollTop + Math.max(0, delta);
            this.emitViewport(scroller);
        },
        scrollToBottom() {
            const scroller = this.$refs.scroller;
            if (!(scroller instanceof HTMLElement)) {
                return;
            }
            scroller.scrollTop = scroller.scrollHeight;
            this.emitViewport(scroller);
        },
        measureViewport() {
            const scroller = this.$refs.scroller;
            if (!(scroller instanceof HTMLElement)) {
                return null;
            }
            return this.emitViewport(scroller);
        },
        isNearBottom() {
            const scroller = this.$refs.scroller;
            if (!(scroller instanceof HTMLElement)) {
                return true;
            }
            return scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop < 18;
        },
        onScroll() {
            const scroller = this.$refs.scroller;
            if (!(scroller instanceof HTMLElement)) {
                return;
            }
            this.emitViewport(scroller);
            if (scroller.scrollTop <= HISTORY_TOP_LOAD_THRESHOLD_PX) {
                this.$emit('top-reached');
            }
        },
        emitViewport(scroller) {
            const viewport = {
                scrollTop: scroller.scrollTop,
                viewportHeight: Math.max(1, scroller.clientHeight),
                nearBottom: scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop < 18,
            };
            this.$emit('viewport', viewport);
            return viewport;
        },
    },
    template: `
        <div
            ref="scroller"
            :class="surfaceClass"
            :aria-label="ariaLabel"
            @scroll.passive="onScroll"
        >
            <div v-if="loading && items.length === 0" class="ttas-run-empty">
                <i class="fa-solid fa-spinner fa-spin"></i>
                <span>{{ tr('timelineLoading') }}</span>
            </div>
            <div v-else-if="items.length === 0" class="ttas-run-empty">
                <i class="fa-solid fa-circle-dot"></i>
                <span>{{ emptyText }}</span>
            </div>
            <ol v-else class="ttas-run-events is-windowed" :class="listClass">
                <li
                    v-if="loadingOlder"
                    class="ttas-run-event-loader"
                    aria-live="polite"
                >
                    <i class="fa-solid fa-spinner fa-spin"></i>
                    <span>{{ tr('timelineLoading') }}</span>
                </li>
                <li
                    v-if="virtualItems.topPadding > 0"
                    class="ttas-run-event-spacer"
                    :style="{ height: virtualItems.topPadding + 'px' }"
                    aria-hidden="true"
                ></li>
                <li
                    v-for="item in virtualItems.items"
                    :key="itemKey(item)"
                    class="ttas-run-event"
                    :data-ttas-kind="item.kind"
                    :data-ttas-row-span="itemRowSpan(item)"
                    :style="itemRowStyle(item)"
                    :class="[
                        'tone-' + item.tone,
                        'kind-' + item.kind,
                        {
                            'is-latest': isLatest(item),
                            'is-active': isActive(item),
                            'is-selected': isSelected(item),
                        },
                    ]"
                >
                    <button type="button" @click="$emit('select', item)">
                        <span class="ttas-run-event-icon" aria-hidden="true">
                            <i class="fa-solid" :class="item.icon"></i>
                        </span>
                        <span class="ttas-run-event-copy">
                            <span class="ttas-run-event-title">
                                {{ itemTitle(item) }}
                                <span v-if="isActive(item)" class="ttas-run-ellipsis" aria-hidden="true">
                                    <i>.</i><i>.</i><i>.</i>
                                </span>
                            </span>
                            <small v-if="item.summary">{{ item.summary }}</small>
                        </span>
                        <span class="ttas-run-event-meta">
                            <em>{{ itemShortLabel(item) }}</em>
                            <time v-if="itemTime(item)">{{ itemTime(item) }}</time>
                        </span>
                    </button>
                </li>
                <li
                    v-if="virtualItems.bottomPadding > 0"
                    class="ttas-run-event-spacer"
                    :style="{ height: virtualItems.bottomPadding + 'px' }"
                    aria-hidden="true"
                ></li>
            </ol>
        </div>
    `,
};

export const RunTimelineDetailPane = {
    props: {
        rootClass: { type: String, required: true },
        ariaLabel: { type: String, required: true },
        title: { type: String, required: true },
        type: { type: String, default: '' },
        navItems: { type: Array, default: () => [] },
        selectedSeq: { type: Number, default: null },
        loading: { type: Boolean, default: false },
        error: { type: String, default: '' },
        sections: { type: Array, default: () => [] },
        showBack: { type: Boolean, default: false },
    },
    emits: ['back', 'select-nav', 'action'],
    methods: {
        tr(key, params) {
            return tr(key, params);
        },
        itemTitle(item) {
            return timelineItemTitle(item);
        },
        itemShortLabel(item) {
            return timelineItemShortLabel(item);
        },
        isSelected(item) {
            return this.selectedSeq != null && this.selectedSeq === item.seq;
        },
        actionTitle(action) {
            return action.hintKey ? tr(action.hintKey) : tr(action.labelKey);
        },
    },
    template: `
        <section :class="rootClass" :aria-label="ariaLabel">
            <div class="ttas-run-detail-head">
                <button
                    v-if="showBack"
                    type="button"
                    class="menu_button menu_button_icon ttas-run-icon-button"
                    :title="tr('showTimelineEvents')"
                    :aria-label="tr('showTimelineEvents')"
                    @click="$emit('back')"
                >
                    <i class="fa-solid fa-arrow-left"></i>
                </button>
                <div>
                    <strong>{{ title }}</strong>
                    <small v-if="type">{{ type }}</small>
                </div>
            </div>

            <div v-if="navItems.length > 1" class="ttas-run-detail-nav">
                <div class="ttas-run-nav-list">
                    <button
                        v-for="item in navItems"
                        :key="'nav-' + item.id"
                        type="button"
                        :class="{ 'is-selected': isSelected(item) }"
                        :title="itemTitle(item)"
                        @click.stop="$emit('select-nav', item)"
                    >
                        <i aria-hidden="true"></i>
                        <span>{{ itemShortLabel(item) }}</span>
                    </button>
                </div>
            </div>

            <div class="ttas-run-detail-scroll">
                <div v-if="loading" class="ttas-run-empty">
                    <i class="fa-solid fa-spinner fa-spin"></i>
                    <span>{{ tr('timelineLoadingDetails') }}</span>
                </div>
                <div v-else-if="error" class="ttas-run-detail-error">
                    <i class="fa-solid fa-triangle-exclamation"></i>
                    <span>{{ error }}</span>
                </div>
                <div v-else-if="sections.length === 0" class="ttas-run-empty">
                    <i class="fa-solid fa-file-circle-question"></i>
                    <span>{{ tr('timelineDetailEmpty') }}</span>
                </div>
                <article v-for="(section, index) in sections" :key="index" class="ttas-run-detail-section">
                    <div class="ttas-run-detail-section-head">
                        <strong>{{ tr(section.labelKey) }}</strong>
                        <small v-if="section.path">{{ section.path }}</small>
                    </div>
                    <div v-if="section.actions && section.actions.length" class="ttas-run-detail-actions">
                        <button
                            v-for="action in section.actions"
                            :key="action.kind"
                            type="button"
                            class="menu_button ttas-run-detail-action"
                            :data-ttas-action="action.kind"
                            :title="actionTitle(action)"
                            @click.stop="$emit('action', action)"
                        >
                            <i v-if="action.icon" class="fa-solid" :class="action.icon" aria-hidden="true"></i>
                            <span>{{ tr(action.labelKey) }}</span>
                        </button>
                    </div>
                    <div v-if="section.fields && section.fields.length" class="ttas-run-detail-fields">
                        <span v-for="field in section.fields" :key="field.label">
                            <b>{{ field.label }}</b>
                            <em>{{ field.value }}</em>
                        </span>
                    </div>
                    <div v-if="section.blocks && section.blocks.length" class="ttas-run-detail-blocks">
                        <details
                            v-for="(block, blockIndex) in section.blocks"
                            :key="blockIndex"
                            class="ttas-run-detail-block"
                            :class="'kind-' + (block.kind || 'text')"
                            :data-ttas-block-kind="block.kind || 'text'"
                            :open="block.defaultOpen !== false"
                        >
                            <summary class="ttas-run-detail-block-head">
                                <strong>{{ block.labelKey ? tr(block.labelKey) : block.label }}</strong>
                                <span class="ttas-run-detail-block-badges">
                                    <small v-if="block.meta">{{ block.meta }}</small>
                                    <small v-if="block.truncated">{{ tr('timelineTruncated') }}</small>
                                    <i class="fa-solid fa-chevron-down" aria-hidden="true"></i>
                                </span>
                            </summary>
                            <div v-if="block.kind === 'diff'" class="ttas-run-diff" role="table">
                                <div
                                    v-for="(row, rowIndex) in block.rows"
                                    :key="rowIndex"
                                    class="ttas-run-diff-row"
                                    :data-ttas-diff-row="row.type"
                                    role="row"
                                >
                                    <span class="ttas-run-diff-gutter" role="cell">{{ row.oldLine || '' }}</span>
                                    <span class="ttas-run-diff-gutter" role="cell">{{ row.newLine || '' }}</span>
                                    <span class="ttas-run-diff-marker" role="cell">{{ row.marker }}</span>
                                    <code class="ttas-run-diff-code" role="cell">{{ row.text }}</code>
                                </div>
                            </div>
                            <pre v-else>{{ block.text }}</pre>
                        </details>
                    </div>
                </article>
            </div>
        </section>
    `,
};

export const SubAgentTray = {
    props: {
        expanded: { type: Boolean, default: false },
        tasks: { type: Array, default: () => [] },
        title: { type: String, required: true },
    },
    emits: ['toggle', 'select'],
    methods: {
        tr(key, params) {
            return tr(key, params);
        },
        subAgentStatusLabel(status) {
            return subAgentStatusLabel(status);
        },
        subAgentTaskStyle(task) {
            return subAgentTaskStyle(task);
        },
        subAgentTaskTone(task) {
            return subAgentTaskTone(task);
        },
    },
    template: `
        <aside
            v-if="tasks.length > 0"
            class="ttas-subagent-tray"
            :class="{ 'is-expanded': expanded }"
        >
            <button
                type="button"
                class="ttas-subagent-tray-toggle"
                :aria-expanded="String(expanded)"
                :title="expanded ? tr('timelineCollapseSubAgents') : tr('timelineExpandSubAgents')"
                @click="$emit('toggle')"
            >
                <span class="ttas-subagent-stack" aria-hidden="true">
                    <i
                        v-for="task in tasks.slice(0, 4)"
                        :key="'dot-' + task.taskId"
                        :style="subAgentTaskStyle(task)"
                    ></i>
                </span>
                <strong>{{ title }}</strong>
                <i class="fa-solid" :class="expanded ? 'fa-chevron-down' : 'fa-chevron-up'" aria-hidden="true"></i>
            </button>
            <div v-if="expanded" class="ttas-subagent-list">
                <button
                    v-for="task in tasks"
                    :key="task.taskId"
                    type="button"
                    class="ttas-subagent-item"
                    :data-ttas-status="subAgentTaskTone(task)"
                    :style="subAgentTaskStyle(task)"
                    @click="$emit('select', task)"
                >
                    <span class="ttas-subagent-color" aria-hidden="true"></span>
                    <span class="ttas-subagent-copy">
                        <strong>{{ task.displayName }}</strong>
                        <small>{{ subAgentStatusLabel(task.status) }}</small>
                    </span>
                    <span class="ttas-subagent-open">
                        <i class="fa-solid fa-up-right-from-square" aria-hidden="true"></i>
                        <span>{{ tr('timelineOpenSubAgent') }}</span>
                    </span>
                </button>
            </div>
        </aside>
    `,
};
