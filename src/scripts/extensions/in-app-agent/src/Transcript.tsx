import { useLayoutEffect, useRef, useState } from 'react';
import type { AssistantSnapshot } from './controller';
import type { AssistantActions, AssistantController } from './host';
import { CopyButton, Disclosure, ErrorNotice, Icon, Markdown, formatDuration, formatTick, useNow } from './components';
import { tr, type MessageKey } from './i18n';

type Part = TauriTavernAgentModelContentPart;
type Call = Extract<Part, { type: 'toolCall' }>['call'];
type Result = Extract<Part, { type: 'toolResult' }>['result'];
const toolLabels: Record<string, [MessageKey, MessageKey]> = {
    'extension/in-app-agent:app.evaluate': ['evaluate', 'evaluateHelp'],
    'extension/in-app-agent:app.read_logs': ['logs', 'logsHelp'],
    'extension/in-app-agent:app.snapshot': ['snapshot', 'snapshotHelp'],
    'extension/in-app-agent:app.interact': ['interact', 'interactHelp'],
    'builtin:workspace.read_file': ['readFile', 'readFileHelp'],
    'builtin:workspace.write_file': ['writeFile', 'writeFileHelp'],
    'builtin:workspace.list_files': ['listFiles', 'listFilesHelp'],
    'builtin:workspace.search_files': ['searchFiles', 'searchFilesHelp'],
    'builtin:workspace.apply_patch': ['applyPatch', 'applyPatchHelp'],
    'builtin:workspace.shell': ['shell', 'shellHelp'],
};
export function toolName(id: string, fallback?: string) {
    const labels = toolLabels[id];
    return labels ? tr(labels[0]) : fallback || id.replace(/^(builtin:|extension\/)/, '');
}
export function toolDescription(tool: TauriTavernAgentToolCatalogItem) {
    const labels = toolLabels[tool.id];
    return labels ? tr(labels[1]) : tool.description;
}
function record(value: unknown): Record<string, unknown> {
    return typeof value === 'object' && value !== null ? value as Record<string, unknown> : {};
}
function shortText(value: unknown): string | null {
    if (typeof value !== 'string') return null;
    const text = value.replace(/\s+/g, ' ').trim();
    return text || null;
}
const detailKeys = ['command', 'query', 'path', 'url', 'input', 'name', 'code'];
function toolDetail(call: Call): string | null {
    const args = record(call.arguments);
    if (call.toolId === 'extension/in-app-agent:app.read_logs' && Array.isArray(args.levels)) return shortText(args.levels.join(', '));
    for (const key of detailKeys) {
        const candidate = shortText(args[key]);
        if (candidate) return candidate;
    }
    return null;
}
export function responseKey(runId: string, invocationId: string, round: number) { return `${runId}:${invocationId}:${round}`; }
function messageKey(entry: TauriTavernAgentSessionMessage) {
    return entry.origin ? responseKey(entry.runId, entry.origin.invocationId, entry.origin.round) : `history:${entry.seq}`;
}

