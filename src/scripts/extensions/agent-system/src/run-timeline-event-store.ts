export const RUN_EVENT_PAGE_LIMIT = 240;
export const RUN_EVENT_TAIL_SEQ = Number.MAX_SAFE_INTEGER;

export type RunTimelineEventStore = {
    add: (event: TauriTavernAgentRunEvent) => boolean;
    events: () => TauriTavernAgentRunEvent[];
    oldestSeq: () => number | null;
};

export function createRunTimelineEventStore(): RunTimelineEventStore {
    const eventsByKey = new Map<string, TauriTavernAgentRunEvent>();
    const events: TauriTavernAgentRunEvent[] = [];

    return {
        add(event) {
            const key = eventKey(event);
            if (eventsByKey.has(key)) return false;
            eventsByKey.set(key, event);
            events.push(event);
            const previous = events.at(-2);
            if (previous && eventSeq(previous) > eventSeq(event)) {
                events.sort(compareEvents);
            }
            return true;
        },
        events: () => events.slice(),
        oldestSeq: () => {
            const oldest = events[0];
            return oldest ? eventSeq(oldest) : null;
        },
    };
}

function eventKey(event: TauriTavernAgentRunEvent): string {
    const id = typeof event.id === 'string' ? event.id.trim() : '';
    if (!id) throw new Error('Agent run event id is required.');
    return id;
}

function eventSeq(event: TauriTavernAgentRunEvent): number {
    const seq = Number(event.seq);
    if (!Number.isInteger(seq) || seq <= 0) {
        throw new Error('Agent run event seq must be a positive integer.');
    }
    return seq;
}

function compareEvents(left: TauriTavernAgentRunEvent, right: TauriTavernAgentRunEvent): number {
    return eventSeq(left) - eventSeq(right);
}
