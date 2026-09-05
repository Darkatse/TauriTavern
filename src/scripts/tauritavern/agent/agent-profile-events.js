// @ts-check

const AGENT_PROFILES_CHANGED = 'tauritavern-agent-profiles-changed';

/** @returns {void} */
export function emitAgentProfilesChanged() {
    window.dispatchEvent(new CustomEvent(AGENT_PROFILES_CHANGED));
}

/**
 * @param {() => void} listener
 * @returns {() => void}
 */
export function subscribeAgentProfilesChanged(listener) {
    const handler = () => listener();
    window.addEventListener(AGENT_PROFILES_CHANGED, handler);
    return () => window.removeEventListener(AGENT_PROFILES_CHANGED, handler);
}
