import { createAssistantProfile } from './profile';
import { appendedMessageSeq, mergeMessages, pendingResponses, progressStatus, terminalStatus, updateResponses } from './session-state';
import type { AssistantRun } from './session-state';

type Agent = Pick<TauriTavernAgentApi, 'sessions' | 'readEvents' | 'subscribe' | 'subscribeLiveProjection' | 'cancel'>;
export type AssistantSelection = { load: () => string | null | undefined; save: (sessionId: string | null) => void };
type ActiveRun = AssistantRun & { sessionId: string };
const PAGE_SIZE = 50;

function mergeEvents(read: TauriTavernAgentRunEvent[], observed: TauriTavernAgentRunEvent[]): TauriTavernAgentRunEvent[] {
    return [...new Map([...read, ...observed].map(event => [event.seq, event])).values()].sort((a, b) => a.seq - b.seq);
}

export type AssistantSnapshot = {
    initialized: boolean;
    busy: boolean;
    loading: boolean;
    error: unknown;
    profile: TauriTavernAgentProfileDefinition;
    profileSaved: boolean;
    sessions: TauriTavernAgentSession[];
    sessionId: string | null;
    draft: string;
    sendingSessionId: string | null | undefined;
    activeRun: ActiveRun | null;
    messages: TauriTavernAgentSessionMessage[];
    nextBeforeSeq: number | null;
    run: AssistantRun | null;
    events: TauriTavernAgentRunEvent[];
    responses: TauriTavernAgentRunLiveResponse[];
};

