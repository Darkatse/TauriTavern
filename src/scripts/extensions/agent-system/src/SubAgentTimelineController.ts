import type { AgentSystemTr } from './i18n';
import type {
    SubAgentTask,
    SubAgentTimelineSnapshot,
    TimelineItem,
    TimelineReadInput,
    TimelineReadResult,
    TimelineViewport,
} from './RunTimelineContract';
import {
    buildEventDetailTargets,
    hasModelTurnNarration,
    isDisplayableRunEvent,
    timelineItemsFromEvents,
} from './run-event-presenter';
import { createTimelineDetailState } from './run-timeline-detail-state';
import { subAgentStatusLabel } from './run-timeline-display';
import { createRunTimelineSession } from './run-timeline-session';
import { virtualizeTimelineItems } from './run-timeline-virtual-list';

export function createSubAgentTimelineController(deps: {
    readEvents: (request: TimelineReadInput) => Promise<TimelineReadResult>;
    readOnly: boolean;
    reportError: (error: unknown) => void;
    tr: AgentSystemTr;
    onChange: () => void;
}) {
    const session = createRunTimelineSession();
    const detail = createTimelineDetailState();
    let isOpen = false;
    let invocationId = '';
    let selectedSeq: number | null = null;
    let viewport: TimelineViewport = { scrollTop: 0, viewportHeight: 1, nearBottom: true };
    // Session arrays are immutable references; keep presentation stable across viewport-only publishes.
    let eventRef: readonly TauriTavernAgentRunEvent[] = session.events;
    let invocationRef = '';
    let items = timelineItemsFromEvents([]);
    let selectedItemsRef: readonly TimelineItem[] | null = null;
    let selectedSeqRef: number | null = null;
    let selectedCache: TimelineItem | null = null;

    function currentItems() {
        if (eventRef !== session.events || invocationRef !== invocationId) {
            eventRef = session.events;
            invocationRef = invocationId;
            items = invocationId ? timelineItemsFromEvents(session.events, { invocationId }) : [];
        }
        return items;
    }

    function selectedItem() {
        const all = currentItems();
        if (selectedItemsRef !== all || selectedSeqRef !== selectedSeq) {
            selectedItemsRef = all;
            selectedSeqRef = selectedSeq;
            selectedCache = (selectedSeq == null ? null : all.find(item => item.seq === selectedSeq)) ?? all.at(-1) ?? null;
        }
        return selectedCache;
    }

    async function loadInitial(): Promise<void> {
        try {
            const pending = session.loadInitial(deps.readEvents);
            deps.onChange();
            const applied = await pending;
            if (applied && isOpen) void loadDetails();
            else deps.onChange();
        } catch (error) {
            deps.onChange();
            deps.reportError(error);
        }
    }

    async function loadDetails(): Promise<void> {
        const item = selectedItem();
        if (!item || !session.runId) {
            detail.reset();
            deps.onChange();
            return;
        }
        const pending = detail.load({
            runId: session.runId,
            targets: buildEventDetailTargets(item, session.events),
            readOnly: deps.readOnly,
        });
        deps.onChange();
        await pending;
        deps.onChange();
    }

    function reset(): void {
        isOpen = false;
        invocationId = '';
        selectedSeq = null;
        viewport = { scrollTop: 0, viewportHeight: 1, nearBottom: true };
        session.reset();
        detail.reset();
    }

    return {
        snapshot(task: SubAgentTask | null): SubAgentTimelineSnapshot {
            const all = currentItems();
            const selected = selectedItem();
            return {
                open: isOpen,
                task,
                title: task ? task.displayName : deps.tr('subAgent'),
                subtitle: task ? `${subAgentStatusLabel(task.status, deps.tr)} | ${task.workspaceKey}` : '',
                displayItems: all,
                virtualItems: virtualizeTimelineItems(all, viewport.scrollTop, viewport.viewportHeight),
                selectedItem: selected,
                selectedSeq: selected?.seq ?? null,
                navItems: all.slice(-20),
                loading: session.loading,
                loadingOlder: session.loadingOlder,
                autoStick: viewport.nearBottom,
                detail: { loading: detail.loading, error: detail.error, sections: detail.sections },
            };
        },
        invocationId: () => invocationId,
        open(runId: string, nextInvocationId: string): void {
            isOpen = true;
            invocationId = nextInvocationId;
            selectedSeq = null;
            viewport = { scrollTop: 0, viewportHeight: 1, nearBottom: true };
            detail.reset();
            session.reset({ runId, invocationId: nextInvocationId });
            void loadInitial();
        },
        close(): void {
            if (!isOpen) return;
            reset();
            deps.onChange();
        },
        reset,
        receiveEvent(event: TauriTavernAgentRunEvent): boolean {
            if (!isOpen || !session.receiveEvent(event)) return false;
            if (selectedSeq == null && (isDisplayableRunEvent(event) || hasModelTurnNarration(event))) {
                void loadDetails();
            }
            return true;
        },
        async loadOlder(): Promise<boolean> {
            try {
                const pending = session.loadOlder(deps.readEvents);
                deps.onChange();
                const applied = await pending;
                deps.onChange();
                return applied;
            } catch (error) {
                deps.onChange();
                deps.reportError(error);
                return false;
            }
        },
        select(seq: number): void {
            selectedSeq = seq;
            void loadDetails();
        },
        setViewport(next: TimelineViewport): void {
            viewport = next;
            deps.onChange();
        },
        dispose(): void {
            reset();
        },
    };
}
