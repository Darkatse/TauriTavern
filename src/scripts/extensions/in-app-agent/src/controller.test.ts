import { expect, test } from '@rstest/core';
import { createInAppAgentController } from './controller';
import { createAssistantProfile } from './profile';

type Dependencies = Parameters<typeof createInAppAgentController>[0];

function message(seq: number, role: 'user' | 'assistant' = 'assistant'): TauriTavernAgentSessionMessage {
    return { seq, runId: 'run', createdAt: '', message: { role, parts: [{ type: 'text', text: `message ${seq}` }], providerMetadata: null },
        ...(role === 'assistant' ? { origin: { invocationId: 'inv_root', round: seq } } : {}) };
}

function harness(selectionId?: string | null) {
    const profile = createAssistantProfile();
    profile.model = { mode: 'connectionRef', connectionRef: 'model', modelId: 'test' };
    const state = {
        messages: [] as TauriTavernAgentSessionMessage[],
        active: null as TauriTavernAgentSessionRunHandle | null,
        events: [] as TauriTavernAgentRunEvent[],
        selectionId, creates: 0, sends: 0, failSend: false,
        sessions: [] as TauriTavernAgentSession[],
        event: null as ((event: TauriTavernAgentRunEvent) => void) | null,
        live: null as ((update: TauriTavernAgentRunLiveUpdate) => void) | null,
    };
    const session = { id: 'session', createdAt: '', title: null, lastUsedAt: null };
    if (selectionId === 'session') state.sessions.push(session);
    const agent: Dependencies['agent'] = {
        sessions: {
            profile: { load: () => Promise.resolve({ profile }), save: () => Promise.resolve() },
            create: () => { state.creates++; state.sessions.push(session); return Promise.resolve({ session }); },
            list: () => Promise.resolve({ sessions: state.sessions, activeRuns: state.active ? [state.active] : [] }),
            rename: ({ sessionId, title }) => Promise.resolve({ session: { ...session, id: sessionId, title } }),
            delete: ({ sessionId }) => { state.sessions = state.sessions.filter(item => item.id !== sessionId); return Promise.resolve(); },
            read: ({ beforeSeq, limit = 50 }) => {
                const candidates = state.messages.filter(entry => beforeSeq === undefined || entry.seq < beforeSeq);
                const messages = candidates.slice(-limit);
                return Promise.resolve({ session, messages, lastSeq: state.messages.at(-1)?.seq ?? 0,
                    nextBeforeSeq: candidates.length > limit ? messages[0]?.seq ?? null : null, activeRun: state.active });
            },
            send: () => {
                state.sends++;
                state.messages.push(message(1, 'user'));
                const handle: TauriTavernAgentSessionRunHandle = { sessionId: 'session', runId: 'run', status: 'calling_model' };
                state.active = handle;
                return state.failSend ? Promise.reject(new Error('send receipt lost')) : Promise.resolve(handle);
            },
        },
        readEvents: () => Promise.resolve({ events: [...state.events] }),
        subscribe: (_runId, handler) => { state.event = handler; return () => { state.event = null; }; },
        subscribeLiveProjection: (_runId, handler) => { state.live = handler; return () => { state.live = null; }; },
        cancel: () => Promise.resolve({ sessionId: 'session', runId: 'run', status: 'cancelling' }),
    };
    const selection: Dependencies['selection'] = {
        load: () => state.selectionId,
        save: sessionId => { state.selectionId = sessionId; },
    };
    return { controller: createInAppAgentController({ agent, selection }), state, agent };
}

const response: TauriTavernAgentRunLiveResponse = {
    invocationId: 'inv_root', invocationExitPolicy: 'reply_allowed', round: 2, attempt: 1,
    text: 'partial', reasoning: 'thinking', toolIds: [],
};

function event(seq: number, type: string): TauriTavernAgentRunEvent {
    return { seq, id: `event${seq}`, runId: 'run', timestamp: '', level: 'info', type };
}

test('Session stays lazy and an uncertain send keeps its identity and draft without resending', async () => {
    const { controller, state } = harness();
    await controller.initialize();
    controller.setDraft('first');
    await controller.newSession();
    expect(state.creates).toBe(0);
    expect(controller.getSnapshot().draft).toBe('first');
    state.failSend = true;
    await expect(controller.send()).rejects.toThrow('receipt lost');
    expect(state.creates).toBe(1);
    expect(state.sends).toBe(1);
    expect(state.selectionId).toBe('session');
    expect(controller.getSnapshot().draft).toBe('first');
    expect(controller.getSnapshot().messages).toHaveLength(1);
    expect(controller.getSnapshot().activeRun?.runId).toBe('run');
    await controller.newSession();
    controller.setDraft('another question');
    await expect(controller.send()).rejects.toThrow('busy');
    expect(state.creates).toBe(1);
    controller.dispose();
});