type RunBlock = {
    runId: string;
    userEntries: TauriTavernAgentSessionMessage[];
    workEntries: TauriTavernAgentSessionMessage[];
    finalEntry: TauriTavernAgentSessionMessage | null;
    responses: TauriTavernAgentRunLiveResponse[];
    active: boolean;
    startedAt: number | null;
    endedAt: number | null;
    toolCalls: number;
};
const runTerminalEvents = ['run_completed', 'run_failed', 'run_cancelled', 'run_partial_success'];
function parseTime(value: string): number | null {
    const time = Date.parse(value);
    return Number.isFinite(time) ? time : null;
}
function buildRunBlocks(snapshot: AssistantSnapshot, lastMessageByRun: Map<string, TauriTavernAgentSessionMessage>): RunBlock[] {
    const blocks: RunBlock[] = [];
    for (const entry of snapshot.messages) {
        let block = blocks.at(-1);
        if (!block || block.runId !== entry.runId) {
            block = { runId: entry.runId, userEntries: [], workEntries: [], finalEntry: null, responses: [], active: false, startedAt: null, endedAt: null, toolCalls: 0 };
            blocks.push(block);
        }
        const at = parseTime(entry.createdAt);
        if (at !== null) {
            block.startedAt ??= at;
            block.endedAt = at;
        }
        const { message } = entry;
        if (message.role === 'user') { block.userEntries.push(entry); continue; }
        block.toolCalls += message.parts.filter(part => part.type === 'toolCall').length;
        if (message.role === 'assistant' && lastMessageByRun.get(entry.runId) === entry && !message.parts.some(part => part.type === 'toolCall')) {
            block.finalEntry = entry;
        } else {
            block.workEntries.push(entry);
        }
    }
    const runId = snapshot.run?.runId;
    if (runId) {
        let block = blocks.at(-1);
        if (!block || block.runId !== runId) {
            block = { runId, userEntries: [], workEntries: [], finalEntry: null, responses: [], active: false, startedAt: null, endedAt: null, toolCalls: 0 };
            blocks.push(block);
        }
        block.responses = snapshot.responses;
        block.active = Boolean(snapshot.run?.active);
        const startedEvent = snapshot.events.find(item => item.type === 'run_started');
        const terminalEvent = [...snapshot.events].reverse().find(item => runTerminalEvents.includes(item.type));
        block.startedAt = (startedEvent ? parseTime(startedEvent.timestamp) : null) ?? block.startedAt;
        if (terminalEvent) block.endedAt = parseTime(terminalEvent.timestamp) ?? block.endedAt;
    }
    return blocks.filter(block => block.userEntries.length + block.workEntries.length + block.responses.length > 0 || block.finalEntry !== null);
}

function RunBar({ block, expanded, onToggle }: { block: RunBlock; expanded: boolean; onToggle: () => void }) {
    const ticking = block.active && block.startedAt !== null;
    const now = useNow(1000, ticking);
    const durationMs = block.startedAt !== null ? Math.max(0, (block.active ? now : block.endedAt ?? block.startedAt) - block.startedAt) : null;
    const parts: string[] = [];
    if (durationMs !== null) parts.push(tr('workedFor', { duration: formatDuration(durationMs) }));
    if (block.toolCalls > 0) parts.push(tr('calls', { count: block.toolCalls }));
    return <button type="button" className={`ttia-run-bar${block.active ? ' is-active' : ''}`} aria-expanded={expanded} data-assistant-disclosure onClick={onToggle}>
        <Icon name="chevron-right" /><span>{parts.length > 0 ? parts.join(' · ') : tr('working')}</span><span className="ttia-run-line" />
    </button>;
}