// One owner for Session IO and its drafts. Navigation never owns the native task.
export function createInAppAgentController({ agent, selection }: { agent: Agent; selection: AssistantSelection }) {
    let snapshot: AssistantSnapshot = {
        initialized: false, busy: false, loading: false, error: null,
        profile: createAssistantProfile(), profileSaved: false, sessions: [], sessionId: null, draft: '',
        sendingSessionId: undefined, activeRun: null, messages: [], nextBeforeSeq: null, run: null, events: [], responses: [],
    };
    const listeners = new Set<() => void>();
    const drafts = new Map<string | null, string>();
    const responses = new Map<string, TauriTavernAgentRunLiveResponse>();
    let disposed = false;
    let selectionEpoch = 0;
    let subscriptions: TauriTavernHostUnsubscribe[] = [];
    let subscribedRunId: string | null = null;
    let taskEvents: TauriTavernAgentRunEvent[] = [];
    let refreshing: { epoch: number; requested: boolean; promise: Promise<void> } | null = null;
    let listing: Promise<void> | null = null;
    let listRequested = false;

    function publish(patch: Partial<AssistantSnapshot>): void {
        if (disposed) return;
        snapshot = { ...snapshot, ...patch };
        snapshot.draft = drafts.get(snapshot.sessionId) ?? '';
        snapshot.responses = snapshot.activeRun?.sessionId === snapshot.sessionId
            ? pendingResponses(responses, snapshot.activeRun.runId, snapshot.messages) : [];
        for (const listener of listeners) listener();
    }
    function reportError(error: unknown): void { publish({ error }); }
    function assertOpen(): void { if (disposed) throw new Error('in-app-agent: controller is disposed'); }
    function current(epoch: number): boolean { return !disposed && epoch === selectionEpoch; }
    function remember(): void {
        // A preference failure is visible, but cannot undo a created Session or a send.
        try { selection.save(snapshot.sessionId); } catch (error) { reportError(error); }
    }
    function upsertSession(session: TauriTavernAgentSession): void {
        const sessions = snapshot.sessions.filter(item => item.id !== session.id).concat(session);
        sessions.sort((a, b) => (b.lastUsedAt ?? b.createdAt).localeCompare(a.lastUsedAt ?? a.createdAt) || b.id.localeCompare(a.id));
        publish({ sessions });
    }
    function detach(): void {
        subscribedRunId = null;
        for (const unsubscribe of subscriptions) void Promise.resolve(unsubscribe()).catch(reportError);
        subscriptions = [];
        responses.clear();
    }
    function attach(handle: TauriTavernAgentSessionRunHandle, events: TauriTavernAgentRunEvent[]): void {
        const status = events.map(progressStatus).reverse().find(value => value !== null) ?? handle.status;
        const run: ActiveRun = { ...handle, status, active: true };
        if (subscribedRunId === handle.runId) {
            taskEvents = mergeEvents(events, taskEvents);
            publish({ activeRun: run });
            return;
        }
        detach();
        subscribedRunId = handle.runId;
        taskEvents = events;
        publish({ activeRun: run, ...(snapshot.sessionId === handle.sessionId ? { run, events } : {}) });
        const owns = () => !disposed && subscribedRunId === handle.runId;
        const onError = (error: unknown) => {
            if (!owns()) return;
            detach(); reportError(error);
        };
        subscriptions.push(agent.subscribe(handle.runId, event => {
            if (!owns() || event.seq <= (taskEvents.at(-1)?.seq ?? 0)) return;
            taskEvents = [...taskEvents, event];
            const terminal = terminalStatus(event);
            const progress = progressStatus(event);
            const status = terminal ?? progress ?? snapshot.activeRun?.status ?? handle.status;
            const next = { ...run, status, active: !terminal };
            if (terminal) detach();
            publish({ activeRun: terminal ? null : next,
                ...(snapshot.sessionId === handle.sessionId ? { run: next, events: taskEvents } : {}) });
            if (snapshot.sessionId === handle.sessionId && (terminal || event.type === 'session_message_appended')) {
                void refresh().catch(reportError);
            }
        }, { afterSeq: events.at(-1)?.seq ?? 0, onError }));
        subscriptions.push(agent.subscribeLiveProjection(handle.runId, update => {
            if (!owns()) return;
            updateResponses(responses, update);
            if (snapshot.sessionId === handle.sessionId) publish({});
        }, { onError }));
    }
    async function observe(handle: TauriTavernAgentSessionRunHandle): Promise<void> {
        if (subscribedRunId === handle.runId) return;
        const { events } = await agent.readEvents({ runId: handle.runId, beforeSeq: Number.MAX_SAFE_INTEGER, limit: 100 });
        if (disposed || events.some(event => terminalStatus(event) !== null)) return;
        // Public Session callers retain independent runs; the assistant observes one task.
        if (snapshot.activeRun && snapshot.activeRun.runId !== handle.runId) return;
        attach(handle, events);
    }
    function refreshSessions(): Promise<void> {
        assertOpen();
        listRequested = true;
        listing ??= (async () => {
            do {
                listRequested = false;
                const observedRunId = snapshot.activeRun?.runId;
                const result = await agent.sessions.list();
                if (disposed) return;
                publish({ sessions: result.sessions });
                if (observedRunId && snapshot.activeRun?.runId === observedRunId
                    && !result.activeRuns.some(run => run.runId === observedRunId)) {
                    detach(); publish({ activeRun: null });
                }
                const active = result.activeRuns.find(run => run.runId === snapshot.activeRun?.runId) ?? result.activeRuns[0];
                if (active) await observe(active);
            } while (listRequested && !disposed);
        })().finally(() => { listing = null; });
        return listing;
    }
    async function readLatest(epoch: number): Promise<void> {
        const sessionId = snapshot.sessionId;
        if (!sessionId) return;
        const loadedTail = snapshot.messages.at(-1)?.seq;
        const tail = await agent.sessions.read({ sessionId, limit: PAGE_SIZE });
        if (!current(epoch)) return;
        let incoming = tail.messages;
        let cursor = tail.nextBeforeSeq;
        // A long background tool loop may leave more than one page to catch up.
        while (loadedTail !== undefined && incoming[0] && incoming[0].seq > loadedTail + 1 && cursor !== null) {
            const page = await agent.sessions.read({ sessionId, beforeSeq: cursor, limit: PAGE_SIZE });
            if (!current(epoch)) return;
            incoming = mergeMessages(page.messages, incoming); cursor = page.nextBeforeSeq;
        }
        const wasEmpty = snapshot.messages.length === 0;
        publish({ messages: mergeMessages(snapshot.messages, incoming), ...(wasEmpty ? { nextBeforeSeq: cursor } : {}) });
        const runId = tail.activeRun?.runId ?? snapshot.messages.at(-1)?.runId;
        if (!runId) return;
        const read = await agent.readEvents({ runId, beforeSeq: Number.MAX_SAFE_INTEGER, limit: 100 });
        if (!current(epoch)) return;
        // Subscription delivery can overtake a history read; keep observed progress.
        const events = mergeEvents(read.events, snapshot.events.filter(event => event.runId === runId));
        const terminal = events.map(terminalStatus).reverse().find(status => status !== null);
        const progress = events.map(progressStatus).reverse().find(status => status !== null);
        const knownSeq = Math.max(0, ...events.map(appendedMessageSeq));
        if (knownSeq > (snapshot.messages.at(-1)?.seq ?? 0) && refreshing?.epoch === epoch) refreshing.requested = true;
        const active = tail.activeRun !== null && !terminal;
        publish({ events, run: { runId, active, status: terminal ?? (active ? progress : null) ?? (active ? tail.activeRun?.status : null) ?? 'interrupted' } });
        if (active && tail.activeRun && (!snapshot.activeRun || snapshot.activeRun.runId === runId)) {
            attach(tail.activeRun, events);
        } else if (!active && snapshot.activeRun?.runId === runId) {
            detach(); publish({ activeRun: null });
        }
    }
    function refresh(): Promise<void> {
        assertOpen();
        if (refreshing?.epoch === selectionEpoch) { refreshing.requested = true; return refreshing.promise; }
        const operation = { epoch: selectionEpoch, requested: true, promise: Promise.resolve() };
        refreshing = operation;
        operation.promise = (async () => {
            do { operation.requested = false; await readLatest(operation.epoch); }
            while (operation.requested && current(operation.epoch));
        })().catch(error => { if (current(operation.epoch)) { reportError(error); throw error; } })
            .finally(() => { if (refreshing === operation) refreshing = null; });
        return operation.promise;
    }
    async function selectSession(sessionId: string | null): Promise<void> {
        assertOpen();
        const epoch = ++selectionEpoch;
        publish({ sessionId, messages: [], nextBeforeSeq: null, run: null, events: [], loading: sessionId !== null, error: null });
        remember();
        try { await refresh(); }
        finally { if (current(epoch)) publish({ loading: false }); }
    }
    async function action<T>(work: () => Promise<T>): Promise<T> {
        assertOpen();
        if (snapshot.busy) throw new Error('in-app-agent: another action is in progress');
        publish({ busy: true, error: null });
        try { return await work(); }
        catch (error) { reportError(error); throw error; }
        finally { publish({ busy: false }); }
    }

    return {
        getSnapshot: () => snapshot,
        subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
        initialize: () => action(async () => {
            if (snapshot.initialized) return;
            const [{ profile }] = await Promise.all([agent.sessions.profile.load(), refreshSessions()]);
            const saved = selection.load();
            const sessionId = saved === null ? null : snapshot.sessions.find(session => session.id === saved)?.id ?? snapshot.sessions[0]?.id ?? null;
            publish({ profile: profile ?? createAssistantProfile(), profileSaved: profile !== null });
            await selectSession(sessionId);
            publish({ initialized: true });
        }),
        selectSession,
        newSession: () => selectSession(null),
        setDraft(text: string) {
            assertOpen();
            if (snapshot.sendingSessionId !== undefined && snapshot.sendingSessionId === snapshot.sessionId) return;
            drafts.set(snapshot.sessionId, text); publish({});
        },
        refreshSessions: () => action(refreshSessions),
        renameSession: (sessionId: string, title: string) => action(async () => {
            await agent.sessions.rename({ sessionId, title });
            await refreshSessions();
        }),
        deleteSession: (sessionId: string) => action(async () => {
            if (snapshot.activeRun?.sessionId === sessionId || snapshot.sendingSessionId === sessionId) {
                throw new Error('agent.session_busy: stop the task before deleting its conversation');
            }
            await agent.sessions.delete({ sessionId });
            drafts.delete(sessionId);
            publish({ sessions: snapshot.sessions.filter(session => session.id !== sessionId) });
            if (snapshot.sessionId === sessionId) await selectSession(null);
            await refreshSessions();
        }),
        saveProfile: (profile: TauriTavernAgentProfileDefinition) => action(async () => {
            await agent.sessions.profile.save(profile);
            publish({ profile: structuredClone(profile), profileSaved: true });
        }),
        async send(text = snapshot.draft) {
            assertOpen();
            if (!snapshot.initialized) throw new Error('in-app-agent: initialize before sending');
            if (!snapshot.profileSaved) throw new Error('in-app-agent: configure and save a model before sending');
            if (!text.trim()) throw new Error('in-app-agent: message cannot be empty');
            if (snapshot.sendingSessionId !== undefined || snapshot.activeRun || snapshot.busy) throw new Error('agent.session_busy: the assistant already has a task');
            let sessionId = snapshot.sessionId;
            drafts.set(sessionId, text);
            publish({ sendingSessionId: sessionId, error: null });
            try {
                if (sessionId === null) {
                    const originEpoch = selectionEpoch;
                    const { session } = await agent.sessions.create();
                    sessionId = session.id;
                    drafts.set(sessionId, text); drafts.delete(null);
                    upsertSession(session);
                    publish({ sendingSessionId: sessionId });
                    if (current(originEpoch)) await selectSession(sessionId);
                }
                let handle: TauriTavernAgentSessionRunHandle;
                try { handle = await agent.sessions.send({ sessionId, text }); }
                catch (error) {
                    // Admission can succeed even when its receipt is lost. Never resend.
                    try {
                        await refreshSessions();
                        if (snapshot.sessionId === sessionId) await refresh();
                    } catch (refreshError) {
                        throw new AggregateError([error, refreshError], 'in-app-agent: send failed and Session state could not be refreshed');
                    }
                    throw error;
                }
                drafts.delete(sessionId);
                publish({});
                await observe(handle);
                await refreshSessions();
                if (snapshot.sessionId === sessionId) await refresh();
                return handle;
            } catch (error) { reportError(error); throw error; }
            finally { publish({ sendingSessionId: undefined }); }
        },
        async cancel() {
            assertOpen();
            const runId = snapshot.activeRun?.runId;
            if (!runId) throw new Error('in-app-agent: no active run to cancel');
            try { await agent.cancel(runId); }
            catch (error) { reportError(error); throw error; }
        },
        async loadOlder() {
            assertOpen();
            const epoch = selectionEpoch;
            const { sessionId, nextBeforeSeq } = snapshot;
            if (!sessionId || nextBeforeSeq === null) return;
            try {
                const page = await agent.sessions.read({ sessionId, beforeSeq: nextBeforeSeq, limit: PAGE_SIZE });
                if (current(epoch)) publish({ messages: mergeMessages(page.messages, snapshot.messages), nextBeforeSeq: page.nextBeforeSeq });
            } catch (error) { if (current(epoch)) throw error; }
        },
        refresh: () => action(async () => { await refreshSessions(); await refresh(); }),
        dispose() { disposed = true; detach(); listeners.clear(); },
    };
}

export type InAppAgentController = ReturnType<typeof createInAppAgentController>;
