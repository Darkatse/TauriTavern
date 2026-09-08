export function getActiveAgentRun(): TauriTavernAgentRunHandle | null;
export function resumeAndWaitForAgentRun(input: { runId: string; additionalRounds?: number; checkpoint?: TauriTavernAgentRunCheckpoint; revisionGuidance?: string }, abortController?: EventTarget & { signal: { aborted: boolean } }): Promise<unknown>;
export function subscribeAgentRunState(listener: (state: {
    activeRun: TauriTavernAgentRunHandle | null;
    lastEvent: TauriTavernAgentRunEvent | null;
    presentationError?: string;
}) => void): () => void;
export function subscribeAgentRunEvents(
    listener: (event: TauriTavernAgentRunEvent) => void,
): () => void;
export function retryAgentRunPresentation(runId: string): Promise<void>;