function ToolResult({ result, runId, actions }: { result: Result; runId: string; actions: AssistantActions }) {
    const [full, setFull] = useState<string | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<unknown>(null);
    const structured = record(result.structured);
    const path = structured.externalized === true && typeof structured.path === 'string' ? structured.path : null;
    const content = full ?? result.content;
    return <div className="ttia-tool-result">
        <div className="ttia-detail-label">{tr('result')}<CopyButton text={content} copy={actions.copy} /></div>
        <pre>{content}</pre>
        {path && full === null && <button type="button" disabled={loading} onClick={() => {
            setLoading(true); setError(null);
            void actions.readResult(runId, path).then(file => setFull(file.text)).catch(setError).finally(() => setLoading(false));
        }}>{loading ? tr('loading') : tr('fullResult')}</button>}
        {result.resourceRefs.length > 0 && <div className="ttia-refs">{result.resourceRefs.join('\n')}</div>}
        {error != null && <ErrorNotice error={error} />}
    </div>;
}
function ToolCall({ call, result, runId, origin, events, active, actions }: {
    call: Call; result: Result | undefined; runId: string; origin: TauriTavernAgentSessionMessage['origin']; events: TauriTavernAgentRunEvent[]; active: boolean; actions: AssistantActions;
}) {
    const matches = (item: TauriTavernAgentRunEvent, types: string[]) => Boolean(origin && item.runId === runId && types.includes(item.type)
        && record(item.payload).callId === call.callId && record(item.payload).round === origin.round
        && record(item.payload).invocationId === origin.invocationId);
    const event = events.find(item => matches(item, ['tool_call_completed', 'tool_call_failed']));
    const requested = events.find(item => matches(item, ['tool_call_requested']));
    const elapsed = record(event?.payload).elapsedMs;
    const failed = result?.isError || event?.type === 'tool_call_failed';
    const done = Boolean(result || event);
    const running = active && !done;
    const startedAt = requested ? Date.parse(requested.timestamp) : NaN;
    const ticking = running && Number.isFinite(startedAt);
    const now = useNow(1000, ticking);
    const stateText = typeof elapsed === 'number' ? `${(elapsed / 1000).toFixed(1)}s`
        : ticking ? formatTick(now - startedAt)
        : tr(done ? (failed ? 'failed' : 'done') : running ? 'working' : 'pending');
    const detail = toolDetail(call);
    const args = typeof call.arguments === 'string' ? call.arguments : JSON.stringify(call.arguments, null, 2);
    const code = record(call.arguments).code;
    return <Disclosure className={`ttia-tool ${failed ? 'has-error' : done ? 'is-done' : running ? 'is-running' : ''}`} label={<>
        <span className="ttia-tile"><Icon name={failed ? 'circle-exclamation' : done ? 'check' : running ? 'circle-notch' : 'minus'} /></span>
        <span className="ttia-tool-name">{toolName(call.toolId)}</span>
        {detail && <span className="ttia-detail-pill" title={detail}>{detail}</span>}
        <span className="ttia-tool-state">{stateText}</span>
    </>}>
        <div className="ttia-tool-detail">
            <div className="ttia-detail-label">{tr(typeof code === 'string' ? 'code' : 'arguments')}
                <CopyButton text={typeof code === 'string' ? code : args} copy={actions.copy} /></div>
            <pre>{typeof code === 'string' ? code : args}</pre>
            {result && <ToolResult result={result} runId={runId} actions={actions} />}
        </div>
    </Disclosure>;
}
function AssistantMessage({ parts, results, runId, origin, scope, events, active, finalReply, streaming = false, actions }: {
    parts: Part[]; results: Map<string, Result>; runId: string; origin: TauriTavernAgentSessionMessage['origin']; scope: string;
    events: TauriTavernAgentRunEvent[]; active: boolean; finalReply?: boolean; streaming?: boolean; actions: AssistantActions;
}) {
    const text = parts.filter(part => part.type === 'text').map(part => part.text).join('\n');
    // Stored part order is provider data; reasoning has a fixed place in the UI.
    const displayParts = [...parts.filter(part => part.type === 'reasoning'), ...parts.filter(part => part.type !== 'reasoning')];
    const lastTextIndex = displayParts.reduce((last, part, index) => part.type === 'text' ? index : last, -1);
    return <article className="ttia-reply">
        {displayParts.map((part, index) => {
            if (part.type === 'text') return <Markdown key={index} text={part.text} actions={actions} streaming={streaming && index === lastTextIndex} />;
            if (part.type === 'reasoning') return part.text ? <Disclosure key={index} className="ttia-reasoning"
                label={streaming ? <span className="ttia-shimmer">{tr('thinking')}</span> : tr('reasoning')}>
                <Markdown text={part.text} actions={actions} /></Disclosure> : null;
            if (part.type === 'toolCall') {
                if (displayParts[index - 1]?.type === 'toolCall') return null;
                const calls: Call[] = [];
                for (const item of displayParts.slice(index)) {
                    if (item.type !== 'toolCall') break;
                    calls.push(item.call);
                }
                const tools = calls.map(call => <ToolCall key={call.callId} call={call} result={results.get(`${scope}:${call.callId}`)}
                    runId={runId} origin={origin} events={events} active={active} actions={actions} />);
                const failed = calls.filter(call => results.get(`${scope}:${call.callId}`)?.isError).length;
                return calls.length > 1 ? <Disclosure key={index} className="ttia-tools" label={<>
                    <span>{tr('calls', { count: calls.length })}</span>{failed > 0 && <span className="ttia-danger">{tr('toolFailed', { count: failed })}</span>}
                </>}>{tools}</Disclosure> : tools;
            }
            if (part.type === 'toolResult') return null;
            return <p className="ttia-muted" key={index}>{tr('unsupported')}</p>;
        })}
        {finalReply && !active && text && <CopyButton text={text} copy={actions.copy} />}
    </article>;
}

