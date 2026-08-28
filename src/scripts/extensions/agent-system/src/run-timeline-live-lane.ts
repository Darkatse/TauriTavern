import type { TimelineItem, TimelineLiveToolId } from './RunTimelineContract';
import { displayToolName } from './run-tool-labels';

// Live projection is a non-authoritative preview: a call appears here only
// while the model is streaming its arguments. Durable journal events remain an
// independent timeline source. This lane owns subscription, projection, and
// per-frame publish coalescing.
//
// Only `run_finish_allowed` calls are presented, so the main lane never shows
// SubAgent internals. The chat consumer separately selects write_file content.

const WRITE_TOOL_ID = 'builtin:workspace.write_file';
const PATCH_TOOL_ID = 'builtin:workspace.apply_patch';
const LIVE_SEQ_BASE = 1_000_000_000;
const TAIL_MAX_CHARS = 420;
const TAIL_MAX_LINES = 3;

type LiveStreamField = 'path' | 'content' | 'oldString' | 'newString';

type LiveLaneCall = {
    invocationId: string;
    toolCallIndex: number;
    toolId: TimelineLiveToolId;
    insertionIndex: number;
    activeField: LiveStreamField | null;
    path: string;
    content: string;
    contentWords: number;
    oldString: string;
    oldStringWords: number;
    newString: string;
    newStringWords: number;
};

export type RunTimelineLiveLaneOptions = {
    subscribeLiveProjection: TauriTavernAgentApi['subscribeLiveProjection'] | undefined;
    scheduleFrame: ((callback: () => void) => void) | undefined;
    onChange: () => void;
    onError: (error: unknown) => void;
};

export type RunTimelineLiveLane = {
    version: () => number;
    items: () => TimelineItem[];
    attach: (runId: string) => void;
    detach: () => void;
    dispose: () => void;
};

