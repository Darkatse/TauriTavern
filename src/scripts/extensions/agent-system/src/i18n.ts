import { DEFAULT_MESSAGES as DEFAULT_AGENT_MESSAGES } from './i18n-default-messages';
import { TIMELINE_MESSAGES } from './i18n-timeline-messages';

const PREFIX = 'agent_system.';
const DEFAULT_MESSAGES = Object.freeze({
    ...DEFAULT_AGENT_MESSAGES,
    ...TIMELINE_MESSAGES,
});

export type AgentSystemMessageKey = keyof typeof DEFAULT_MESSAGES;
export type AgentSystemMessageParam = string | number | boolean | null | undefined;
export type AgentSystemMessageParams = Readonly<Record<string, AgentSystemMessageParam>>;
export type AgentSystemTr = (key: AgentSystemMessageKey, params?: AgentSystemMessageParams) => string;

const SKILL_ACTION_KEYS: Readonly<Record<string, AgentSystemMessageKey>> = Object.freeze({
    already_installed: 'skillActionAlreadyInstalled',
    installed: 'skillActionInstalled',
    replaced: 'skillActionReplaced',
    skipped: 'skillActionSkipped',
});

type SillyTavernI18nContext = {
    translate?: (fallback: string, key: string) => string;
};

type SillyTavernWindow = Window & {
    SillyTavern?: { getContext?: () => SillyTavernI18nContext | null };
};

function getSillyTavernContext(): SillyTavernI18nContext | null {
    if (typeof window === 'undefined') {
        return null;
    }
    return (window as SillyTavernWindow).SillyTavern?.getContext?.() ?? null;
}

function formatMessage(message: string, params: AgentSystemMessageParams): string {
    return message.replace(/\{(\w+)}/g, (match, name: string) => (
        Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : match
    ));
}

export const translateAgentSystem: AgentSystemTr = (key, params = {}) => {
    const defaultMessage = DEFAULT_MESSAGES[key];
    const translate = getSillyTavernContext()?.translate;
    const message = typeof translate === 'function'
        ? translate(defaultMessage, `${PREFIX}${key}`)
        : defaultMessage;

    return formatMessage(message, params);
};

export function translateSkillInstallAction(action: string): string {
    const normalized = action.trim();
    const key = SKILL_ACTION_KEYS[normalized];
    return key ? translateAgentSystem(key) : normalized;
}
