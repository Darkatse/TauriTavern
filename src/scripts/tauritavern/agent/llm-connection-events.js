// @ts-check

const LLM_CONNECTIONS_CHANGED = 'tauritavern-llm-connections-changed';

/** @returns {void} */
export function emitLlmConnectionsChanged() {
    window.dispatchEvent(new CustomEvent(LLM_CONNECTIONS_CHANGED));
}

/**
 * @param {() => void} listener
 * @returns {() => void}
 */
export function subscribeLlmConnectionsChanged(listener) {
    const handler = () => listener();
    window.addEventListener(LLM_CONNECTIONS_CHANGED, handler);
    return () => window.removeEventListener(LLM_CONNECTIONS_CHANGED, handler);
}
