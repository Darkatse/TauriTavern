// Typed view of model-target-connection.js for TS/TSX consumers. The JS
// module stays the runtime owner; it re-exports the pure converters from
// tauritavern/agent/model-target-llm-connection.js and wires Host access.

/** SillyTavern Connection Manager model target (chat-completion kind). */
export type AgentModelTarget = {
    id: string;
    kind?: string;
    mode?: string;
    name?: string;
    api?: string;
    model?: string;
    'custom-api-format'?: string;
    'api-url'?: string;
    [key: string]: unknown;
};

export function listSavedModelTargets(): AgentModelTarget[];
export function saveModelTargetAsLlmConnection(target: AgentModelTarget): Promise<unknown>;
export function syncSavedModelTargetLlmConnections(): Promise<{ synced: number; failed: unknown[] }>;
export function startModelTargetLlmConnectionSync(): () => void;
export function subscribeModelTargetChanges(listener: () => void): () => void;
export function findModelTargetForBinding(
    modelTargets: readonly AgentModelTarget[],
    model: TauriTavernAgentProfileDefinition['model'] | null | undefined,
): AgentModelTarget | null;
export function modelBindingFromTarget(target: AgentModelTarget): {
    mode: 'connectionRef';
    connectionRef: string;
    modelId: string;
};
export function modelTargetIdFromConnectionRef(connectionRef: string): string;
