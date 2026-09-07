import type { TimelineItem } from './RunTimelineContract';
import { displayToolLabel, displayToolName } from './run-tool-labels';

// Non-authoritative previews of streaming tool arguments and reasoning.
// Durable journal events remain independent. This lane owns subscription,
// projection, and per-frame publish coalescing.
//
// Only `run_finish_allowed` calls are presented, so the main lane never shows
// SubAgent internals. The chat consumer separately selects write_file content.

const WRITE_TOOL_ID = 'builtin:workspace.write_file';
const PATCH_TOOL_ID = 'builtin:workspace.apply_patch';
const LIVE_SEQ_BASE = 1_000_000_000;
const TAIL_MAX_CHARS = 420;
const TAIL_MAX_LINES = 3;

type LiveStreamField = 'content' | 'oldString' | 'newString';

type LiveLaneState = {
    insertionIndex: number;
    expanded: boolean;
    tail: string;
    truncated: boolean;
};

type LiveLaneReasoning = TauriTavernAgentRunLiveReasoning & LiveLaneState;
type LiveLaneCall = TauriTavernAgentRunLiveToolCall & LiveLaneState & {
    activeField: LiveStreamField | null;
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
    toggleExpanded: (id: string) => void;
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
    const reasoning = new Map<string, LiveLaneReasoning>();
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
                changed = replaceSnapshot(update);
                break;
            case 'reasoningReplace':
                changed = upsertReasoning(update.reasoning);
                break;
            case 'reasoningAppend': {
                const current = reasoning.get(reasoningKey(update.invocationId));
                if (!current) return;
                const preview = streamPreview(current.tail + update.text);
                reasoning.set(reasoningKey(update.invocationId), {
                    ...current, ...preview, truncated: current.truncated || preview.truncated,
                    text: current.text + update.text,
                    toolIds: [...current.toolIds, ...update.toolIds],
                });
                changed = true;
                break;
            }
            case 'reasoningRemove':
                changed = reasoning.delete(reasoningKey(update.invocationId));
                break;
            case 'replace':
                changed = upsertCall(update.call);
                break;
            case 'append':
                changed = appendField(update);
                break;
            case 'remove':
                changed = calls.delete(toolCallKey(update.invocationId, update.toolCallIndex));
                break;
            default:
                throw new Error('agent.timeline_live_update_invalid: unsupported live update');
        }
        if (!changed) return;
        storeVersion += 1;
        publishSoon();
    }

    function replaceSnapshot(snapshot: Extract<TauriTavernAgentRunLiveUpdate, { type: 'snapshot' }>): boolean {
        let changed = resetProjection();
        for (const item of snapshot.reasoning) changed = upsertReasoning(item) || changed;
        for (const call of snapshot.calls) changed = upsertCall(call) || changed;
        return changed;
    }

    function upsertReasoning(item: TauriTavernAgentRunLiveReasoning): boolean {
        if (item.invocationExitPolicy !== 'run_finish_allowed') return false;
        const key = reasoningKey(item.invocationId);
        reasoning.set(key, {
            ...item,
            expanded: false,
            insertionIndex: reasoning.get(key)?.insertionIndex ?? insertionCounter++,
            ...streamPreview(item.text),
        });
        return true;
    }

    function presentReasoning([id, item]: [string, LiveLaneReasoning]): TimelineItem {
        return {
            id,
            seq: LIVE_SEQ_BASE + item.insertionIndex,
            runId,
            type: 'live_reasoning',
            level: 'info',
            timestamp: '',
            icon: 'fa-brain',
            tone: 'active',
            kind: 'reasoning',
            titleKey: 'timelineLiveReasoning',
            titleParams: {},
            summary: '',
            rowSpan: 2,
            live: {
                tail: `${item.truncated ? '…' : ''}${item.tail}`,
                truncated: item.truncated,
                streamTone: 'reasoning',
                expanded: item.expanded,
                blocks: [{ text: item.text, streamTone: 'reasoning' }],
                toolLabel: item.toolIds.map(displayToolLabel).join(' · '),
            },
        };
    }

    function upsertCall(call: TauriTavernAgentRunLiveToolCall): boolean {
        if (call.invocationExitPolicy !== 'run_finish_allowed') return false;
        const key = toolCallKey(call.invocationId, call.toolCallIndex);
        const existing = calls.get(key);
        const isWrite = call.toolId === WRITE_TOOL_ID;
        const activeField = isWrite
            ? (call.content ? 'content' : null)
            : call.newString ? 'newString'
                : call.oldString ? 'oldString' : null;
        const preview = streamPreview(isWrite ? call.content : call.newString || call.oldString);
        calls.set(key, {
            ...call,
            expanded: false,
            insertionIndex: existing?.insertionIndex ?? insertionCounter++,
            activeField,
            ...preview,
        });
        return true;
    }

    function appendField(update: Extract<TauriTavernAgentRunLiveUpdate, { type: 'append' }>): boolean {
        const key = toolCallKey(update.invocationId, update.toolCallIndex);
        const call = calls.get(key);
        if (!call) return false;
        if (update.field === 'path') {
            call.path += update.text;
        } else if (update.field === 'content' && call.toolId === WRITE_TOOL_ID) {
            call.content += update.text;
            call.contentWords += update.wordDelta;
        } else if (update.field === 'oldString' && call.toolId === PATCH_TOOL_ID) {
            call.oldString += update.text;
            call.oldStringWords += update.wordDelta;
        } else if (update.field === 'newString' && call.toolId === PATCH_TOOL_ID) {
            call.newString += update.text;
            call.newStringWords += update.wordDelta;
        } else {
            throw new Error('agent.timeline_live_update_invalid: field does not match tool call');
        }
        if (update.field !== 'path') {
            const continuing = call.activeField === update.field;
            const preview = streamPreview(`${continuing ? call.tail : ''}${update.text}`);
            call.activeField = update.field;
            call.tail = preview.tail;
            call.truncated = (continuing && call.truncated) || preview.truncated;
        }
        return true;
    }

    function presentCall(call: LiveLaneCall): TimelineItem {
        const isWrite = call.toolId === WRITE_TOOL_ID;
        // The stream follows the field currently arriving: a patch locates the
        // old_string in red first, then switches to the green new_string; a
        // write is a single neutral stream.
        const streamTone = call.activeField === 'newString' ? 'added'
            : call.activeField === 'oldString' ? 'removed'
                : 'neutral';
        return {
            id: toolCallKey(call.invocationId, call.toolCallIndex),
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
                tail: `${call.truncated ? '…' : ''}${call.tail}`,
                truncated: call.truncated,
                streamTone,
                expanded: call.expanded,
                blocks: isWrite
                    ? [{ text: call.content, streamTone: 'neutral' }]
                    : [
                        { text: call.oldString, streamTone: 'removed', labelKey: 'timelinePatchOriginal' },
                        { text: call.newString, streamTone: 'added', labelKey: 'timelinePatchReplacement' },
                    ],
                addedWords: isWrite ? call.contentWords : call.newStringWords,
                removedWords: isWrite ? 0 : call.oldStringWords,
            },
        };
    }

    function resetProjection(): boolean {
        const changed = calls.size > 0 || reasoning.size > 0;
        calls.clear();
        reasoning.clear();
        insertionCounter = 0;
        return changed;
    }

    function detachInternal(): void {
        const stop = unsubscribe;
        unsubscribe = null;
        if (stop) void stop();
        if (resetProjection()) {
            storeVersion += 1;
            publishSoon();
        }
    }

    return {
        version: () => storeVersion,
        items: () => [
            ...[...reasoning.entries()].map(presentReasoning),
            ...[...calls.values()].map(presentCall),
        ].sort((a, b) => a.seq - b.seq),
        toggleExpanded(id) {
            const item = calls.get(id) ?? reasoning.get(id);
            if (!item) return; // A completed preview may disappear before the click is delivered.
            item.expanded = !item.expanded;
            storeVersion += 1;
            publishSoon();
        },
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

function toolCallKey(invocationId: string, toolCallIndex: number): string {
    return `live:${invocationId}:${toolCallIndex}`;
}

function reasoningKey(invocationId: string): string {
    return `live:reasoning:${invocationId}`;
}

function streamPreview(text: string): { tail: string; truncated: boolean } {
    if (!text) return { tail: '', truncated: false };
    const window = text.length > TAIL_MAX_CHARS ? text.slice(-TAIL_MAX_CHARS) : text;
    const lines = window.split('\n');
    const kept = lines.length > TAIL_MAX_LINES ? lines.slice(-TAIL_MAX_LINES).join('\n') : window;
    return { tail: kept, truncated: kept.length < text.length };
}
