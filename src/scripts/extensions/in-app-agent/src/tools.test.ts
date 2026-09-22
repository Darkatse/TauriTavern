import { expect, test } from '@rstest/core';
import { registerAssistantTools } from './tools';

test('log levels are filtered before the limit without changing console capture', async () => {
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
    let captureChanges = 0;
    const entries: TauriTavernFrontendLogEntry[] = [
        { id: 1, timestampMs: 1, level: 'error', message: 'first' },
        { id: 2, timestampMs: 2, level: 'warn', message: 'second' },
        { id: 3, timestampMs: 3, level: 'info', message: 'last' },
    ];
    await registerAssistantTools(api, tools, {
        list: () => Promise.resolve(entries),
        getConsoleCaptureEnabled: () => Promise.resolve(false),
        setConsoleCaptureEnabled: () => { captureChanges++; return Promise.resolve(); },
        subscribe: () => Promise.resolve(() => {}),
    });
    const context: TauriTavernExtensionToolContext = {
        runId: 'run', invocationId: 'inv_root', callId: 'call', target: { kind: 'session', sessionId: 'session' },
        signal: new AbortController().signal,
    };
    const logs = handlers.get('app.read_logs');
    if (!logs) throw new Error('log tool not registered');
    expect(await logs({ limit: 1, levels: ['error', 'warn'] }, context)).toEqual({ consoleCaptureEnabled: false, entries: [entries[1]] });
    expect(captureChanges).toBe(0);
});