export function createRunTimelineLiveLane(options: RunTimelineLiveLaneOptions): RunTimelineLiveLane {
    const subscribe = options.subscribeLiveProjection ?? null;
    const scheduleFrame = options.scheduleFrame ?? defaultScheduleFrame;
    let runId = '';
    let unsubscribe: TauriTavernHostUnsubscribe | null = null;
    const calls = new Map<string, LiveLaneCall>();
    let insertionCounter = 0;
    let storeVersion = 0;
    let frameScheduled = false;
    let disposed = false;

    function publishSoon(): void {
        if (disposed || frameScheduled) return;
        frameScheduled = true;
        scheduleFrame(() => {
            frameScheduled = false;
            if (!disposed) options.onChange();
        });
    }

    function receiveUpdate(update: TauriTavernAgentRunLiveUpdate): void {
        if (disposed) return;
        let changed: boolean;
        switch (update.type) {
            case 'snapshot':
                changed = replaceSnapshot(update.calls);
                break;
            case 'replace':
                changed = upsertCall(update.call);
                break;
            case 'append':
                changed = appendField(update);
                break;
            case 'remove':
                changed = calls.delete(keyOf(update.invocationId, update.toolCallIndex));
                break;
            default:
                throw new Error('agent.timeline_live_update_invalid: unsupported live update');
        }
        if (!changed) return;
        storeVersion += 1;
        publishSoon();
    }

    function replaceSnapshot(snapshot: readonly TauriTavernAgentRunLiveToolCall[]): boolean {
        let changed = resetCalls();
        for (const call of snapshot) changed = upsertCall(call) || changed;
        return changed;
    }

    function upsertCall(call: TauriTavernAgentRunLiveToolCall): boolean {
        if (call.invocationExitPolicy !== 'run_finish_allowed') return false;
        const key = keyOf(call.invocationId, call.toolCallIndex);
        const existing = calls.get(key);
        const fields = {
            path: call.path,
            content: call.toolId === WRITE_TOOL_ID ? call.content : '',
            contentWords: call.toolId === WRITE_TOOL_ID ? call.contentWords : 0,
            oldString: call.toolId === PATCH_TOOL_ID ? call.oldString : '',
            oldStringWords: call.toolId === PATCH_TOOL_ID ? call.oldStringWords : 0,
            newString: call.toolId === PATCH_TOOL_ID ? call.newString : '',
            newStringWords: call.toolId === PATCH_TOOL_ID ? call.newStringWords : 0,
        };
        calls.set(key, {
            invocationId: call.invocationId,
            toolCallIndex: call.toolCallIndex,
            toolId: call.toolId,
            insertionIndex: existing?.insertionIndex ?? insertionCounter++,
            // A full call has no delta arrival order; use the furthest populated
            // field in the tool schema.
            activeField: fields.newString ? 'newString'
                : fields.oldString ? 'oldString'
                    : fields.content ? 'content'
                        : fields.path ? 'path' : null,
            ...fields,
        });
        return true;
    }

    function appendField(update: Extract<TauriTavernAgentRunLiveUpdate, { type: 'append' }>): boolean {
        const key = keyOf(update.invocationId, update.toolCallIndex);
        const call = calls.get(key);
        if (!call) return false;
        const next = { ...call, activeField: update.field };
        if (update.field === 'path') {
            next.path += update.text;
        } else if (update.field === 'content' && call.toolId === WRITE_TOOL_ID) {
            next.content += update.text;
            next.contentWords += update.wordDelta;
        } else if (update.field === 'oldString' && call.toolId === PATCH_TOOL_ID) {
            next.oldString += update.text;
            next.oldStringWords += update.wordDelta;
        } else if (update.field === 'newString' && call.toolId === PATCH_TOOL_ID) {
            next.newString += update.text;
            next.newStringWords += update.wordDelta;
        } else {
            throw new Error('agent.timeline_live_update_invalid: field does not match tool call');
        }
        calls.set(key, next);
        return true;
    }

    function presentCall(call: LiveLaneCall): TimelineItem {
        const isWrite = call.toolId === WRITE_TOOL_ID;
        // The stream follows the field currently arriving: a patch locates the
        // old_string in red first, then switches to the green new_string; a
        // write is a single neutral stream.
        const text = call.activeField === 'content' ? call.content
            : call.activeField === 'newString' ? call.newString
                : call.activeField === 'oldString' ? call.oldString
                    : '';
        const { tail, truncated } = streamTail(text);
        const streamTone = call.activeField === 'newString' ? 'added'
            : call.activeField === 'oldString' ? 'removed'
                : 'neutral';
        return {
            id: `live:${call.invocationId}:${call.toolCallIndex}`,
            seq: LIVE_SEQ_BASE + call.insertionIndex,
            runId,
            type: 'live_tool_call',
            level: 'info',
            timestamp: '',
            icon: isWrite ? 'fa-file-lines' : 'fa-code-commit',
            tone: 'active',
            kind: isWrite ? 'write' : 'patch',
            titleKey: call.path
                ? (isWrite ? 'timelineLiveWriting' : 'timelineLivePatching')
                : 'timelineEventToolRequested',
            titleParams: call.path
                ? { path: call.path }
                : { tool: displayToolName(call.toolId.replace(/^builtin:/u, '')) },
            summary: '',
            rowSpan: 2,
            live: {
                toolId: call.toolId,
                tail,
                truncated,
                streamTone,
                addedWords: isWrite ? call.contentWords : call.newStringWords,
                removedWords: isWrite ? 0 : call.oldStringWords,
            },
        };
    }

    function resetCalls(): boolean {
        const changed = calls.size > 0;
        calls.clear();
        insertionCounter = 0;
        return changed;
    }

    function detachInternal(): void {
        const stop = unsubscribe;
        unsubscribe = null;
        if (stop) void stop();
        if (resetCalls()) {
            storeVersion += 1;
            publishSoon();
        }
    }

    return {
        version: () => storeVersion,
        items: () => [...calls.values()].map(presentCall),
        attach(nextRunId) {
            const normalized = nextRunId.trim();
            if (!normalized) throw new Error('Agent run id is required.');
            detachInternal();
            runId = normalized;
            if (!subscribe || disposed) return;
            unsubscribe = subscribe(normalized, receiveUpdate, { onError: options.onError });
        },
        detach: detachInternal,
        dispose() {
            if (disposed) return;
            disposed = true;
            detachInternal();
        },
    };
}

function defaultScheduleFrame(callback: () => void): void {
    if (typeof requestAnimationFrame === 'function') {
        requestAnimationFrame(callback);
        return;
    }
    setTimeout(callback, 16);
}

function keyOf(invocationId: string, toolCallIndex: number): string {
    return `${invocationId} ${toolCallIndex}`;
}

function streamTail(text: string): { tail: string; truncated: boolean } {
    if (!text) return { tail: '', truncated: false };
    const window = text.length > TAIL_MAX_CHARS ? text.slice(-TAIL_MAX_CHARS) : text;
    const lines = window.split('\n');
    const kept = lines.length > TAIL_MAX_LINES ? lines.slice(-TAIL_MAX_LINES).join('\n') : window;
    const truncated = kept.length < text.length;
    return { tail: truncated ? `…${kept}` : kept, truncated };
}