test('canonical origin replaces previews in either delivery order; terminal refresh stops both subscriptions', async () => {
    const { controller, state, agent } = harness();
    await controller.initialize();
    await controller.send('help');
    state.live?.({ type: 'responseReplace', response });
    state.live?.({ type: 'responseAppend', invocationId: 'inv_root', text: ' answer', reasoning: '', toolIds: [] });
    expect(controller.getSnapshot().responses[0]?.text).toBe('partial answer');
    state.live?.({ type: 'responseReplace', response: { ...response, attempt: 2, text: 'retry' } });
    expect(controller.getSnapshot().responses[0]?.text).toBe('retry');
    state.messages.push(message(2));
    await controller.refresh();
    expect(controller.getSnapshot().responses).toEqual([]);
    state.live?.({ type: 'snapshot', calls: [], responses: [response] });
    expect(controller.getSnapshot().responses).toEqual([]);
    state.live?.({ type: 'responseReplace', response: { ...response, round: 3 } });
    expect(controller.getSnapshot().responses).toHaveLength(1);
    let finishSave: (() => void) | undefined;
    agent.sessions.profile.save = () => new Promise<void>(resolve => { finishSave = resolve; });
    const saving = controller.saveProfile(controller.getSnapshot().profile);
    await controller.cancel();
    expect(controller.getSnapshot().run?.active).toBe(true);
    finishSave?.();
    await saving;
    state.active = null;
    state.messages.push(message(3));
    const done = event(4, 'run_cancelled');
    state.events.push(done);
    state.event?.(done);
    await controller.refresh();
    expect(controller.getSnapshot().run).toMatchObject({ runId: 'run', status: 'cancelled', active: false });
    expect(controller.getSnapshot().responses).toEqual([]);
    expect(state.event).toBeNull();
    expect(state.live).toBeNull();
    controller.dispose();
});

test('tail refresh fills a multi-page gap, and inactive history without a terminal event stays interrupted', async () => {
    const { controller, state } = harness('session');
    state.messages = Array.from({ length: 80 }, (_, i) => message(i + 1));
    await controller.initialize();
    expect(controller.getSnapshot().messages[0]?.seq).toBe(31);
    expect(controller.getSnapshot().run?.status).toBe('interrupted');
    expect(state.live).toBeNull();
    state.messages.push(...Array.from({ length: 130 }, (_, i) => message(i + 81)));
    await controller.refresh();
    expect(controller.getSnapshot().messages.map(entry => entry.seq)).toEqual(Array.from({ length: 180 }, (_, i) => i + 31));
    await controller.loadOlder();
    expect(controller.getSnapshot().messages).toHaveLength(210);
    expect(controller.getSnapshot().nextBeforeSeq).toBeNull();
    controller.dispose();
});

test('a reply saved between the history and event reads is caught up before initialization completes', async () => {
    const { controller, state, agent } = harness('session');
    state.messages = [message(1, 'user')];
    state.active = { sessionId: 'session', runId: 'run', status: 'calling_model' };
    const read = agent.sessions.read;
    let finishTail: (() => void) | undefined;
    let signalRead: (() => void) | undefined;
    const requested = new Promise<void>(resolve => { signalRead = resolve; });
    agent.sessions.read = async input => {
        const page = await read(input);
        // Freeze the initial history page while the native run finishes.
        agent.sessions.read = read;
        await new Promise<void>(resolve => { finishTail = resolve; signalRead?.(); });
        return page;
    };
    const initializing = controller.initialize();
    await requested;
    if (!finishTail) throw new Error('initial tail was not requested');
    state.messages.push(message(2));
    state.active = null;
    state.events = [{ ...event(2, 'session_message_appended'), payload: { sessionId: 'session', seq: 2 } }, event(3, 'run_completed')];
    finishTail();
    await initializing;
    expect(controller.getSnapshot().messages.map(entry => entry.seq)).toEqual([1, 2]);
    expect(controller.getSnapshot().run).toEqual({ runId: 'run', status: 'completed', active: false });
    expect(state.live).toBeNull();
    controller.dispose();
});

function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>(done => { resolve = done; });
    return { promise, resolve };
}

