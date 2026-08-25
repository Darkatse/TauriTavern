import { expect, rs, test } from '@rstest/core';

import {
    type AgentModelTarget,
    type AgentModelTargetChange,
    startModelTargetLlmConnectionSync,
    subscribeModelTargetChanges,
    syncSavedModelTargetLlmConnections,
} from './model-target-connection';

const EVENT_TYPES = Object.freeze({
    MODEL_TARGET_CREATED: 'test_model_target_created',
    MODEL_TARGET_UPDATED: 'test_model_target_updated',
    MODEL_TARGET_DELETED: 'test_model_target_deleted',
});

type EventListener = (...targets: AgentModelTarget[]) => unknown;

class TestEventSource {
    readonly listeners = new Map<string, Set<EventListener>>();

    on(eventName: string, listener: EventListener): void {
        const listeners = this.listeners.get(eventName) ?? new Set<EventListener>();
        listeners.add(listener);
        this.listeners.set(eventName, listeners);
    }

    removeListener(eventName: string, listener: EventListener): void {
        this.listeners.get(eventName)?.delete(listener);
    }

    async emit(eventName: string, ...targets: AgentModelTarget[]): Promise<void> {
        for (const listener of this.listeners.get(eventName) ?? []) {
            await listener(...targets);
        }
    }
}

function sampleTarget(overrides: Partial<AgentModelTarget> = {}): AgentModelTarget {
    return {
        schemaVersion: 1,
        kind: 'tauritavern.modelTarget',
        id: 'Writer Target',
        mode: 'cc',
        name: 'Writer model',
        api: 'custom_claude_messages',
        model: 'claude-3-7-sonnet',
        'api-url': 'https://example.test/v1',
        secretRef: {
            key: 'api_key_custom',
            id: 'secret-custom',
            labelSnapshot: 'Custom key',
        },
        ...overrides,
    };
}

function installHost(targets: AgentModelTarget[]): {
    eventSource: TestEventSource;
    savedConnections: TauriTavernLlmConnectionDefinition[];
    deletedConnections: string[];
    errors: string[];
    restore: () => void;
} {
    const hostDescriptor = Object.getOwnPropertyDescriptor(window, '__TAURITAVERN__');
    const sillyTavernDescriptor = Object.getOwnPropertyDescriptor(window, 'SillyTavern');
    const toastrDescriptor = Object.getOwnPropertyDescriptor(window, 'toastr');
    const eventSource = new TestEventSource();
    const savedConnections: TauriTavernLlmConnectionDefinition[] = [];
    const deletedConnections: string[] = [];
    const errors: string[] = [];

    Object.defineProperty(window, 'SillyTavern', {
        configurable: true,
        value: {
            getContext: () => ({
                extensionSettings: { connectionManager: { modelTargets: targets } },
                eventSource,
                eventTypes: EVENT_TYPES,
            }),
        },
    });
    Object.defineProperty(window, '__TAURITAVERN__', {
        configurable: true,
        value: {
            api: {
                llmConnections: {
                    save: ({ connection }: { connection: TauriTavernLlmConnectionDefinition }) => {
                        savedConnections.push(structuredClone(connection));
                        return Promise.resolve();
                    },
                    delete: ({ connectionId }: { connectionId: string }) => {
                        deletedConnections.push(connectionId);
                        return Promise.resolve();
                    },
                },
            },
        },
    });
    Object.defineProperty(window, 'toastr', {
        configurable: true,
        value: { error: (message: string) => errors.push(message) },
    });

    return {
        eventSource,
        savedConnections,
        deletedConnections,
        errors,
        restore: () => {
            restoreProperty(window, '__TAURITAVERN__', hostDescriptor);
            restoreProperty(window, 'SillyTavern', sillyTavernDescriptor);
            restoreProperty(window, 'toastr', toastrDescriptor);
        },
    };
}

test('Model Target sync and UI changes use the context-owned event source', async () => {
    const target = sampleTarget();
    const updatedTarget = sampleTarget({
        secretRef: {
            key: 'api_key_custom',
            id: 'secret-rotated',
            labelSnapshot: 'Rotated custom key',
        },
    });
    const host = installHost([target]);
    const observedChanges: AgentModelTargetChange[] = [];
    const stopSync = startModelTargetLlmConnectionSync();
    const unsubscribeChanges = subscribeModelTargetChanges(change => observedChanges.push(change));

    try {
        await syncSavedModelTargetLlmConnections();
        expect(host.savedConnections.at(-1)?.auth.secretRef.id).toBe('secret-custom');

        await host.eventSource.emit(EVENT_TYPES.MODEL_TARGET_UPDATED, target, updatedTarget);
        expect(host.savedConnections.at(-1)?.auth.secretRef.id).toBe('secret-rotated');
        expect(observedChanges.at(-1)?.type).toBe('updated');

        await host.eventSource.emit(EVENT_TYPES.MODEL_TARGET_DELETED, updatedTarget);
        expect(observedChanges.at(-1)?.type).toBe('deleted');
        expect(host.deletedConnections).toEqual([]);
        expect(host.errors).toEqual([]);
    } finally {
        unsubscribeChanges();
        stopSync();
        host.restore();
    }
});

test('startup sync invalidates a stale connection when materialization fails', async () => {
    const host = installHost([sampleTarget({ proxy: 'corporate-proxy' })]);
    const warn = rs.spyOn(console, 'warn').mockImplementation(() => undefined);
    try {
        await syncSavedModelTargetLlmConnections();
        expect(host.savedConnections).toEqual([]);
        expect(host.deletedConnections).toEqual(['model-target-writer-target']);
    } finally {
        warn.mockRestore();
        host.restore();
    }
});

function restoreProperty(target: object, key: PropertyKey, descriptor?: PropertyDescriptor): void {
    if (descriptor) Object.defineProperty(target, key, descriptor);
    else Reflect.deleteProperty(target, key);
}
