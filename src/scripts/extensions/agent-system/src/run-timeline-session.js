import { eventBelongsToInvocation } from './run-invocation-projector.js';
import { TERMINAL_EVENT_TYPES } from './run-event-presenter.js';
import {
    createRunTimelineEventStore,
    RUN_EVENT_PAGE_LIMIT,
    RUN_EVENT_TAIL_SEQ,
} from './run-timeline-event-store.js';
import {
    emptyTimelineProjection,
    normalizeTimelineProjection,
} from './run-timeline-projection.js';

export function createRunTimelineSession(options = {}) {
    return {
        runId: '',
        invocationId: '',
        includeTimelineProjection: false,
        eventStore: createRunTimelineEventStore(),
        events: [],
        timelineProjection: emptyTimelineProjection(),
        terminalEvent: null,
        loading: false,
        loadingOlder: false,
        hasMoreBefore: false,
        requestId: 0,

        reset(next = {}) {
            this.runId = normalizeOptionalString(next.runId);
            this.invocationId = normalizeOptionalString(next.invocationId);
            this.includeTimelineProjection = next.includeTimelineProjection === true;
            this.eventStore = createRunTimelineEventStore();
            this.events = [];
            this.timelineProjection = emptyTimelineProjection();
            this.terminalEvent = null;
            this.loading = false;
            this.loadingOlder = false;
            this.hasMoreBefore = false;
            this.requestId += 1;
            return this;
        },

        async loadInitial(readEvents) {
            return loadInitialTimelinePage(this, readEvents);
        },

        async loadOlder(readEvents) {
            return loadOlderTimelinePage(this, readEvents);
        },

        async refreshProjection(readEvents) {
            return refreshTimelineProjection(this, readEvents);
        },

        receiveEvents(events) {
            if (!Array.isArray(events)) {
                throw new Error('agent.timeline_events_invalid: events must be an array');
            }
            let added = false;
            for (const event of events) {
                added = this.receiveEvent(event) || added;
            }
            return added;
        },

        receiveEvent(event) {
            if (!this.acceptsEvent(event)) {
                return false;
            }
            if (!this.eventStore.add(event)) {
                return false;
            }
            this.events = this.eventStore.events();
            if (TERMINAL_EVENT_TYPES.includes(event.type)) {
                this.terminalEvent = event;
            }
            return true;
        },

        acceptsEvent(event) {
            if (!event?.runId) {
                return false;
            }
            if (!this.runId) {
                this.runId = String(event.runId);
            }
            if (event.runId !== this.runId) {
                return false;
            }
            if (!this.invocationId) {
                return true;
            }
            return eventBelongsToInvocation(event, this.invocationId);
        },

        oldestSeq() {
            return this.eventStore.oldestSeq();
        },
    }.reset(options);
}

async function loadInitialTimelinePage(session, readEvents) {
    const requestId = ++session.requestId;
    session.loading = true;
    try {
        const result = await readTimelinePage(session, readEvents, {
            beforeSeq: RUN_EVENT_TAIL_SEQ,
        });
        if (!isCurrentRequest(session, requestId)) {
            return false;
        }
        applyReadEventsResult(session, result);
        return true;
    } finally {
        if (isCurrentRequest(session, requestId)) {
            session.loading = false;
        }
    }
}

async function loadOlderTimelinePage(session, readEvents) {
    if (session.loading || session.loadingOlder || !session.hasMoreBefore) {
        return false;
    }
    const beforeSeq = session.oldestSeq();
    if (beforeSeq == null || beforeSeq <= 1) {
        session.hasMoreBefore = false;
        return false;
    }

    const requestId = ++session.requestId;
    session.loadingOlder = true;
    try {
        const result = await readTimelinePage(session, readEvents, { beforeSeq });
        if (!isCurrentRequest(session, requestId)) {
            return false;
        }
        applyReadEventsResult(session, result);
        return true;
    } finally {
        if (isCurrentRequest(session, requestId)) {
            session.loadingOlder = false;
        }
    }
}

async function refreshTimelineProjection(session, readEvents) {
    if (!session.includeTimelineProjection) {
        return false;
    }
    const requestId = session.requestId;
    const result = await readTimelinePage(session, readEvents, {
        afterSeq: RUN_EVENT_TAIL_SEQ,
        limit: 1,
    });
    if (!isCurrentRequest(session, requestId)) {
        return false;
    }
    session.timelineProjection = normalizeTimelineProjection(result.timelineProjection);
    return true;
}

async function readTimelinePage(session, readEvents, page) {
    if (typeof readEvents !== 'function') {
        throw new Error('Agent timeline readEvents dependency must be a function.');
    }
    const runId = requireRunId(session.runId);
    const input = {
        runId,
        limit: page.limit ?? RUN_EVENT_PAGE_LIMIT,
    };
    if (session.invocationId) {
        input.invocationId = session.invocationId;
    }
    if (page.beforeSeq != null) {
        input.beforeSeq = page.beforeSeq;
    }
    if (page.afterSeq != null) {
        input.afterSeq = page.afterSeq;
    }
    if (session.includeTimelineProjection) {
        input.includeTimelineProjection = true;
    }
    return readEvents(input);
}

function applyReadEventsResult(session, result) {
    if (session.includeTimelineProjection) {
        session.timelineProjection = normalizeTimelineProjection(result?.timelineProjection);
    }
    if (!Array.isArray(result?.events)) {
        throw new Error('agent.timeline_events_invalid: readEvents.events must be an array');
    }
    session.receiveEvents(result.events);
    session.hasMoreBefore = result.events.length >= RUN_EVENT_PAGE_LIMIT
        && Number(session.oldestSeq() || 0) > 1;
}

function isCurrentRequest(session, requestId) {
    return session.requestId === requestId;
}

function requireRunId(value) {
    const runId = normalizeOptionalString(value);
    if (!runId) {
        throw new Error('Agent run id is required.');
    }
    return runId;
}

function normalizeOptionalString(value) {
    return String(value || '').trim();
}
