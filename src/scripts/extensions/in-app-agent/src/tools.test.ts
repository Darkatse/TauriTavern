import { afterEach, expect, rstest, test } from '@rstest/core';
import { registerAssistantTools } from './tools';

afterEach(() => {
    rstest.restoreAllMocks();
    document.body.replaceChildren();
    document.body.style.removeProperty('visibility');
});

async function registerTools(entries: TauriTavernFrontendLogEntry[] = []) {
    type Execute = Parameters<TauriTavernAgentToolsApi['register']>[1];
    const handlers = new Map<string, Execute>();
    const api: TauriTavernHostApi = {};
    const tools = {
        register(definition: TauriTavernExtensionToolDefinition, execute: Execute) {
            handlers.set(definition.name, execute);
            return Promise.resolve();
        },
        setEnabled: () => Promise.resolve(),
        list: () => Promise.resolve({ tools: [], diagnostics: [] }),
    };
    const setConsoleCaptureEnabled = rstest.fn(() => Promise.resolve());
    await registerAssistantTools(api, tools, {
        list: () => Promise.resolve(entries),
        getConsoleCaptureEnabled: () => Promise.resolve(false),
        setConsoleCaptureEnabled,
        subscribe: () => Promise.resolve(() => {}),
    }, () => true, () => undefined);
    const context: TauriTavernExtensionToolContext = {
        runId: 'run', invocationId: 'inv_root', callId: 'call', target: { kind: 'session', sessionId: 'session' },
        signal: new AbortController().signal, attachImage: () => {},
    };
    return { handlers, context, setConsoleCaptureEnabled };
}

test('log levels are filtered before the limit without changing console capture', async () => {
    const entries: TauriTavernFrontendLogEntry[] = [
        { id: 1, timestampMs: 1, level: 'error', message: 'first' },
        { id: 2, timestampMs: 2, level: 'warn', message: 'second' },
        { id: 3, timestampMs: 3, level: 'info', message: 'last' },
    ];
    const { handlers, context, setConsoleCaptureEnabled } = await registerTools(entries);
    const logs = handlers.get('app.read_logs');
    if (!logs) throw new Error('log tool not registered');
    expect(await logs({ limit: 1, levels: ['error', 'warn'] }, context)).toEqual({ consoleCaptureEnabled: false, entries: [entries[1]] });
    expect(setConsoleCaptureEnabled).not.toHaveBeenCalled();
});

test('registered snapshots omit TT secrets and assistant history while keeping public content', async () => {
    document.body.style.visibility = 'visible';
    const rects = [new DOMRect(10, 10, 100, 30)];
    rstest.spyOn(Element.prototype, 'getClientRects').mockReturnValue(
        Object.assign(rects, { item: (index: number) => rects[index] ?? null }),
    );
    document.body.innerHTML = `
        <label>Public field<input value="public-value"></label>
        <input data-tt-sensitive aria-label="API key" value="secret-value">
        <div data-tt-sensitive>secret-content</div>
        <div class="ttia-history-area">assistant-history</div>`;
    const { handlers, context } = await registerTools();
    const snapshot = handlers.get('app.snapshot');
    if (!snapshot) throw new Error('snapshot tool not registered');
    const result = JSON.stringify(await snapshot({}, context));
    expect(result).toContain('Public field');
    expect(result).toContain('public-value');
    expect(result).not.toContain('secret-value');
    expect(result).not.toContain('secret-content');
    expect(result).not.toContain('assistant-history');
});
