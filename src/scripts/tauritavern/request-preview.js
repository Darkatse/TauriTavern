import { characters, Generate, is_send_press, main_api, this_chid, waitForGenerationIdle } from '../../script.js';
import { is_group_generating, selected_group } from '../group-chats.js';
import { createCurrentGenerationParameters, promptManager } from '../openai.js';
import { waitUntilCondition } from '../utils.js';

const SEND_WAIT_TIMEOUT_MS = 1024 * 1024;
const SEND_WAIT_INTERVAL_MS = 100;

/**
 * @typedef {object} RequestPreviewAssembly
 * @property {object} payload
 * @property {import('../openai.js').PromptSource[]} promptSources
 * @property {import('../openai.js').ToolSource[]} toolSources
 * @property {Record<string, string>} promptNames
 * @property {{ included: number, candidates: number }} chatHistory
 * @property {string|null} assemblyError
 * @property {{ code: string, message: string }[]} mcpDiagnostics
 */

/** @type {Promise<object>|null} Shared only until assembly starts. */
let queued = null;

/**
 * @param {(payload: object) => Promise<object>} previewChatCompletion
 * @returns {Promise<object>}
 */
export function previewRequest(previewChatCompletion) {
    queued ??= (async () => {
        try {
            await waitUntilNothingGenerates();
        } finally {
            queued = null;
        }
        const { payload, ...assembly } = await assemble();
        return { ...await previewChatCompletion(payload), ...assembly };
    })();
    return queued;
}

async function waitUntilNothingGenerates() {
    const isSending = () => is_send_press || is_group_generating;
    for (;;) {
        await waitForGenerationIdle();
        if (!isSending()) {
            return;
        }
        await waitUntilCondition(() => !isSending(), SEND_WAIT_TIMEOUT_MS, SEND_WAIT_INTERVAL_MS);
    }
}

/** @returns {Promise<RequestPreviewAssembly>} */
async function assemble() {
    if (main_api !== 'openai') {
        throw new Error('request_preview.chat_completion_required: A request preview needs the Chat Completion API');
    }
    if (selected_group) {
        throw new Error('request_preview.group_unsupported: A request preview does not cover group chats');
    }
    if (!characters[this_chid]) {
        throw new Error('request_preview.chat_required: A request preview needs an open character chat');
    }

    const dryRun = await Generate('normal', {}, true);
    if (!Array.isArray(dryRun?.generateData?.prompt)) {
        throw new Error('request_preview.prompt_missing: The dry run assembled no prompt');
    }
    const { generateData, promptAssembly } = dryRun;
    const { generate_data: payload } = await createCurrentGenerationParameters('normal', generateData.prompt, { toolData: generateData.toolData });

    return {
        // Match the JSON body of a send, including omission of undefined fields.
        payload: JSON.parse(JSON.stringify(payload)),
        promptSources: promptAssembly.sources,
        toolSources: promptAssembly.tools,
        promptNames: getPromptNames(promptAssembly),
        chatHistory: promptAssembly.chatHistory,
        assemblyError: promptAssembly.error,
        mcpDiagnostics: promptAssembly.mcpDiagnostics,
    };
}

/**
 * @param {import('../openai.js').PromptAssembly} promptAssembly
 * @returns {Record<string, string>}
 */
function getPromptNames({ sources, tools }) {
    const pieces = sources.flatMap(source => source.pieces);
    const prompts = [
        ...pieces.filter(piece => piece.type === 'prompt'),
        ...pieces.flatMap(piece => piece.attached ?? []),
        ...tools.filter(tool => tool.type === 'prompt'),
    ];
    const names = {};
    for (const { identifier } of prompts) {
        const name = promptManager.getPromptById(identifier)?.name;
        if (name) {
            names[identifier] = name;
        }
    }
    return names;
}
