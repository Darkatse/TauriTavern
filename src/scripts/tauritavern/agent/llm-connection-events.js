export const LLM_CONNECTIONS_CHANGED = 'tauritavern-llm-connections-changed';

export function emitLlmConnectionsChanged() {
    window.dispatchEvent(new CustomEvent(LLM_CONNECTIONS_CHANGED));
}

export function subscribeLlmConnectionsChanged(listener) {
    const handler = () => listener();
    window.addEventListener(LLM_CONNECTIONS_CHANGED, handler);
    return () => window.removeEventListener(LLM_CONNECTIONS_CHANGED, handler);
}