function addOtherConversation(agent: Dependencies['agent'], state: ReturnType<typeof harness>['state']) {
    const other = { id: 'other', createdAt: '', title: 'Other conversation', lastUsedAt: null };
    state.sessions.push(other);
    const read = agent.sessions.read;
    agent.sessions.read = input => input.sessionId !== other.id ? read(input) : Promise.resolve({
        session: other, messages: [{ ...message(1, 'user'), runId: 'other-run' }], lastSeq: 1, nextBeforeSeq: null, activeRun: null,
    });
}

test('navigation during send preserves the destination draft and stopping still targets the source run', async () => {
    const { controller, state, agent } = harness('session');
    addOtherConversation(agent, state);
    await controller.initialize();
    controller.setDraft('source question');
    const received = deferred<void>();
    const release = deferred<void>();
    const send = agent.sessions.send;
    agent.sessions.send = async input => {
        received.resolve();
        await release.promise;
        expect(input).toEqual({ sessionId: 'session', text: 'source question' });
        return send(input);
    };
    const sending = controller.send();
    await received.promise;
    await controller.selectSession('other');
    controller.setDraft('next question');
    release.resolve();
    await sending;
    expect(controller.getSnapshot().sessionId).toBe('other');
    expect(controller.getSnapshot().draft).toBe('next question');
    expect(controller.getSnapshot().messages[0]?.runId).toBe('other-run');
    expect(controller.getSnapshot().activeRun?.sessionId).toBe('session');
    let cancelled: string | undefined;
    agent.cancel = runId => { cancelled = runId; return Promise.resolve({ sessionId: 'session', runId, status: 'cancelling' }); };
    await controller.cancel();
    expect(cancelled).toBe('run');
    state.active = null;
    const done = event(2, 'run_cancelled');
    state.events.push(done);
    state.event?.(done);
    expect(controller.getSnapshot().activeRun).toBeNull();
    expect(controller.getSnapshot().draft).toBe('next question');
    await controller.selectSession('session');
    expect(controller.getSnapshot().draft).toBe('');
    controller.dispose();
});

test('an earlier conversation read cannot publish after navigation', async () => {
    const { controller, state, agent } = harness('session');
    addOtherConversation(agent, state);
    await controller.initialize();
    const read = agent.sessions.read;
    const received = deferred<void>();
    const release = deferred<void>();
    agent.sessions.read = async input => {
        const page = await read(input);
        if (input.sessionId === 'session') { received.resolve(); await release.promise; }
        return page;
    };
    const refreshing = controller.refresh();
    await received.promise;
    await controller.selectSession('other');
    release.resolve();
    await refreshing;
    expect(controller.getSnapshot().sessionId).toBe('other');
    expect(controller.getSnapshot().messages[0]?.runId).toBe('other-run');
    expect(controller.getSnapshot().loading).toBe(false);
    controller.dispose();
});

test('a lazy creation receipt does not reverse later navigation back to the blank draft', async () => {
    const { controller, state, agent } = harness();
    addOtherConversation(agent, state);
    await controller.initialize();
    await controller.newSession();
    controller.setDraft('create this conversation');
    const received = deferred<void>();
    const release = deferred<void>();
    const create = agent.sessions.create;
    agent.sessions.create = async () => { received.resolve(); await release.promise; return create(); };
    const sending = controller.send();
    await received.promise;
    await controller.selectSession('other');
    controller.setDraft('keep this draft');
    await controller.newSession();
    release.resolve();
    await sending;
    expect(controller.getSnapshot().sessionId).toBeNull();
    expect(controller.getSnapshot().activeRun?.sessionId).toBe('session');
    expect(state.creates).toBe(1);
    await controller.selectSession('other');
    expect(controller.getSnapshot().draft).toBe('keep this draft');
    controller.dispose();
});

test('late event reads retain newer progress already delivered by the subscription', async () => {
    const { controller, state, agent } = harness('session');
    state.messages = [message(1, 'user')];
    state.active = { sessionId: 'session', runId: 'run', status: 'calling_model' };
    state.events = [{ ...event(1, 'status_changed'), payload: { status: 'calling_model' } }];
    await controller.initialize();
    const received = deferred<void>();
    const release = deferred<void>();
    agent.readEvents = async () => {
        const events = [...state.events];
        received.resolve(); await release.promise;
        return { events };
    };
    const reading = controller.refresh();
    await received.promise;
    const progress = { ...event(2, 'status_changed'), payload: { status: 'dispatching_tool' } };
    state.events.push(progress);
    state.event?.(progress);
    release.resolve();
    await reading;
    expect(controller.getSnapshot().run?.status).toBe('dispatching_tool');
    expect(controller.getSnapshot().activeRun?.status).toBe('dispatching_tool');
    expect(controller.getSnapshot().events.at(-1)).toEqual(progress);
    controller.dispose();
});
