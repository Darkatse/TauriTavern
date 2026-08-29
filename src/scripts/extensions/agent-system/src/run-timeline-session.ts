import { eventBelongsToInvocation } from './run-invocation-projector';
import { TERMINAL_EVENT_TYPES } from './run-event-presenter';
import type {
    TimelineProjection,
    TimelineReadInput,
    TimelineReadResult,
} from './RunTimelineContract';
import {
    createRunTimelineEventStore,
    RUN_EVENT_PAGE_LIMIT,
    RUN_EVENT_TAIL_SEQ,
} from './run-timeline-event-store';
import { emptyTimelineProjection, normalizeTimelineProjection } from './run-timeline-projection';

type RunTimelineSessionOptions = {
    runId?: string;
    invocationId?: string;
    includeTimelineProjection?: boolean;
};

type TimelineEventReader = (input: TimelineReadInput) => Promise<TimelineReadResult>;

type RunTimelineSession = {
    runId: string;
    invocationId: string;
    includeTimelineProjection: boolean;
    events: TauriTavernAgentRunEvent[];
    timelineProjection: TimelineProjection;
    terminalEvent: TauriTavernAgentRunEvent | null;
    loading: boolean;
    loadingOlder: boolean;
    hasMoreBefore: boolean;
    requestId: number;
    reset: (next?: RunTimelineSessionOptions) => RunTimelineSession;
    loadInitial: (readEvents: TimelineEventReader) => Promise<boolean>;
    loadOlder: (readEvents: TimelineEventReader) => Promise<boolean>;
    refreshProjection: (readEvents: TimelineEventReader) => Promise<boolean>;
    receiveEvents: (events: readonly TauriTavernAgentRunEvent[]) => boolean;
    receiveEvent: (event: TauriTavernAgentRunEvent) => boolean;
    acceptsEvent: (event: TauriTavernAgentRunEvent) => boolean;
};

export function createRunTimelineSession(options: RunTimelineSessionOptions = {}): RunTimelineSession {
    let eventStore = createRunTimelineEventStore();
    const session: RunTimelineSession = {
        runId: '',
        invocationId: '',
        includeTimelineProjection: false,
        events: [],
        timelineProjection: emptyTimelineProjection(),
        terminalEvent: null,
        loading: false,
        loadingOlder: false,
        hasMoreBefore: false,
        requestId: 0,
        reset(next = {}) {
            session.runId = optionalString(next.runId);
            session.invocationId = optionalString(next.invocationId);
            session.includeTimelineProjection = next.includeTimelineProjection === true;
            eventStore = createRunTimelineEventStore();
            session.events = [];
            session.timelineProjection = emptyTimelineProjection();
            session.terminalEvent = null;
            session.loading = false;
            session.loadingOlder = false;
            session.hasMoreBefore = false;
            session.requestId += 1;
            return session;
        },
        loadInitial: readEvents => loadInitialPage(session, readEvents),
        loadOlder: readEvents => loadOlderPage(session, readEvents),
        refreshProjection: readEvents => refreshProjection(session, readEvents),
        receiveEvents(events) {
            const accepted = events.filter(session.acceptsEvent);
            if (!eventStore.addMany(accepted)) return false;
            session.events = eventStore.events();
            for (const event of accepted) {
                if (TERMINAL_EVENT_TYPES.includes(event.type)) session.terminalEvent = event;
            }
            return true;
        },
        receiveEvent: event => session.receiveEvents([event]),
        acceptsEvent(event) {
            if (!event.runId) throw new Error('Agent run event runId is required.');
            if (!session.runId) session.runId = event.runId;
            if (event.runId !== session.runId) return false;
            return !session.invocationId || eventBelongsToInvocation(event, session.invocationId);
        },
    };
    return session.reset(options);
}

async function loadInitialPage(session: RunTimelineSession, readEvents: TimelineEventReader): Promise<boolean> {
    const requestId = ++session.requestId;
    session.loading = true;
    try {
        const result = await readPage(session, readEvents, { beforeSeq: RUN_EVENT_TAIL_SEQ });
        if (!isCurrent(session, requestId)) return false;
        applyResult(session, result);
        return true;
    } finally {
        if (isCurrent(session, requestId)) session.loading = false;
    }
}

async function loadOlderPage(session: RunTimelineSession, readEvents: TimelineEventReader): Promise<boolean> {
    if (session.loading || session.loadingOlder || !session.hasMoreBefore) return false;
    const beforeSeq = session.events[0]?.seq;
    if (beforeSeq == null || beforeSeq <= 1) {
        session.hasMoreBefore = false;
        return false;
    }
    const requestId = ++session.requestId;
    session.loadingOlder = true;
    try {
        const result = await readPage(session, readEvents, { beforeSeq });
        if (!isCurrent(session, requestId)) return false;
        applyResult(session, result);
        return true;
    } finally {
        if (isCurrent(session, requestId)) session.loadingOlder = false;
    }
}

async function refreshProjection(session: RunTimelineSession, readEvents: TimelineEventReader): Promise<boolean> {
    if (!session.includeTimelineProjection) return false;
    const requestId = session.requestId;
    const result = await readPage(session, readEvents, { afterSeq: RUN_EVENT_TAIL_SEQ, limit: 1 });
    if (!isCurrent(session, requestId)) return false;
    session.timelineProjection = normalizeTimelineProjection(result.timelineProjection);
    return true;
}

async function readPage(
    session: RunTimelineSession,
    readEvents: TimelineEventReader,
    page: { beforeSeq?: number; afterSeq?: number; limit?: number },
): Promise<TimelineReadResult> {
    const input: TimelineReadInput = {
        runId: requireRunId(session.runId),
        limit: page.limit ?? RUN_EVENT_PAGE_LIMIT,
    };
    if (session.invocationId) input.invocationId = session.invocationId;
    if (page.beforeSeq != null) input.beforeSeq = page.beforeSeq;
    if (page.afterSeq != null) input.afterSeq = page.afterSeq;
    if (session.includeTimelineProjection) input.includeTimelineProjection = true;
    return readEvents(input);
}

function applyResult(session: RunTimelineSession, result: TimelineReadResult): void {
    if (session.includeTimelineProjection) {
        session.timelineProjection = normalizeTimelineProjection(result.timelineProjection);
    }
    if (!Array.isArray(result.events)) {
        throw new Error('agent.timeline_events_invalid: readEvents.events must be an array');
    }
    session.receiveEvents(result.events);
    session.hasMoreBefore = result.events.length >= RUN_EVENT_PAGE_LIMIT
        && (session.events[0]?.seq ?? 0) > 1;
}

function isCurrent(session: RunTimelineSession, requestId: number): boolean {
    return session.requestId === requestId;
}

function requireRunId(value: string): string {
    const runId = optionalString(value);
    if (!runId) throw new Error('Agent run id is required.');
    return runId;
}

function optionalString(value: string | undefined): string {
    return value?.trim() ?? '';
}
