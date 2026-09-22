import { listSavedModelTargets } from '../../../tauritavern/agent/model-target-llm-connection.js';
import type { createInAppAgentController } from './controller';

const CONTENT_WIDTH = { namespace: 'in-app-agent', key: 'contentWidthPercent' };

export type AssistantController = ReturnType<typeof createInAppAgentController>;
export type ModelTarget = ReturnType<typeof listSavedModelTargets>[number];
export type SettingsOptions = {
    models: ModelTarget[];
    presets: string[];
    tools: TauriTavernAgentToolCatalogItem[];
    diagnostics: Awaited<ReturnType<TauriTavernAgentToolsApi['list']>>['diagnostics'];
    skills: TauriTavernSkillIndexEntry[];
};
export type AssistantContext = {
    eventSource: { once: (event: string, listener: () => void) => void };
    eventTypes: { APP_READY: string };
    shouldSendOnEnter: () => boolean;
    isMobile: () => boolean;
    getPresetManager: (api: string) => { getAllPresets: () => string[]; findPreset: (name: string) => unknown };
};
export function requireContext(): AssistantContext {
    const context = (window as Window & { SillyTavern?: { getContext: () => AssistantContext } }).SillyTavern?.getContext();
    if (!context?.eventSource || !context.eventTypes.APP_READY || !context.shouldSendOnEnter || !context.getPresetManager) {
        throw new Error('in-app-agent: SillyTavern context is unavailable');
    }
    return context;
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
    return {
        skill,
        contentWidthPercent,
        saveContentWidth: (value: number) => store.setJson({ ...CONTENT_WIDTH, value }),
        isMobile: context.isMobile,
        shouldSendOnEnter: context.shouldSendOnEnter,
        copy: bridge.writeClipboardText,
        openLink: bridge.openExternalUrl,
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
                models: listSavedModelTargets(context),
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
