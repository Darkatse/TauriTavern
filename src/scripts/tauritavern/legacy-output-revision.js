export function buildOutputRevisionRequest(text, guidance) {
    return {
        prompt: `The user would like to revise the reply below. Use apply_patch to make the requested changes, preserving the rest.\n\nUser request:\n${guidance}\n\nCurrent reply:\n${text}`,
        toolData: {
            tools: [{
                type: 'function',
                function: {
                    name: 'apply_patch',
                    description: 'Replace one exact passage in the reply. Patches are applied in order to the text produced by earlier patches.',
                    parameters: {
                        type: 'object',
                        properties: {
                            old_string: { type: 'string', description: 'Exact text to replace. Include enough context to match exactly once.' },
                            new_string: { type: 'string', description: 'Replacement text. Use an empty string to delete the passage.' },
                        },
                        required: ['old_string', 'new_string'],
                        additionalProperties: false,
                    },
                },
            }],
            tool_choice: 'auto',
            enable_web_search: false,
            request_images: false,
        },
    };
}

export function applyOutputPatches(text, calls) {
    if (!Array.isArray(calls) || calls.length === 0) {
        throw new Error('output_revision.no_patch: the model did not return an apply_patch call');
    }
    let result = text;
    for (const [index, call] of calls.entries()) {
        const tool = call?.function;
        if (tool?.name !== 'apply_patch') {
            throw new Error(`output_revision.unknown_tool: patch ${index + 1} must call apply_patch`);
        }
        const args = typeof tool.arguments === 'string' ? JSON.parse(tool.arguments) : tool.arguments;
        const { old_string: oldText, new_string: newText } = args ?? {};
        if (typeof oldText !== 'string' || !oldText || typeof newText !== 'string') {
            throw new Error(`output_revision.invalid_patch: patch ${index + 1} requires a non-empty old_string and a string new_string`);
        }
        const position = result.indexOf(oldText);
        if (position < 0 || position !== result.lastIndexOf(oldText)) {
            throw new Error(`output_revision.match_failed: old_string in patch ${index + 1} must match exactly once`);
        }
        result = result.replace(oldText, () => newText);
    }
    return result;
}
