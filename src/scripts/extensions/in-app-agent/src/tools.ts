import { createObservation, interact, snapshotTool, interactTool, captureTool, renderCapture, captureSummary, evaluateTool, readLogsTool, appToolAliases } from '@tauritavern/app-use';
import type { InteractionOptions, InteractionPoint } from '@tauritavern/app-use';
import { CONTROL_SHELL_CLASS } from '../../../legacy-controls.js';

const EXTENSION_ID = 'in-app-agent';
const { snapshot: SNAPSHOT, interact: INTERACT, evaluate: EVALUATE, readLogs: READ_LOGS } = appToolAliases;

export const ASSISTANT_TOOL_IDS = [
    evaluateTool, readLogsTool, snapshotTool, interactTool, captureTool,
].map(tool => `extension/${EXTENSION_ID}:${tool.name}`);

type ToolContext = TauriTavernExtensionToolContext;
type ToolResult = TauriTavernJsonValue | void;
const AsyncFunction = (async () => {}).constructor as new (
    ...args: string[]
) => (api: TauriTavernHostApi, context: ToolContext) => Promise<ToolResult>;

async function nextPaint(): Promise<void> {
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
}

export function createInteractionOptions(scrollChatSurfaceTo: (options: ScrollToOptions) => void): InteractionOptions {
    return {
        surfaceCandidates: element => element.matches(`.${CONTROL_SHELL_CLASS} > .sr-only`) && element.parentElement
            ? [element.parentElement] : [],
        scrollTo: (element, options) => {
            if (element === document.getElementById('chat')) scrollChatSurfaceTo(options);
            else element.scrollTo(options);
        },
    };
}

export async function registerAssistantTools(
    api: TauriTavernHostApi,
    tools: TauriTavernAgentToolsApi,
    logs: TauriTavernFrontendLogsApi,
    isPageReady: () => boolean,
    scrollChatSurfaceTo: (options: ScrollToOptions) => void,
    onInteract?: (point: InteractionPoint, context: ToolContext) => void,
): Promise<void> {
    const observation = createObservation({
        sensitiveSelector: '[data-tt-sensitive]',
        omit: element => element.matches('.ttia-history-area') ? 'assistant conversation and tool logs' : null,
    });
    const interactionOptions = createInteractionOptions(scrollChatSurfaceTo);

    function enterUI(context: ToolContext) {
        if (context.signal.aborted) throw new Error('The operation was cancelled before acting.');
        if (!isPageReady()) {
            throw new Error(`The app is still starting. No UI action was performed. Call ${SNAPSHOT} once startup has finished.`);
        }
        observation.enterRun(context.runId);
    }

    await tools.register({
        extensionId: EXTENSION_ID,
        ...snapshotTool,
        contexts: ['session'],
        enabled: true,
        description: snapshotTool.description + `
The assistant panel is usually open during conversation and can cover other controls. Before operating controls outside it, check whether it is open. If open, use ${INTERACT} to click its top-right close button with the observed ref, then take a fresh snapshot of your target after the panel has closed. Your task continues while the panel is closed.
Assistant conversation and tool logs are omitted from observation.`,
    }, (args, context) => {
        enterUI(context);
        return observation.snapshot(args);
    });

    await tools.register({
        extensionId: EXTENSION_ID,
        ...interactTool,
        contexts: ['session'],
        enabled: true,
    }, (args, context) => {
        enterUI(context);
        return interact(args, observation, {
            ...interactionOptions,
            feedback: point => onInteract?.(point, context),
        });
    });

    await tools.register({
        extensionId: EXTENSION_ID,
        ...captureTool,
        contexts: ['session'],
        enabled: true,
        returnsImages: true,
        description: captureTool.description + `
Gray boxes cover what you cannot observe: this assistant's conversation and sensitive fields. The assistant panel is part of the screen; close it first if it covers what you need to see.`,
    }, async (args, context) => {
        enterUI(context);
        await nextPaint();
        // Do not await between measuring layout and requesting the matching viewport pixels.
        const layout = observation.captureLayout(args);
        const pixels = api.layout!.capture();
        const bitmap = await createImageBitmap(await pixels);
        try {
            const { blob, width, height } = await renderCapture(bitmap, layout);
            context.attachImage(blob);
            return captureSummary(layout, { width, height });
        } finally {
            bitmap.close();
        }
    });

    await tools.register({
        extensionId: EXTENSION_ID,
        ...evaluateTool,
        contexts: ['session'],
        enabled: true,
        description: evaluateTool.description + `
context.signal is an AbortSignal for cooperative cancellation.
This executes in the app page. workspace_shell JavaScript is a separate environment without this window, document or api.`,
    }, async (args, context) => {
        if (typeof args.code !== 'string' || !args.code.trim()) {
            throw new Error(`${EVALUATE}: code must be a non-empty string`);
        }
        observation.enterRun(context.runId);
        observation.clearCursor();
        return await new AsyncFunction('api', 'context', args.code)(api, context);
    });

    await tools.register({
        extensionId: EXTENSION_ID,
        ...readLogsTool,
        contexts: ['session'],
        enabled: true,
        description: readLogsTool.description + `
Returns consoleCaptureEnabled and entries with id, timestampMs, level, message and optional target.
When consoleCaptureEnabled is false, console messages are not captured, but errors and unhandled rejections may still be recorded. Logs and individual messages have retention/size limits; IDs reset on page reload.`,
    }, async ({ limit = 30, levels }) => {
        if (typeof limit !== 'number' || !Number.isSafeInteger(limit) || limit < 1 || limit > 100) {
            throw new Error(`${READ_LOGS}: limit must be an integer between 1 and 100`);
        }
        if (levels !== undefined && (!Array.isArray(levels)
            || levels.some(level => typeof level !== 'string' || !['debug', 'info', 'warn', 'error'].includes(level)))) {
            throw new Error(`${READ_LOGS}: levels must contain only debug, info, warn or error`);
        }
        const selectedLevels = levels === undefined ? null : new Set(levels as string[]);
        const [entries, consoleCaptureEnabled] = await Promise.all([logs.list(), logs.getConsoleCaptureEnabled()]);
        return {
            consoleCaptureEnabled,
            entries: entries.filter(entry => selectedLevels === null || selectedLevels.has(entry.level)).slice(-limit),
        };
    });
}
