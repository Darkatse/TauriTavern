const EXTENSION_ID = 'in-app-agent';

export const ASSISTANT_TOOL_IDS = [
    `extension/${EXTENSION_ID}:app.evaluate`,
    `extension/${EXTENSION_ID}:app.read_logs`,
];

type ToolContext = TauriTavernExtensionToolContext;
type ToolResult = TauriTavernJsonValue | void;
const AsyncFunction = (async () => {}).constructor as new (
    ...args: string[]
) => (api: TauriTavernHostApi, context: ToolContext) => Promise<ToolResult>;

export async function registerAssistantTools(
    api: TauriTavernHostApi,
    tools: TauriTavernAgentToolsApi,
    logs: TauriTavernFrontendLogsApi,
): Promise<void> {
    await tools.register({
        extensionId: EXTENSION_ID,
        name: 'app.evaluate',
        contexts: ['session'],
        enabled: true,
        description: `Execute code once as an async function body in the current application WebView.
Use window/document and the public Host API injected as api (window.__TAURITAVERN__.api).
context provides runId, invocationId, callId, target and signal (AbortSignal). The Session target is not the selected character chat.
Use explicit return for plain JSON results; console.log is not a result. Extract DOM text/attributes/state instead of returning nodes, functions, Error objects or cyclic values. Variables do not persist between calls.
Prefer known public APIs. Inspect a bounded set of elements and verify the target before acting; await operations and independently read the resulting state to verify success.
Keep execution and waits bounded. Cancellation is cooperative via context.signal; a timeout does not stop synchronous JS or undo effects. Do not repeat a mutation whose outcome is unknown.
This is page JavaScript, not the workspace.shell JS environment.`,
        inputSchema: {
            type: 'object', properties: { code: { type: 'string', description: 'Async function body; return a JSON value.' } },
            required: ['code'], additionalProperties: false,
        },
    }, async (args, context) => {
        if (typeof args.code !== 'string' || !args.code.trim()) {
            throw new Error('app.evaluate: code must be a non-empty string');
        }
        return await new AsyncFunction('api', 'context', args.code)(api, context);
    });

    await tools.register({
        extensionId: EXTENSION_ID,
        name: 'app.read_logs',
        contexts: ['session'],
        enabled: true,
        description: `Read recent retained frontend logs, filtering levels before taking the last limit entries.
Returns consoleCaptureEnabled and entries with id, timestampMs, level, message and optional target.
Does not enable console capture or clear logs. Errors/rejections may be recorded while console capture is off. IDs are page-local and reset on reload; retention and individual messages are bounded. Empty logs do not prove an operation succeeded.`,
        inputSchema: {
            type: 'object',
            properties: {
                limit: { type: 'integer', minimum: 1, maximum: 100, default: 30 },
                levels: { type: 'array', items: { type: 'string', enum: ['debug', 'info', 'warn', 'error'] } },
            },
            additionalProperties: false,
        },
    }, async ({ limit = 30, levels }) => {
        if (typeof limit !== 'number' || !Number.isSafeInteger(limit) || limit < 1 || limit > 100) {
            throw new Error('app.read_logs: limit must be an integer between 1 and 100');
        }
        if (levels !== undefined && (!Array.isArray(levels)
            || levels.some(level => typeof level !== 'string' || !['debug', 'info', 'warn', 'error'].includes(level)))) {
            throw new Error('app.read_logs: levels must contain only debug, info, warn or error');
        }
        const selectedLevels = levels === undefined ? null : new Set(levels as string[]);
        const [entries, consoleCaptureEnabled] = await Promise.all([logs.list(), logs.getConsoleCaptureEnabled()]);
        return {
            consoleCaptureEnabled,
            entries: entries.filter(entry => selectedLevels === null || selectedLevels.has(entry.level)).slice(-limit),
        };
    });
}
