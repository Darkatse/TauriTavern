import { listSavedModelTargets, modelTargetSource } from '../../../tauritavern/agent/model-target-llm-connection.js';
import { getOmittedParams } from '../../../tauri/generation-params/omission.js';
import type { createInAppAgentController } from './controller';
import { tr } from './i18n';

const CONTENT_WIDTH = { namespace: 'in-app-agent', key: 'contentWidthPercent' };
const SELECTION_KEY = 'tauritavern:in_app_agent_current_session';

// Session data is local to this device, so its navigation preference is too.
export const assistantSelection = {
    load(): string | null | undefined {
        const raw = localStorage.getItem(SELECTION_KEY);
        if (raw === null) return undefined;
        const value: unknown = JSON.parse(raw);
        if (value !== null && (typeof value !== 'string' || !value.trim())) {
            throw new Error('in-app-agent: stored selection must be a Session ID or null');
        }
        return value;
    },
    save(id: string | null): void { localStorage.setItem(SELECTION_KEY, JSON.stringify(id)); },
};

export type AssistantController = ReturnType<typeof createInAppAgentController>;
export type ModelTarget = ReturnType<typeof listSavedModelTargets>[number];
export type SettingsOptions = {
    presets: string[];
    tools: TauriTavernAgentToolCatalogItem[];
    diagnostics: Awaited<ReturnType<TauriTavernAgentToolsApi['list']>>['diagnostics'];
    skills: TauriTavernSkillIndexEntry[];
};
const MODEL_TARGET_EVENTS = ['MODEL_TARGET_CREATED', 'MODEL_TARGET_UPDATED', 'MODEL_TARGET_DELETED'] as const;
export type AssistantContext = {
    eventSource: {
        once: (event: string, listener: () => void) => void;
        on: (event: string, listener: () => void) => void;
        removeListener: (event: string, listener: () => void) => void;
    };
    eventTypes: Record<'APP_READY' | typeof MODEL_TARGET_EVENTS[number], string>;
    shouldSendOnEnter: () => boolean;
    isMobile: () => boolean;
    getPresetManager: (api: string) => {
        getAllPresets: () => string[];
        findPreset: (name: string) => unknown;
        getCompletionPresetByName: (name: string) => { reasoning_effort?: string; extensions?: Record<string, unknown> } | undefined;
    };
    Popup: {
        show: { confirm: (title: string, message: string) => Promise<unknown> };
        util: { getTopmostModalLayer: () => HTMLElement };
    };
    POPUP_RESULT: { AFFIRMATIVE: unknown };
};
export function requireContext(): AssistantContext {
    const context = (window as Window & { SillyTavern?: { getContext: () => AssistantContext } }).SillyTavern?.getContext();
    if (!context?.eventSource?.on || !context.eventSource.removeListener || !context.eventTypes.APP_READY
        || MODEL_TARGET_EVENTS.some(name => !context.eventTypes[name]) || !context.shouldSendOnEnter || !context.getPresetManager) {
        throw new Error('in-app-agent: SillyTavern context is unavailable');
    }
    return context;
}

function createModelTargets(context: AssistantContext) {
    const listeners = new Set<() => void>();
    let targets = listSavedModelTargets(context);
    const update = () => {
        targets = listSavedModelTargets(context);
        listeners.forEach(listener => listener());
    };
    return {
        getSnapshot: () => targets,
        subscribe: (listener: () => void) => {
            if (listeners.size === 0) {
                targets = listSavedModelTargets(context);
                MODEL_TARGET_EVENTS.forEach(name => context.eventSource.on(context.eventTypes[name], update));
            }
            listeners.add(listener);
            return () => {
                listeners.delete(listener);
                if (listeners.size === 0) MODEL_TARGET_EVENTS.forEach(name => context.eventSource.removeListener(context.eventTypes[name], update));
            };
        },
    };
}

