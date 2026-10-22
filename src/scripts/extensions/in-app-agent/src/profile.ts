import { ASSISTANT_TOOL_IDS } from './tools';
import { appToolAliases } from '@tauritavern/app-use';

const { snapshot: SNAPSHOT, interact: INTERACT, capture: CAPTURE, evaluate: EVALUATE, readLogs: READ_LOGS } = appToolAliases;

const INSTRUCTIONS = `You are the in-app assistant of TauriTavern, the app the user has open: a SillyTavern-based app for chatting and roleplaying with AI characters. You help the user understand the app, operate it and fix problems in it, from settings, connections and presets to characters, chats, extensions, themes and the interface itself.
This conversation is separate from the user's character chats. When a request concerns a chat or a character, find out which one is open now instead of assuming it.
You work in the live app. ${SNAPSHOT} and ${INTERACT} observe and operate the interface the way the user does. ${CAPTURE}, when available, shows how something looks on screen. ${EVALUATE} runs code in the page with api, the app's public API, for known operations and for state the interface does not show; api.agent.tools.list({ context: 'session' }) lists the tools the assistant can be given, with problems reported by MCP servers. ${READ_LOGS} reads the app's recent logs, and api.dev.frontendLogs turns console capture on or off.
A change to a setting counts once it is saved. After changing one, check that it was saved, not only that it is shown.
If workspace tools are available, use work/ for materials and tmp/ for scratch files. Each conversation has its own files, kept across turns and app restarts.`;

// A first-use form value, not a partially valid profile persisted at startup.
export function createAssistantProfile(): TauriTavernAgentProfileDefinition {
    return {
        schemaVersion: 4,
        kind: 'tauritavern.agentProfile',
        id: 'in-app-assistant',
        displayName: 'In-App Assistant',
        preset: { mode: 'ref', ref: { apiId: 'openai', name: 'Agent' }, required: true },
        model: { mode: 'requiresConfiguration' },
        run: { presentation: 'foreground', stream: true, directRunnable: true, modelRetry: { maxRetries: 3, intervalMs: 3000 } },
        context: { initialChatHistoryMessages: -1, includeActivatedWorldInfo: false },
        delegation: {
            canDelegate: false, canHandoff: false, callable: false, allowAsSubagent: false,
            allowAsHandoffTarget: false, allowNestedDelegation: false, allowedCallers: ['*'],
            descriptionForAgents: null, maxConcurrentInvocations: 3, maxInvocationsPerRun: 8,
            resultBudgetTokens: 8000, maxHandoffDepth: 8,
        },
        instructions: { agentSystemPrompt: INSTRUCTIONS },
        tools: {
            allow: [...ASSISTANT_TOOL_IDS], deny: [], toolDescriptions: {},
            maxRounds: 80, maxCallsPerRun: 80, externalResultInlineCharLimit: 50_000, maxCallsPerTool: {},
        },
        skills: { visible: [], deny: [] },
        workspace: { visibleRoots: ['work', 'tmp'], writableRoots: ['work', 'tmp'] },
        plan: { mode: 'none', beta: true, nodes: [] },
        output: { artifacts: [] },
    };
}
