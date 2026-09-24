import { createObservation, MAX_DEPTH } from './ui/snapshot';
import { interact } from './ui/interact';
import type { InteractionPoint } from './ui/interact';

const EXTENSION_ID = 'in-app-agent';

export const ASSISTANT_TOOL_IDS = [
    `extension/${EXTENSION_ID}:app.evaluate`,
    `extension/${EXTENSION_ID}:app.read_logs`,
    `extension/${EXTENSION_ID}:app.snapshot`,
    `extension/${EXTENSION_ID}:app.interact`,
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
    isPageReady: () => boolean,
    onInteract?: (point: InteractionPoint, context: ToolContext) => void,
): Promise<void> {
    const observation = createObservation();

    function enterUI(context: ToolContext) {
        if (!isPageReady()) {
            throw new Error('The app is still starting. No UI action was performed. Call app.snapshot once startup has finished.');
        }
        if (context.signal.aborted) {
            throw new Error('The operation was cancelled before acting.');
        }
        observation.enterRun(context.runId);
    }

    await tools.register({
        extensionId: EXTENSION_ID,
        name: 'app.snapshot',
        contexts: ['session'],
        enabled: true,
        description: `Observe the app interface to find controls, inspect their current state, or check the result of an action. Returns a tree of roles, names, text, values and states, with refs such as s42:e27 for app.interact.
Start with {} for an overview of the main page. Open dialogs appear first as summaries. Pass a region's ref as root to inspect its contents; for a standard select, this reveals option labels and exact values. depth controls how many levels are included within that document. A collapsed panel must first be opened with app.interact; increasing depth does not open it.
When you already know a target's CSS selector, pass selector to observe it directly without paging through the whole interface. Without root, it searches the main document. With root, it searches that region's descendants; if root is an accessible iframe, it searches inside that page. A selected iframe is entered the same way. Selectors do not search across other frames. Hidden, zero-size and omitted descendants are excluded; more than one observable match requires a narrower selector or root. After a region is replaced, query its known selector again to obtain fresh refs; old refs are not redirected to new elements.
The assistant panel is usually open during conversation and can cover other controls. Before operating controls outside it, check whether it is open. If open, use app.interact to click its top-right close button with the observed ref, then take a fresh snapshot of your target after the panel has closed. Your task continues while the panel is closed.
An iframe is an embedded page. Pass its ref as root to enter it when its content is accessible; enter nested frames the same way. Use the returned refs directly with app.interact. Call app.snapshot with {} to return to the main page. Unavailable content is marked omitted; a page that is still loading may show only its current content. Observation does not load, reload or wake a paused page. Operate its visible restore button or scroll its containing region when needed, then observe again.
If nextCursor is returned, pass it as cursor alone to continue reading. Every returned page, including a continuation or targeted inspection, replaces the previous refs. Copy refs from the latest returned page; do not construct them or reuse refs from earlier pages. After app.interact or app.evaluate, start a new snapshot instead of continuing an old cursor. Pages read the live interface, so content can change between calls.
Message bodies and reasoning have text previews of up to about 1,000 characters per region, shared across paragraphs and formatting and bounded by a page-wide text budget. Links, buttons and other controls remain observable within depth limits. textTruncated=true marks omitted text; cursor continues interface observation, not the omitted text. Use root to inspect a region's controls; use a known API through app.evaluate to read complete application data, such as chat history.
truncated means some content was shortened or omitted, not necessarily that another page exists. Follow nextCursor when present, use root for children omitted by depth, and treat text marked preview or nameTruncated/valueTruncated as incomplete.
Observation does not scroll or open panels. Listed elements may be off-screen or covered, including by a panel outside an iframe. Call app.snapshot with {} to inspect outer-page obstructions. Frame reloads invalidate refs and pagination within that page; observe the main page and enter again. Assistant conversation/tool logs and known sensitive contents are omitted. Cross-origin or isolated frame contents, shadow DOM and complex-editor contents are not inspected; use a known application API when available. app.evaluate does not bypass browser access restrictions.`,
        inputSchema: {
            type: 'object',
            properties: {
                root: { type: 'string', description: 'Copy a ref from the latest snapshot to inspect a region, enter an accessible iframe, or read a select\'s options. Omit to observe the main page.' },
                selector: { type: 'string', description: 'A known CSS selector, such as #send_textarea, to choose the snapshot root directly. Searches the main document by default, or within root when provided. Requires one observable match; does not pierce other iframes or reveal hidden/omitted contents.' },
                depth: { type: 'integer', minimum: 1, maximum: MAX_DEPTH, default: 2, description: 'Number of interface levels below the root to read. Default 2; layout-only wrappers do not count. Does not expand closed panels.' },
                cursor: { type: 'string', description: 'Copy nextCursor from the last snapshot to continue reading. Pass cursor alone, without root, selector or depth.' },
            },
            additionalProperties: false,
        },
    }, (args, context) => {
        enterUI(context);
        return observation.snapshot(args);
    });

    await tools.register({
        extensionId: EXTENSION_ID,
        name: 'app.interact',
        contexts: ['session'],
        enabled: true,
        description: `Perform one action on an interface control, then read its immediate state. Use a ref from the latest app.snapshot, or a known CSS selector. Provide exactly one of ref or selector. Use app.snapshot first when the target is unclear. For a task with a known application API, or a complex editor these actions do not support, use app.evaluate with that API.
selector locates one observable element at execution time. Without root it searches the main document, even if the latest snapshot is inside an iframe. With root it searches that region's descendants, or the document inside an accessible iframe. Selectors do not search across other frames. root is only accepted with selector. No match or multiple observable matches requires checking the selector or narrowing its scope; the tool never picks the first match or redirects an expired ref. Both addressing methods use the same disabled, read-only, visibility and hit checks.
Choose the action that matches the control:
- click activates a button, link or other clickable element. It needs only action and the target address.
- fill replaces the entire value of an ordinary text/number input or textarea; pass value as a string. It does not append text or fill passwords/files.
- select chooses an enabled option in a standard single-select control. Pass its exact value, not its displayed label; inspect the select with app.snapshot root to find available values and a fresh ref.
- set_checked sets a checkbox or radio to the requested checked state. Prefer it to click when the desired state is known. A radio can be checked, but cannot be unchecked; choose another radio option instead.
- scroll moves a region marked scrollable=true up or down by one screen. Address the scrollable region, not an off-screen child.
For an expired ref or a removed target, observe again. Inside an iframe, use refs returned after entering it with app.snapshot; a reload requires observing and entering again. For an off-screen or covered target, scroll its region or close the covering panel. If the obstruction is outside the iframe, call app.snapshot with {} to inspect the main page. For a disabled or read-only control, inspect its prerequisites or find an editable control; repeating the same action will not help. The tool does not force input, scroll implicitly, send keyboard events or operate system dialogs. Embedded pages with rotation, skew or 3D transforms cannot be mapped reliably; use a known application API for those controls.
dispatched=true means the action was requested, not that its intended result was achieved. false means the checkbox/radio already matched. observed is the target's immediate state; connected=false means it left the page, which may be expected when closing a dialog. Verify the intended result with a new snapshot or a known API before reporting success, including any required save. If a call reports that an action was attempted or timed out, inspect the current state before repeating it.`,
        inputSchema: {
            type: 'object',
            properties: {
                action: { type: 'string', enum: ['click', 'fill', 'select', 'set_checked', 'scroll'], description: 'Choose the operation for this control. Include only the parameters required by that action.' },
                ref: { type: 'string', description: 'Copy the target\'s ref from the latest app.snapshot page, for example s42:e27. Cannot be combined with selector or root.' },
                selector: { type: 'string', description: 'A known CSS selector matching one observable element. Searches the main document unless root is provided. Cannot be combined with ref.' },
                root: { type: 'string', description: 'Optional scope for selector: a ref from the latest snapshot. Searches the region\'s descendants, or the document inside an accessible iframe. Requires selector.' },
                value: { type: 'string', description: 'For fill: the complete replacement text. For select: the exact option value, not its label. Required for those two actions; omit otherwise.' },
                checked: { type: 'boolean', description: 'Desired checkbox/radio state. Required only for set_checked; false is not supported for radios.' },
                direction: { type: 'string', enum: ['up', 'down'], description: 'Scroll direction. Required only for scroll.' },
            },
            required: ['action'],
            additionalProperties: false,
        },
    }, (args, context) => {
        enterUI(context);
        return interact(args, observation, context.signal, point => onInteract?.(point, context));
    });

    await tools.register({
        extensionId: EXTENSION_ID,
        name: 'app.evaluate',
        contexts: ['session'],
        enabled: true,
        description: `Execute JavaScript in the running app to call known APIs, inspect specific data, or handle tasks outside app.snapshot/app.interact's supported controls. Use app.snapshot for general interface exploration and app.interact for ordinary controls when available; use this tool directly when a known API is the appropriate path.
code is an async function body executed once. window and document are available, and api is the public application API (window.__TAURITAVERN__.api). This assistant conversation is separate from the selected character chat; read the current app state when that distinction matters.
Use await for asynchronous work and an explicit return for plain JSON results; console.log is not a result. Extract DOM text, attributes or state instead of returning nodes, functions, Error objects or cyclic values. Local variables do not persist between calls.
Use known API signatures; discovering a method name does not tell you its arguments. Inspect a bounded set of elements and confirm the target before changing it. Independently read the resulting state before reporting success.
Keep execution and waits bounded. context.signal is an AbortSignal for cooperative cancellation. Cancellation or timeout does not stop synchronous JavaScript or undo effects; inspect the affected state before retrying an uncertain operation. Calls invalidate an earlier snapshot's pagination cursor, so start a new snapshot if further UI observation is needed.
This executes in the app page. workspace.shell JavaScript is a separate environment without this window, document or api.`,
        inputSchema: {
            type: 'object', properties: { code: { type: 'string', description: 'An async JavaScript function body. Await operations and explicitly return the JSON data needed to assess their result.' } },
            required: ['code'], additionalProperties: false,
        },
    }, async (args, context) => {
        if (typeof args.code !== 'string' || !args.code.trim()) {
            throw new Error('app.evaluate: code must be a non-empty string');
        }
        observation.enterRun(context.runId);
        observation.clearCursor();
        return await new AsyncFunction('api', 'context', args.code)(api, context);
    });

    await tools.register({
        extensionId: EXTENSION_ID,
        name: 'app.read_logs',
        contexts: ['session'],
        enabled: true,
        description: `Read recent app frontend logs to investigate an error or unexpected behavior. Use app.snapshot or a known API to verify the actual application state; logs alone do not establish success or failure.
Filters levels before taking the last limit matching entries. Returns consoleCaptureEnabled and entries with id, timestampMs, level, message and optional target. Use timestamps and messages to relate entries to the operation being investigated; old or unrelated errors may also be present.
This tool does not enable console capture or clear logs. When consoleCaptureEnabled is false, console messages are not captured, but errors and unhandled rejections may still be recorded. Logs and individual messages have retention/size limits; IDs reset on page reload. Empty logs do not prove an operation succeeded.`,
        inputSchema: {
            type: 'object',
            properties: {
                limit: { type: 'integer', minimum: 1, maximum: 100, default: 30, description: 'Maximum number of recent matching entries to return. Default 30.' },
                levels: { type: 'array', items: { type: 'string', enum: ['debug', 'info', 'warn', 'error'] }, description: 'Include only these levels, for example ["warn", "error"]. Omit to include all levels.' },
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