export async function createAssistantActions(api: TauriTavernHostApi, context: AssistantContext) {
    if (!api.agent || !api.skill || !api.extension) throw new Error('in-app-agent: Agent, Skill and Extension APIs are required');
    const agent = api.agent;
    const skill = api.skill;
    const store = api.extension.store;
    // These modules belong to the page, not to a second bundled host runtime.
    const libraryUrl = '/lib.js';
    const bridgeUrl = '/tauri-bridge.js';
    const [lib, bridge, widthSetting] = await Promise.all([
        import(libraryUrl /* webpackIgnore: true */) as Promise<{
            showdown: { Converter: new (options: Record<string, boolean>) => { makeHtml: (text: string) => string } };
            DOMPurify: { sanitize: (html: string, options: Record<string, unknown>) => string };
        }>,
        import(bridgeUrl /* webpackIgnore: true */) as Promise<{
            writeClipboardText: (text: string) => Promise<void>;
            openExternalUrl: (url: string) => Promise<void>;
        }>,
        store.tryGetJson(CONTENT_WIDTH),
    ]);
    const contentWidthPercent: unknown = widthSetting.found ? widthSetting.value : 100;
    if (typeof contentWidthPercent !== 'number' || !Number.isInteger(contentWidthPercent) || contentWidthPercent < 40 || contentWidthPercent > 100) {
        throw new Error('in-app-agent: stored contentWidthPercent must be an integer from 40 to 100');
    }
    const converter = new lib.showdown.Converter({ tables: true, strikethrough: true, simpleLineBreaks: false });
    // SillyTavern's preset panel declares which sources accept a reasoning effort.
    const effortSources = document.getElementById('openai_reasoning_effort_block')?.dataset.source?.split(',');
    if (!effortSources) throw new Error('in-app-agent: reasoning effort sources are unavailable');
    return {
        skill,
        models: createModelTargets(context),
        supportsReasoningEffort: (target: ModelTarget) => effortSources.includes(modelTargetSource(target)),
        presetReasoningEffort(name: string): string | null {
            const manager = context.getPresetManager('openai');
            // Checked first because SillyTavern logs an error for unknown names.
            if (!manager.getAllPresets().includes(name)) return null;
            const preset = manager.getCompletionPresetByName(name);
            return !preset?.reasoning_effort || getOmittedParams(preset).includes('reasoning_effort') ? 'auto' : preset.reasoning_effort;
        },
        contentWidthPercent,
        saveContentWidth: (value: number) => store.setJson({ ...CONTENT_WIDTH, value }),
        isMobile: context.isMobile,
        shouldSendOnEnter: context.shouldSendOnEnter,
        copy: bridge.writeClipboardText,
        openLink: bridge.openExternalUrl,
        async confirmDeleteSession(title: string): Promise<boolean> {
            if (!context.Popup?.show.confirm || !context.POPUP_RESULT) throw new Error('in-app-agent: confirmation dialog is unavailable');
            const message = document.createElement('p');
            message.textContent = tr('deleteConversationNote', { title });
            return await context.Popup.show.confirm(tr('deleteConversation'), message.outerHTML) === context.POPUP_RESULT.AFFIRMATIVE;
        },
        markdown: (text: string) => lib.DOMPurify.sanitize(converter.makeHtml(text), {
            USE_PROFILES: { html: true }, FORBID_TAGS: ['img', 'video', 'audio', 'iframe', 'style', 'form', 'input', 'button'],
        }),
        async loadOptions(profileId: string): Promise<SettingsOptions> {
            const manager = context.getPresetManager('openai');
            if (!manager) throw new Error('in-app-agent: preset manager is unavailable');
            const [catalog, skills] = await Promise.all([
                agent.tools.list({ context: 'session' }), skill.list({ scope: { kind: 'profile', profileId } }),
            ]);
            return {
                presets: manager.getAllPresets().filter(name => name && manager.findPreset(name) !== 'gui').sort((a, b) => a.localeCompare(b)),
                tools: catalog.tools, diagnostics: catalog.diagnostics, skills,
            };
        },
        readResult: (runId: string, path: string) => agent.readWorkspaceFile({ runId, path }),
        openConnections() {
            const toggle = document.querySelector<HTMLElement>('#sys-settings-button .drawer-toggle');
            if (!toggle) throw new Error('in-app-agent: connection settings entry is unavailable');
            const drawer = document.getElementById('rm_api_block');
            if (!drawer?.classList.contains('openDrawer')) toggle.click();
        },
    };
}
export type AssistantActions = Awaited<ReturnType<typeof createAssistantActions>>;