export function Transcript({ snapshot, controller, actions, visible }: {
    snapshot: AssistantSnapshot; controller: AssistantController; actions: AssistantActions; visible: boolean;
}) {
    const scroller = useRef<HTMLDivElement>(null);
    const following = useRef(true);
    const [away, setAway] = useState(false);
    const [loadingOlder, setLoadingOlder] = useState(false);
    const [pageError, setPageError] = useState<unknown>(null);
    const [initialSeq] = useState(() => snapshot.messages.at(-1)?.seq ?? 0);
    const lastMessageSeq = useRef(snapshot.messages.at(-1)?.seq ?? 0);
    const anchor = useRef<{ seq: string; offset: number; firstSeq: number } | null>(null);
    const lastMessageByRun = new Map(snapshot.messages.map(entry => [entry.runId, entry]));
    const [expandOverride, setExpandOverride] = useState<ReadonlyMap<string, boolean>>(new Map());
    const results = new Map<string, Result>();
    const calls = new Set<string>();
    const lastCall = new Map<string, string>();
    const paired = new Set<Result>();
    for (const entry of snapshot.messages) for (const part of entry.message.parts) {
        if (part.type === 'toolCall') {
            const key = `${messageKey(entry)}:${part.call.callId}`;
            calls.add(key); lastCall.set(`${entry.runId}:${part.call.callId}`, key);
        }
        if (part.type === 'toolResult') {
            // Old history has no origin. Its ordered call/result records still
            // associate with the preceding call, never a later reuse of its ID.
            const key = entry.origin ? `${messageKey(entry)}:${part.result.callId}` : lastCall.get(`${entry.runId}:${part.result.callId}`);
            if (key && calls.has(key)) { results.set(key, part.result); paired.add(part.result); }
        }
    }
    useLayoutEffect(() => {
        const root = scroller.current;
        if (!root || !visible) return;
        const content = root.firstElementChild;
        if (!content) return;
        const observer = new ResizeObserver(() => { if (following.current) root.scrollTop = root.scrollHeight; });
        observer.observe(content);
        observer.observe(root);
        if (following.current) root.scrollTop = root.scrollHeight;
        return () => observer.disconnect();
    }, [visible]);
    useLayoutEffect(() => {
        const root = scroller.current;
        if (!root || !visible) return;
        const userSeq = snapshot.messages.filter(entry => entry.message.role === 'user').at(-1)?.seq;
        if (userSeq !== undefined && userSeq > lastMessageSeq.current) following.current = true;
        lastMessageSeq.current = snapshot.messages.at(-1)?.seq ?? 0;
        if (anchor.current && (snapshot.messages[0]?.seq ?? Infinity) < anchor.current.firstSeq) {
            const item = root.querySelector<HTMLElement>(`[data-message-seq="${anchor.current.seq}"]`);
            if (item) root.scrollTop += item.getBoundingClientRect().top - root.getBoundingClientRect().top - anchor.current.offset;
            anchor.current = null;
        } else if (!anchor.current && following.current) root.scrollTop = root.scrollHeight;
    }, [snapshot.messages, snapshot.responses, visible]);
    async function loadOlder() {
        const root = scroller.current;
        // Clipped descendants keep their layout boxes; exclude them from paging anchors.
        const first = root && [...root.querySelectorAll<HTMLElement>('[data-message-seq]')]
            .find(item => !item.closest('[aria-hidden="true"]')
                && item.getBoundingClientRect().bottom > root.getBoundingClientRect().top && item.getBoundingClientRect().height > 0);
        const seq = first?.dataset.messageSeq;
        if (root && first && seq) anchor.current = { seq, offset: first.getBoundingClientRect().top - root.getBoundingClientRect().top, firstSeq: snapshot.messages[0]?.seq ?? 0 };
        following.current = false; setLoadingOlder(true); setPageError(null);
        try { await controller.loadOlder(); } catch (error) { anchor.current = null; setPageError(error); }
        finally { setLoadingOlder(false); }
    }
    return <div className="ttia-history-wrap">
        <div className="ttia-history" ref={scroller} role="region" aria-label={tr('conversation')}
            onClickCapture={event => {
                if (event.target instanceof Element && event.target.closest('[data-assistant-disclosure]')) {
                    following.current = false; setAway(true);
                }
            }} onScroll={() => {
                const root = scroller.current;
                if (!root) return;
                following.current = root.scrollHeight - root.clientHeight - root.scrollTop < 40;
                setAway(!following.current);
            }}>
            <div className="ttia-messages">
                {snapshot.nextBeforeSeq !== null && <button type="button" className="ttia-older" disabled={loadingOlder || snapshot.busy}
                    onClick={() => { void loadOlder(); }}>{loadingOlder ? tr('loading') : tr('older')}</button>}
                {pageError != null && <ErrorNotice error={pageError} retry={() => { void loadOlder(); }} />}
                {buildRunBlocks(snapshot, lastMessageByRun).map((block, blockIndex, blocks) => {
                    const renderEntry = (entry: TauriTavernAgentSessionMessage, collapseContent = false) => {
                        const { message } = entry;
                        if (message.role === 'user') return <div className="ttia-user" key={entry.seq} data-message-seq={entry.seq} data-new={entry.seq > initialSeq}>
                            {message.parts.filter(part => part.type === 'text').map(part => part.text).join('\n')}</div>;
                        if (message.role === 'tool') {
                            const orphaned = message.parts.filter(part => part.type === 'toolResult' && !paired.has(part.result));
                            return orphaned.length ? <div key={entry.seq} data-message-seq={entry.seq}>{orphaned.map((part, index) => part.type === 'toolResult'
                                ? <Disclosure key={index} label={tr('missingCall')}><ToolResult result={part.result} runId={entry.runId} actions={actions} /></Disclosure> : null)}</div> : null;
                        }
                        const key = messageKey(entry);
                        const parts = collapseContent ? message.parts.filter(part => part.type !== 'reasoning') : message.parts;
                        return <div key={key} data-message-seq={entry.seq} data-new={entry.seq > initialSeq}><AssistantMessage parts={parts} results={results}
                            finalReply={entry === block.finalEntry}
                            runId={entry.runId} scope={key} origin={entry.origin} events={snapshot.events} active={Boolean(snapshot.run?.active && snapshot.run.runId === entry.runId)} actions={actions} /></div>;
                    };
                    const renderResponse = (response: TauriTavernAgentRunLiveResponse, streaming: boolean, collapseContent = false) => <div key={responseKey(block.runId, response.invocationId, response.round)} data-new>
                        <AssistantMessage parts={[...(!collapseContent && response.reasoning ? [{ type: 'reasoning' as const, text: response.reasoning, provider_metadata: null }] : []),
                            ...(response.text ? [{ type: 'text' as const, text: response.text }] : [])]}
                        results={results} runId={block.runId} scope={responseKey(block.runId, response.invocationId, response.round)} origin={response} events={snapshot.events} active streaming={streaming} actions={actions} />
                    </div>;
                    const workResponses = block.responses.slice(0, -1);
                    const finalResponse = block.responses.at(-1);
                    const workContent = [...block.workEntries.map(entry => renderEntry(entry)), ...workResponses.map(response => renderResponse(response, false))];
                    const expanded = expandOverride.get(block.runId) ?? (block.active || blockIndex === blocks.length - 1);
                    const hasReasoning = block.finalEntry?.message.parts.some(part => part.type === 'reasoning' && part.text) || Boolean(finalResponse?.reasoning);
                    const showBar = workContent.length > 0 || hasReasoning || (block.active && block.userEntries.length > 0);
                    return <div className="ttia-run" key={block.runId}>
                        {block.userEntries.map(entry => renderEntry(entry))}
                        {showBar && <RunBar block={block} expanded={expanded} onToggle={() => setExpandOverride(prev => new Map(prev).set(block.runId, !expanded))} />}
                        {workContent.length > 0 && <div className={`ttia-run-work${expanded ? ' is-open' : ''}`} inert={!expanded} aria-hidden={!expanded}><div>{workContent}</div></div>}
                        {block.finalEntry && renderEntry(block.finalEntry, !expanded)}
                        {finalResponse && renderResponse(finalResponse, true, !expanded)}
                    </div>;
                })}
            </div>
        </div>
        {away && <button type="button" className="ttia-latest" onClick={() => {
            following.current = true; setAway(false); scroller.current?.scrollTo({ top: scroller.current.scrollHeight, behavior: 'smooth' });
        }}><Icon name="arrow-down" />{tr('latest')}</button>}
    </div>;
}
