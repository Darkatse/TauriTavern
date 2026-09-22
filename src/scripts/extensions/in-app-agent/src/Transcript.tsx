import { useLayoutEffect, useRef, useState } from 'react';
import type { AssistantSnapshot } from './controller';
import type { AssistantActions, AssistantController } from './host';
import { CopyButton, Disclosure, ErrorNotice, Icon, Markdown } from './components';
import { tr, type MessageKey } from './i18n';

type Part = TauriTavernAgentModelContentPart;
type Call = Extract<Part, { type: 'toolCall' }>['call'];
type Result = Extract<Part, { type: 'toolResult' }>['result'];
const toolLabels: Record<string, [MessageKey, MessageKey]> = {
    'extension/in-app-agent:app.evaluate': ['evaluate', 'evaluateHelp'],
    'extension/in-app-agent:app.read_logs': ['logs', 'logsHelp'],
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
export function responseKey(runId: string, invocationId: string, round: number) { return `${runId}:${invocationId}:${round}`; }
function messageKey(entry: TauriTavernAgentSessionMessage) {
    return entry.origin ? responseKey(entry.runId, entry.origin.invocationId, entry.origin.round) : `history:${entry.seq}`;
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
    const event = origin && events.find(item => item.runId === runId && ['tool_call_completed', 'tool_call_failed'].includes(item.type)
        && record(item.payload).callId === call.callId && record(item.payload).round === origin.round
        && record(item.payload).invocationId === origin.invocationId);
    const elapsed = record(event?.payload).elapsedMs;
    const failed = result?.isError || event?.type === 'tool_call_failed';
    const done = Boolean(result || event);
    const args = typeof call.arguments === 'string' ? call.arguments : JSON.stringify(call.arguments, null, 2);
    const code = record(call.arguments).code;
    return <Disclosure className={`ttia-tool ${failed ? 'has-error' : ''}`} label={<>
        <Icon name={failed ? 'circle-exclamation' : done ? 'check' : active ? 'circle-notch' : 'minus'} />
        <span className="ttia-tool-name">{toolName(call.toolId)}</span>
        <span className="ttia-tool-state">{typeof elapsed === 'number' ? `${(elapsed / 1000).toFixed(1)}s`
            : tr(done ? (failed ? 'failed' : 'done') : active ? 'working' : 'pending')}</span>
    </>}>
        <div className="ttia-tool-detail">
            <div className="ttia-detail-label">{tr(typeof code === 'string' ? 'code' : 'arguments')}
                <CopyButton text={typeof code === 'string' ? code : args} copy={actions.copy} /></div>
            <pre>{typeof code === 'string' ? code : args}</pre>
            {result && <ToolResult result={result} runId={runId} actions={actions} />}
        </div>
    </Disclosure>;
}
function AssistantMessage({ parts, results, runId, origin, scope, events, active, actions }: {
    parts: Part[]; results: Map<string, Result>; runId: string; origin: TauriTavernAgentSessionMessage['origin']; scope: string;
    events: TauriTavernAgentRunEvent[]; active: boolean; actions: AssistantActions;
}) {
    const text = parts.filter(part => part.type === 'text').map(part => part.text).join('\n');
    // Stored part order is provider data; reasoning has a fixed place in the UI.
    const displayParts = [...parts.filter(part => part.type === 'reasoning'), ...parts.filter(part => part.type !== 'reasoning')];
    return <article className="ttia-reply">
        {displayParts.map((part, index) => {
            if (part.type === 'text') return <Markdown key={index} text={part.text} actions={actions} />;
            if (part.type === 'reasoning') return part.text ? <Disclosure key={index} className="ttia-reasoning" label={tr('reasoning')}>
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
        {text && <CopyButton text={text} copy={actions.copy} />}
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
    const runId = snapshot.run?.runId;
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
        const first = root && [...root.querySelectorAll<HTMLElement>('[data-message-seq]')]
            .find(item => item.getBoundingClientRect().bottom > root.getBoundingClientRect().top);
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
                {[...snapshot.messages.map(entry => {
                    const { message } = entry;
                    if (message.role === 'user') return <div className="ttia-user" key={entry.seq} data-message-seq={entry.seq} data-new={entry.seq > initialSeq}>
                        {message.parts.filter(part => part.type === 'text').map(part => part.text).join('\n')}</div>;
                    if (message.role === 'tool') {
                        const orphaned = message.parts.filter(part => part.type === 'toolResult' && !paired.has(part.result));
                        return orphaned.length ? <div key={entry.seq} data-message-seq={entry.seq}>{orphaned.map((part, index) => part.type === 'toolResult'
                            ? <Disclosure key={index} label={tr('missingCall')}><ToolResult result={part.result} runId={entry.runId} actions={actions} /></Disclosure> : null)}</div> : null;
                    }
                    const key = messageKey(entry);
                    return <div key={key} data-message-seq={entry.seq} data-new={entry.seq > initialSeq}><AssistantMessage parts={message.parts} results={results}
                        runId={entry.runId} scope={key} origin={entry.origin} events={snapshot.events} active={Boolean(snapshot.run?.active && snapshot.run.runId === entry.runId)} actions={actions} /></div>;
                }), ...(runId ? snapshot.responses.map(response => <div key={responseKey(runId, response.invocationId, response.round)} data-new>
                    <AssistantMessage parts={[...(response.reasoning ? [{ type: 'reasoning' as const, text: response.reasoning, provider_metadata: null }] : []),
                        ...(response.text ? [{ type: 'text' as const, text: response.text }] : [])]}
                    results={results} runId={runId} scope={responseKey(runId, response.invocationId, response.round)} origin={response} events={snapshot.events} active actions={actions} />
                </div>) : [])]}
            </div>
        </div>
        {away && <button type="button" className="ttia-latest" onClick={() => {
            following.current = true; setAway(false); scroller.current?.scrollTo({ top: scroller.current.scrollHeight, behavior: 'smooth' });
        }}><Icon name="arrow-down" />{tr('latest')}</button>}
    </div>;
}
