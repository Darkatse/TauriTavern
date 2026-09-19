export const RUN_EVENT_PAGE_LIMIT = 240;
export const RUN_EVENT_TAIL_SEQ = Number.MAX_SAFE_INTEGER;

export function createRunTimelineEventStore() {
    const eventKeys = new Set<string>();
    const events: TauriTavernAgentRunEvent[] = [];

    function addMany(incoming: readonly TauriTavernAgentRunEvent[]): boolean {
        let added = false;
        let needsSort = false;
        for (const event of incoming) {
            const key = eventKey(event);
            if (eventKeys.has(key)) continue;
            const seq = eventSeq(event);
            const previous = events.at(-1);
            needsSort ||= previous != null && eventSeq(previous) > seq;
            eventKeys.add(key);
            events.push(event);
            added = true;
        }
        if (needsSort) events.sort(compareEvents);
        return added;
    }

    return {
        addMany,
        events: () => events.slice(),
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
