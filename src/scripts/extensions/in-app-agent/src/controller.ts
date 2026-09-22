import { createAssistantProfile } from './profile';
import { appendedMessageSeq, mergeMessages, pendingResponses, progressStatus, terminalStatus, updateResponses } from './session-state';
import type { AssistantRun } from './session-state';

type Agent = Pick<TauriTavernAgentApi, 'sessions' | 'readEvents' | 'subscribe' | 'subscribeLiveProjection' | 'cancel'>;
type Store = Pick<TauriTavernExtensionStoreApi, 'tryGetJson' | 'setJson'>;
const POINTER = { namespace: 'in-app-agent', key: 'currentSessionId' };
const PAGE_SIZE = 50;

export type AssistantSnapshot = {
    initialized: boolean;
    busy: boolean;
    error: unknown;
    profile: TauriTavernAgentProfileDefinition;
    profileSaved: boolean;
    sessionId: string | null;
    messages: TauriTavernAgentSessionMessage[];
    nextBeforeSeq: number | null;
    run: AssistantRun | null;
    events: TauriTavernAgentRunEvent[];
    responses: TauriTavernAgentRunLiveResponse[];
};

// Feature-local external store: it owns Session IO, not composer/settings drafts
// or drawer visibility. A future view keeps this instance while hidden.
export function createInAppAgentController({ agent, store }: { agent: Agent; store: Store }) {
    let snapshot: AssistantSnapshot = {
        initialized: false, busy: false, error: null, profile: createAssistantProfile(), profileSaved: false,
        sessionId: null, messages: [], nextBeforeSeq: null, run: null, events: [], responses: [],
    };
    const listeners = new Set<() => void>();
    const responses = new Map<string, TauriTavernAgentRunLiveResponse>();
    let disposed = false;
    let pointerSaved = false;
    let subscriptions: TauriTavernHostUnsubscribe[] = [];
    let subscribedRunId: string | null = null;
    let refreshing: Promise<void> | null = null;
    let refreshRequested = false;

    function publish(patch: Partial<AssistantSnapshot>): void {
        if (disposed) return;
        snapshot = { ...snapshot, ...patch };
        snapshot.responses = pendingResponses(responses, snapshot.run?.runId, snapshot.messages);
        for (const listener of listeners) listener();
    }

    function reportError(error: unknown): void {
        publish({ error });
    }

    function detach(): void {
        subscribedRunId = null;
        for (const unsubscribe of subscriptions) void Promise.resolve(unsubscribe()).catch(reportError);
        subscriptions = [];
        responses.clear();
    }

    function subscriptionError(error: unknown): void {
        detach();
        reportError(error);
    }

    function attach(runId: string, afterSeq: number): void {
        if (subscribedRunId === runId) return;
        detach();
        subscribedRunId = runId;
        publish({});
        const current = () => !disposed && subscribedRunId === runId;
        const onError = (error: unknown) => { if (current()) subscriptionError(error); };
        subscriptions.push(agent.subscribe(runId, event => {
            if (!current()) return;
            const status = terminalStatus(event);
            const progress = progressStatus(event);
            if (status) detach();
            publish({
                events: [...snapshot.events, event],
                ...(status ? { run: { runId, status, active: false } }
                    : progress ? { run: { runId, status: progress, active: true } } : {}),
            });
            if (status || event.type === 'session_message_appended') {
                void refresh().catch(reportError);
            }
        }, { afterSeq, onError }));
        subscriptions.push(agent.subscribeLiveProjection(runId, update => {
            if (!current()) return;
            updateResponses(responses, update);
            publish({});
        }, { onError }));
    }

    async function readLatest(): Promise<void> {
        const sessionId = snapshot.sessionId;
        if (!sessionId) return;
        const tail = await agent.sessions.read({ sessionId, limit: PAGE_SIZE });
        let incoming = tail.messages;
        let cursor = tail.nextBeforeSeq;
        const loadedTail = snapshot.messages.at(-1)?.seq;
        // Fill a gap after a long tool loop before merging the new tail. lastSeq
        // is read separately by the backend; it is not a message-page cursor.
        while (loadedTail !== undefined && incoming[0] && incoming[0].seq > loadedTail + 1 && cursor !== null) {
            const page = await agent.sessions.read({ sessionId, beforeSeq: cursor, limit: PAGE_SIZE });
            incoming = mergeMessages(page.messages, incoming);
            cursor = page.nextBeforeSeq;
        }
        if (disposed) return;
        const wasEmpty = snapshot.messages.length === 0;
        publish({
            messages: mergeMessages(snapshot.messages, incoming),
            ...(wasEmpty ? { nextBeforeSeq: cursor } : {}),
        });
        const runId = tail.activeRun?.runId ?? snapshot.messages.at(-1)?.runId;
        if (!runId) return;
        const changedRun = snapshot.run?.runId !== runId;
        if (changedRun || subscribedRunId !== runId || !tail.activeRun) {
            const { events } = await agent.readEvents({ runId, beforeSeq: Number.MAX_SAFE_INTEGER, limit: 100 });
            if (disposed) return;
            const terminal = events.map(terminalStatus).reverse().find(status => status !== null);
            const progress = events.map(progressStatus).reverse().find(status => status !== null);
            // Events are read after history. A fast reply can already be saved
            // and terminal here, so catch up before finishing this refresh.
            const knownSeq = Math.max(0, ...events.map(appendedMessageSeq));
            if (knownSeq > (snapshot.messages.at(-1)?.seq ?? 0)) refreshRequested = true;
            if (changedRun || !tail.activeRun || terminal) detach();
            publish({
                events,
                run: { runId, active: tail.activeRun !== null && !terminal,
                    status: terminal ?? (tail.activeRun ? progress : null) ?? tail.activeRun?.status ?? 'interrupted' },
            });
            if (snapshot.run?.active) attach(runId, events.at(-1)?.seq ?? 0);
            else detach();
        }
    }

    function refresh(): Promise<void> {
        if (disposed) return Promise.reject(new Error('in-app-agent: controller is disposed'));
        refreshRequested = true;
        refreshing ??= (async () => {
            do {
                refreshRequested = false;
                await readLatest();
            } while (refreshRequested && !disposed);
        })().finally(() => { refreshing = null; });
        return refreshing;
    }

    async function action<T>(work: () => Promise<T>): Promise<T> {
        if (disposed) throw new Error('in-app-agent: controller is disposed');
        if (snapshot.busy) throw new Error('in-app-agent: another action is in progress');
        publish({ busy: true, error: null });
        try {
            return await work();
        } catch (error) {
            reportError(error);
            throw error;
        } finally {
            publish({ busy: false });
        }
    }

    return {
        getSnapshot: () => snapshot,
        subscribe(listener: () => void) {
            listeners.add(listener);
            return () => { listeners.delete(listener); };
        },
        initialize: () => action(async () => {
            if (snapshot.initialized) return;
            const [{ profile }, pointer] = await Promise.all([agent.sessions.profile.load(), store.tryGetJson(POINTER)]);
            const value: unknown = pointer.value;
            if (pointer.found && (typeof value !== 'string' || !value.trim())) {
                throw new Error('in-app-agent: stored currentSessionId is invalid');
            }
            pointerSaved = pointer.found;
            publish({ profile: profile ?? createAssistantProfile(), profileSaved: profile !== null,
                sessionId: pointer.found ? value as string : null });
            await refresh();
            publish({ initialized: true });
        }),
        saveProfile: (profile: TauriTavernAgentProfileDefinition) => action(async () => {
            await agent.sessions.profile.save(profile);
            publish({ profile: structuredClone(profile), profileSaved: true });
        }),
        send: (text: string) => action(async () => {
            if (!snapshot.initialized) throw new Error('in-app-agent: initialize before sending');
            if (!snapshot.profileSaved) throw new Error('in-app-agent: configure and save a model before sending');
            if (!text.trim()) throw new Error('in-app-agent: message cannot be empty');
            await refreshing;
            if (snapshot.run?.active) throw new Error('agent.session_busy: this Session is already running');
            if (!snapshot.sessionId) {
                const { session } = await agent.sessions.create();
                publish({ sessionId: session.id });
            }
            const sessionId = snapshot.sessionId;
            if (!sessionId) throw new Error('in-app-agent: session creation returned no ID');
            if (!pointerSaved) {
                await store.setJson({ ...POINTER, value: sessionId });
                pointerSaved = true;
            }
            let handle: TauriTavernAgentSessionRunHandle;
            try {
                handle = await agent.sessions.send({ sessionId, text });
            } catch (error) {
                // Sending is not retried: admission may have written history.
                // Read the actual state before the user decides what to do next.
                try { await refresh(); } catch (refreshError) {
                    throw new AggregateError([error, refreshError], 'in-app-agent: send failed and Session state could not be refreshed');
                }
                throw error;
            }
            publish({ run: { runId: handle.runId, status: handle.status, active: true }, events: [] });
            await refresh();
            return handle;
        }),
        async cancel() {
            if (disposed) throw new Error('in-app-agent: controller is disposed');
            if (!snapshot.run?.active) throw new Error('in-app-agent: no active run to cancel');
            try {
                await agent.cancel(snapshot.run.runId);
            } catch (error) {
                reportError(error);
                throw error;
            }
            // Cancellation stays available during other IO. Only the terminal
            // event ends the run; cancelling is an idempotent backend request.
        },
        loadOlder: () => action(async () => {
            await refreshing;
            if (!snapshot.sessionId || snapshot.nextBeforeSeq === null) return;
            const page = await agent.sessions.read({ sessionId: snapshot.sessionId, beforeSeq: snapshot.nextBeforeSeq, limit: PAGE_SIZE });
            publish({ messages: mergeMessages(page.messages, snapshot.messages), nextBeforeSeq: page.nextBeforeSeq });
        }),
        refresh: () => action(refresh),
        dispose() {
            disposed = true;
            detach();
            listeners.clear();
        },
    };
}

export type InAppAgentController = ReturnType<typeof createInAppAgentController>;
