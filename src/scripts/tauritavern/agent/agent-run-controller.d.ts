export function getActiveAgentRun(): TauriTavernAgentRunHandle | null;
export function subscribeAgentRunState(listener: (state: {
    activeRun: TauriTavernAgentRunHandle | null;
    lastEvent: TauriTavernAgentRunEvent | null;
}) => void): () => void;
export function subscribeAgentRunEvents(
    listener: (event: TauriTavernAgentRunEvent) => void,
): () => void;
