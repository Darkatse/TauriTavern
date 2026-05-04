import { translateAgentSystem as tr } from './i18n.js';

export function waitForHostReady() {
    return window.__TAURITAVERN__?.ready
        ?? window.__TAURITAVERN_MAIN_READY__
        ?? Promise.resolve();
}

export function requireHostApi(name) {
    const api = window.__TAURITAVERN__?.api?.[name];
    if (!api) {
        throw new Error(tr('hostApiUnavailable', { name }));
    }
    return api;
}

export function requireAgentApi() {
    const agent = requireHostApi('agent');
    if (!agent.profiles) {
        throw new Error(tr('hostAgentProfileApiUnavailable'));
    }
    return agent;
}

export function requireSkillApi() {
    return requireHostApi('skill');
}

export function requireExtensionStore() {
    const store = window.__TAURITAVERN__?.api?.extension?.store;
    if (!store) {
        throw new Error(tr('hostExtensionStoreUnavailable'));
    }
    return store;
}

export function clone(value) {
    return JSON.parse(JSON.stringify(value));
}

export function prettyJson(value) {
    return JSON.stringify(value, null, 2);
}

export function errorText(error) {
    return String(error?.message || error || tr('unknownError'));
}
