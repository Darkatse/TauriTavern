export function retryAgentRunFailure(input: {
    run?: { runId: string; generationType?: string } | null;
    events?: readonly TauriTavernAgentRunEvent[];
    terminalEvent?: TauriTavernAgentRunEvent | null;
    runtime?: unknown;
}): Promise<unknown>;
