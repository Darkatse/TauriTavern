import { expect, test } from '@rstest/core';
import { createInAppAgentController } from './controller';
import { createAssistantProfile } from './profile';

type Dependencies = Parameters<typeof createInAppAgentController>[0];

function message(seq: number, role: 'user' | 'assistant' = 'assistant'): TauriTavernAgentSessionMessage {
    return { seq, runId: 'run', createdAt: '', message: { role, parts: [{ type: 'text', text: `message ${seq}` }], providerMetadata: null },
        ...(role === 'assistant' ? { origin: { invocationId: 'inv_root', round: seq } } : {}) };
}

function harness(pointer: unknown = undefined) {
    const profile = createAssistantProfile();
    profile.model = { mode: 'connectionRef', connectionRef: 'model', modelId: 'test' };
    const state = {
        messages: [] as TauriTavernAgentSessionMessage[],
        active: null as TauriTavernAgentSessionRunHandle | null,
        events: [] as TauriTavernAgentRunEvent[],
        pointer, creates: 0, sends: 0, failStore: false, failSend: false,
        event: null as ((event: TauriTavernAgentRunEvent) => void) | null,
        live: null as ((update: TauriTavernAgentRunLiveUpdate) => void) | null,
    };
    const session = { id: 'session', createdAt: '' };
    const agent: Dependencies['agent'] = {
        sessions: {
            profile: { load: () => Promise.resolve({ profile }), save: () => Promise.resolve() },
            create: () => { state.creates++; return Promise.resolve({ session }); },
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
        readEvents: () => Promise.resolve({ events: state.events }),
        subscribe: (_runId, handler) => { state.event = handler; return () => { state.event = null; }; },
        subscribeLiveProjection: (_runId, handler) => { state.live = handler; return () => { state.live = null; }; },
        cancel: () => Promise.resolve({ sessionId: 'session', runId: 'run', status: 'cancelling' }),
    };
    const store: Dependencies['store'] = {
        tryGetJson: () => Promise.resolve(state.pointer === undefined ? { found: false } : { found: true, value: state.pointer }),
        setJson: ({ value }: { value: unknown }) => {
            if (state.failStore) return Promise.reject(new Error('store unavailable'));
            state.pointer = value;
            return Promise.resolve();
        },
    };
    return { controller: createInAppAgentController({ agent, store }), state, agent };
}

const response: TauriTavernAgentRunLiveResponse = {
    invocationId: 'inv_root', invocationExitPolicy: 'reply_allowed', round: 2, attempt: 1,
    text: 'partial', reasoning: 'thinking', toolIds: [],
};

function event(seq: number, type: string): TauriTavernAgentRunEvent {
    return { seq, id: `event${seq}`, runId: 'run', timestamp: '', level: 'info', type };
}

test('Session is lazy, a failed pointer save reuses its ID, and an uncertain send is never repeated', async () => {
    const { controller, state } = harness();
    await controller.initialize();
    expect(state.creates).toBe(0);
    state.failStore = true;
    await expect(controller.send('first')).rejects.toThrow('store unavailable');
    expect(state.sends).toBe(0);
    state.failStore = false;
    state.failSend = true;
    await expect(controller.send('first')).rejects.toThrow('receipt lost');
    expect(state.creates).toBe(1);
    expect(state.sends).toBe(1);
    expect(state.pointer).toBe('session');
    expect(controller.getSnapshot().messages).toHaveLength(1);
    expect(controller.getSnapshot().run?.active).toBe(true);
    await expect(controller.send('again')).rejects.toThrow('busy');
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
    expect(controller.getSnapshot().run).toEqual({ runId: 'run', status: 'cancelled', active: false });
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
