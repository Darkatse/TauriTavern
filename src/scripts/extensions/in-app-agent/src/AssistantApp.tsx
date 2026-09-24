import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type CSSProperties } from 'react';
import { modelBindingFromTarget } from '../../../tauritavern/agent/model-target-llm-connection.js';
import type { AssistantActions, AssistantController, ModelTarget, SettingsOptions } from './host';
import type { AssistantDrawer } from './drawer';
import type { AssistantSnapshot } from './controller';
import { tr, type MessageKey } from './i18n';
import { ErrorNotice, Icon, NewConversationIcon, formatTick, useNow } from './components';
import { EffortMenu, withReasoningEffort } from './EffortMenu';
import { ModelMenu, modelChoice } from './ModelMenu';
import { Settings } from './Settings';
import { Transcript, toolName } from './Transcript';
import { History, sessionTitle } from './History';

function statusLabel(snapshot: AssistantSnapshot): string {
    const status = snapshot.run?.status;
    if (status === 'calling_model') return tr(snapshot.responses.some(response => response.text) ? 'replying' : 'waiting');
    if (status === 'dispatching_tool') {
        const event = [...snapshot.events].reverse().find(item => item.type === 'tool_call_requested');
        const payload = event?.payload as { toolId?: string } | undefined;
        return payload?.toolId ? toolName(payload.toolId) : tr('working');
    }
    return tr(status === 'cancelling' ? 'stopping' : status === 'finishing' ? 'finishing' : 'preparing');
}
function StatusStrip({ snapshot, preparing, cancelling }: { snapshot: AssistantSnapshot; preparing: boolean; cancelling: boolean }) {
    const active = Boolean(snapshot.run?.active) || preparing;
    const startedAt = (() => {
        const event = snapshot.events.find(item => item.type === 'run_started');
        const time = event ? Date.parse(event.timestamp) : NaN;
        return Number.isFinite(time) ? time : null;
    })();
    const now = useNow(1000, active && startedAt !== null);
    if (!active) return null;
    const label = snapshot.run?.active ? (cancelling ? tr('stopping') : statusLabel(snapshot)) : tr('preparing');
    return <div className="ttia-status-strip">
        <span className="ttia-pulse" aria-hidden="true" />
        <span className="ttia-status-text" role="status"><span key={label}>{label}</span></span>
        {startedAt !== null && <span className="ttia-status-elapsed">{formatTick(now - startedAt)}</span>}
    </div>;
}
function RunEnd({ snapshot }: { snapshot: AssistantSnapshot }) {
    const run = snapshot.run;
    if (!run || run.active) return null;
    const key = ({ failed: 'failed', partial_success: 'partial', cancelled: 'cancelled', interrupted: 'interrupted' } as Partial<Record<string, MessageKey>>)[run.status];
    if (!key) return null;
    const failure = [...snapshot.events].reverse().find(event => event.type === 'run_failed' || event.type === 'run_partial_success');
    return <div className={`ttia-run-end ${run.status === 'failed' ? 'has-error' : ''}`} role="status">
        <Icon name={run.status === 'failed' ? 'circle-exclamation' : 'circle-stop'} /><div>{tr(key)}
            {failure?.payload != null && <details><summary>{tr('details')}</summary><pre>{JSON.stringify(failure.payload, null, 2)}</pre></details>}
        </div>
    </div>;
}

export function AssistantApp({ controller, actions, drawer }: { controller: AssistantController; actions: AssistantActions; drawer: AssistantDrawer }) {
    const snapshot = useSyncExternalStore(listener => controller.subscribe(listener), controller.getSnapshot);
    const surface = useSyncExternalStore(listener => drawer.subscribe(listener), drawer.getSnapshot);
    const models = useSyncExternalStore(actions.models.subscribe, actions.models.getSnapshot);
    const visible = surface.open && surface.assistant;
    const [view, setView] = useState<'chat' | 'settings' | 'history'>('chat');
    const text = snapshot.draft;
    const [options, setOptions] = useState<SettingsOptions | null>(null);
    const [optionsError, setOptionsError] = useState<unknown>(null);
    const [draft, setDraft] = useState(() => structuredClone(snapshot.profile));
    const [contentWidth, setContentWidth] = useState(actions.contentWidthPercent);
    const [savedContentWidth, setSavedContentWidth] = useState(actions.contentWidthPercent);
    const [settingsVersion, setSettingsVersion] = useState(0);
    const [operation, setOperation] = useState<'save' | null>(null);
    const [error, setError] = useState<unknown>(null);
    const [notice, setNotice] = useState('');
    const [cancellingRunId, setCancellingRunId] = useState<string | null>(null);
    const [menu, setMenu] = useState<'model' | 'effort' | null>(null);
    const [sweep, setSweep] = useState(0);
    const input = useRef<HTMLTextAreaElement>(null);
    const resizeAnimation = useRef<Animation | null>(null);
    const initialized = useRef<Promise<void> | null>(null);
    const activity = useRef({ wasActive: false, unread: false });

    async function refreshOptions() {
        try { const result = await actions.loadOptions(controller.getSnapshot().profile.id); setOptions(result); setOptionsError(null); }
        catch (failure) { setOptionsError(failure); }
    }
    async function initialize() {
        await controller.initialize();
        setDraft(structuredClone(controller.getSnapshot().profile));
        await refreshOptions();
    }
    useEffect(() => {
        if (!visible) return;
        const first = initialized.current === null;
        initialized.current ??= controller.initialize();
        void initialized.current.then(async () => {
            if (first) setDraft(structuredClone(controller.getSnapshot().profile));
            const next = await actions.loadOptions(controller.getSnapshot().profile.id);
            setOptions(next); setOptionsError(null);
        }).catch(failure => {
            if (!controller.getSnapshot().initialized) initialized.current = null;
            setOptionsError(failure);
        });
    }, [visible, controller, actions]);
    useEffect(() => {
        const current = activity.current;
        if (current.wasActive && !snapshot.activeRun && !visible) current.unread = true;
        if (visible) current.unread = false;
        current.wasActive = Boolean(snapshot.activeRun);
        drawer.setActivity(current.wasActive ? 'working' : current.unread ? 'unread' : '');
    }, [drawer, snapshot.activeRun, visible]);
    useEffect(() => {
        if (visible && view === 'chat' && snapshot.initialized && !actions.isMobile()) input.current?.focus({ preventScroll: true });
    }, [visible, view, snapshot.initialized, snapshot.sessionId, actions]);
    useLayoutEffect(() => {
        const element = input.current;
        if (!element || !visible || view !== 'chat') return;
        const previousHeight = element.getBoundingClientRect().height;
        resizeAnimation.current?.cancel();
        // Measure without animation: transitioning to auto can make scrollHeight retain the old height.
        element.style.height = 'auto';
        element.style.height = `${element.scrollHeight}px`;
        const height = element.getBoundingClientRect().height;
        if (height !== previousHeight) resizeAnimation.current = element.animate([
            { height: `${previousHeight}px` }, { height: `${height}px` },
        ], { duration: 160, easing: getComputedStyle(element).getPropertyValue('--ttia-ease') });
    }, [text, visible, view]);
    useEffect(() => {
        if (!notice) return;
        const timer = setTimeout(() => setNotice(''), 3200);
        return () => clearTimeout(timer);
    }, [notice]);

    async function save(profile: TauriTavernAgentProfileDefinition) {
        setOperation('save'); setError(null);
        try {
            if (!snapshot.profileSaved || JSON.stringify(profile) !== JSON.stringify(snapshot.profile)) await controller.saveProfile(profile);
            if (contentWidth !== savedContentWidth) {
                await actions.saveContentWidth(contentWidth); setSavedContentWidth(contentWidth);
            }
            setDraft(structuredClone(profile)); setView(current => current === 'settings' ? 'chat' : current); setNotice(tr('saved'));
        } catch (failure) { setError(failure); throw failure; }
        finally { setOperation(null); }
    }
    // Keep an unsaved Settings draft from overwriting the composer's saved choices.
    async function saveChoice(change: (profile: TauriTavernAgentProfileDefinition) => TauriTavernAgentProfileDefinition) {
        if (!actions.isMobile()) input.current?.focus({ preventScroll: true });
        const current = controller.getSnapshot().profile;
        const next = change(current);
        if (JSON.stringify(next) === JSON.stringify(current)) return false;
        setOperation('save'); setError(null);
        try {
            await controller.saveProfile(next);
            setDraft(change);
            return true;
        } catch (failure) { setError(failure); return false; }
        finally { setOperation(null); }
    }
    async function chooseModel(target: ModelTarget) {
        const model = modelBindingFromTarget(target);
        if (await saveChoice(profile => ({ ...profile, model }))) setSweep(count => count + 1);
    }
    function submit() {
        if (!canSubmit) return;
        if (!canSend) { setMenu('model'); return; }
        void send();
    }
    async function send() {
        if (!canSend) return;
        setError(null); setNotice('');
        try { await controller.send(); }
        catch { /* The controller keeps the failure and draft with the originating Session. */ }
    }
    async function stop() {
        const runId = snapshot.activeRun?.runId;
        if (!runId) return;
        setCancellingRunId(runId); setError(null);
        try { await controller.cancel(); }
        catch (failure) { setError(failure); setCancellingRunId(null); }
    }
    function openSession(id: string | null) {
        setView('chat'); setError(null);
        void controller.selectSession(id).catch(() => { /* The selected view exposes read and storage errors. */ });
    }
    useEffect(() => {
        const keyDown = (event: KeyboardEvent) => {
            // Consume after child controls, before SillyTavern's document shortcuts.
            event.stopPropagation();
            // Menus and inline editors that own Escape mark it handled.
            if (event.key !== 'Escape' || event.defaultPrevented || event.isComposing || event.keyCode === 229) return;
            event.preventDefault();
            if (view !== 'chat') setView('chat'); else drawer.close();
        };
        drawer.mount.addEventListener('keydown', keyDown);
        return () => drawer.mount.removeEventListener('keydown', keyDown);
    }, [drawer, view]);
    const choice = modelChoice(models, snapshot.profile.model);
    const effortChoosable = snapshot.profileSaved && choice.state === 'ready' && actions.supportsReasoningEffort(choice.target);
    const presetName = snapshot.profile.preset.ref?.name ?? '';
    const choicesDisabled = !snapshot.initialized || snapshot.busy || operation !== null;
    // Leaving the chat view closes the menu instead of restoring it later.
    if (menu && (view !== 'chat' || !visible)) setMenu(null);
    const preparing = snapshot.sendingSessionId !== undefined;
    const preparingHere = preparing && snapshot.sendingSessionId === snapshot.sessionId;
    const cancelling = snapshot.activeRun?.runId === cancellingRunId;
    const taskSessionId = snapshot.activeRun?.sessionId ?? snapshot.sendingSessionId;
    const showTaskBanner = taskSessionId !== undefined && (view !== 'chat' || taskSessionId !== snapshot.sessionId);
    const canSubmit = snapshot.initialized && Boolean(text.trim()) && !snapshot.activeRun && !preparing && !snapshot.busy && !snapshot.loading && !operation;
    // Without a usable model, submitting opens the model menu instead of sending.
    const canSend = canSubmit && snapshot.profileSaved && choice.state === 'ready';
    const session = snapshot.sessions.find(item => item.id === snapshot.sessionId);
    const heading = view === 'chat' ? (session ? sessionTitle(session) : tr('title')) : tr(view === 'settings' ? 'settings' : 'history');
    const issue = error ?? snapshot.error;
    return <div className="ttia-root" data-visible={visible} data-mobile={actions.isMobile()} style={{ '--ttia-content-width': `${contentWidth}%` } as CSSProperties}>
        <button hidden={surface.assistant} type="button" className="ttia-return" onClick={() => drawer.setMode('assistant')}><Icon name="arrow-left" />{tr('back')}</button>
        <div className="ttia-assistant" hidden={!surface.assistant}>
            <header className="ttia-header"><div className="ttia-heading">
                {view !== 'chat' ? <button type="button" className="ttia-icon-button" aria-label={tr('conversation')} onClick={() => setView('chat')}><Icon name="arrow-left" /></button>
                    : <span className="ttia-brand" aria-hidden="true">A<span /></span>}
                <strong title={heading}>{heading}</strong>
            </div><div className="ttia-header-actions">
                <button type="button" className="ttia-advanced" onClick={() => drawer.setMode('advanced')}>{tr('advanced')}</button>
                {view === 'chat' && <>
                    <button type="button" className="ttia-icon-button" aria-label={tr('newConversation')} title={tr('newConversation')} disabled={!snapshot.initialized}
                        onClick={() => openSession(null)}><NewConversationIcon /></button>
                    <button type="button" className="ttia-icon-button" aria-label={tr('history')} title={tr('history')} disabled={!snapshot.initialized}
                        onClick={() => { setError(null); setView('history'); void controller.refreshSessions().catch(() => { /* The history view displays the error. */ }); }}><Icon name="clock-rotate-left" /></button>
                </>}
                {view === 'chat' && snapshot.profileSaved && <button type="button" className="ttia-icon-button" aria-label={tr('settings')} title={tr('settings')}
                    onClick={() => { setView('settings'); void refreshOptions(); }}><Icon name="sliders" /></button>}
                <button type="button" className="ttia-icon-button" aria-label={tr('close')} onClick={() => drawer.close()}><Icon name="xmark" /></button>
            </div></header>
            {showTaskBanner && <div className="ttia-task-banner">
                <div role="status"><span className="ttia-pulse" /><span>{tr(cancelling ? 'stopping' : snapshot.activeRun
                    ? taskSessionId === snapshot.sessionId ? 'working' : 'backgroundTask'
                    : taskSessionId === snapshot.sessionId ? 'preparing' : 'backgroundPreparation')}</span></div>
                <div className="ttia-actions"><button type="button" onClick={() => openSession(taskSessionId)}>{tr('returnToTask')}<Icon name="arrow-right" /></button>
                    {snapshot.activeRun && <button type="button" className="ttia-icon-button" aria-label={tr('stop')} title={tr('stop')} disabled={cancelling} onClick={() => { void stop(); }}><Icon name="stop" /></button>}
                </div>
            </div>}
            <div className="ttia-chat-view" hidden={view !== 'chat'}>
                {!snapshot.initialized ? <div className="ttia-loading"><span className="ttia-pulse" />{tr('loading')}
                    {issue != null && <ErrorNotice error={issue} retry={() => { setError(null); void initialize().catch(setError); }} />}</div>
                    : snapshot.loading ? <div className="ttia-loading"><span className="ttia-pulse" />{tr('loadingConversation')}</div>
                        : <div className="ttia-conversation-area">
                            {snapshot.messages.length === 0 && !snapshot.run && <div className="ttia-welcome"><div className="ttia-emblem" aria-hidden="true">A<span /></div>
                            <h2>{tr('welcome')}</h2><p>{tr('welcomeNote')}</p>
                            {!text && <div className="ttia-suggestions">{([
                                { key: 'suggestLogs', icon: 'magnifying-glass' }, { key: 'suggestExtension', icon: 'puzzle-piece' }, { key: 'suggestSettings', icon: 'sliders' },
                            ] as const).map(({ key, icon }, index) =>
                                <button type="button" key={key} style={{ '--ttia-stagger': index } as CSSProperties} onClick={() => { controller.setDraft(tr(key)); input.current?.focus(); }}>
                                    <span className="ttia-tile" aria-hidden="true"><Icon name={icon} /></span><span>{tr(key)}</span><Icon name="arrow-up" /></button>)}</div>}
                            </div>}
                            <div className="ttia-history-area" hidden={snapshot.messages.length === 0 && !snapshot.run}>
                                <Transcript key={snapshot.sessionId ?? 'new'} snapshot={snapshot} controller={controller} actions={actions} visible={visible && view === 'chat' && (snapshot.messages.length > 0 || snapshot.run !== null)} />
                            </div>
                        </div>}
                <div className="ttia-bottom">
                    <RunEnd snapshot={snapshot} />
                    {snapshot.initialized && issue != null && <ErrorNotice error={issue} retry={() => {
                        setError(null); void controller.refresh().catch(setError);
                    }} />}
                    {notice && <p className="ttia-notice" role="status">{notice}</p>}
                    <form className={`ttia-composer ${snapshot.run?.active || preparingHere ? 'has-status' : ''}`} onSubmit={event => { event.preventDefault(); submit(); }}>
                        {sweep > 0 && <span key={sweep} className="ttia-sweep" aria-hidden="true" />}
                        <StatusStrip snapshot={snapshot} preparing={preparingHere} cancelling={cancelling && snapshot.run?.active === true} />
                        <textarea ref={input} rows={1} aria-label={tr('composer')} placeholder={tr('placeholder')} value={text} readOnly={preparingHere}
                            onChange={event => controller.setDraft(event.target.value)} onKeyDown={event => {
                                if (event.key === 'Enter' && !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey
                                    && !event.nativeEvent.isComposing && event.nativeEvent.keyCode !== 229 && actions.shouldSendOnEnter()) {
                                    event.preventDefault(); submit();
                                }
                            }} />
                        <div className="ttia-composer-footer">
                            <div className="ttia-composer-tools">
                                <ModelMenu models={models} choice={choice} open={menu === 'model'} disabled={choicesDisabled}
                                    onOpenChange={open => setMenu(open ? 'model' : null)} onSelect={target => { void chooseModel(target); }} onManage={() => actions.openConnections()} />
                                {effortChoosable && <EffortMenu effort={snapshot.profile.preset.reasoningEffort} preset={presetName}
                                    presetEffort={actions.presetReasoningEffort(presetName)} open={menu === 'effort'} disabled={choicesDisabled} onOpenChange={open => setMenu(open ? 'effort' : null)}
                                    onSelect={effort => { void saveChoice(profile => withReasoningEffort(profile, effort)); }} />}
                            </div>
                            {snapshot.run?.active ? <button type="button" className="ttia-send is-stop" aria-label={tr('stop')} title={tr('stop')} disabled={cancelling} onClick={() => { void stop(); }}><Icon name="stop" /></button>
                                : <button type="submit" className="ttia-send" aria-label={tr('send')} title={tr('send')} disabled={!canSubmit}><Icon name={preparingHere ? 'ellipsis' : 'arrow-up'} /></button>}
                        </div>
                    </form>
                    <div className="ttia-input-hint">{tr(preparingHere ? 'preparing' : snapshot.activeRun || preparing ? 'waitingForTask'
                        : snapshot.initialized && choice.state !== 'ready' ? choice.state === 'missing' ? 'bindingMissing' : 'chooseModelHint'
                        : actions.shouldSendOnEnter() ? 'sendHint' : 'newlineHint')}</div>
                </div>
            </div>
            {view === 'history' && <History snapshot={snapshot} controller={controller} actions={actions} onOpen={openSession} onNew={() => openSession(null)} />}
            <div className="ttia-settings-view" hidden={view !== 'settings'}>
                {optionsError != null && <ErrorNotice error={optionsError} retry={() => { void refreshOptions(); }} />}
                {options && <Settings key={settingsVersion} draft={draft} setDraft={setDraft} options={options} actions={actions} busy={snapshot.busy || operation === 'save'}
                    contentWidth={contentWidth} onContentWidthChange={setContentWidth}
                    error={error} dirty={JSON.stringify(draft) !== JSON.stringify(snapshot.profile) || contentWidth !== savedContentWidth}
                    onSave={() => { void save(draft).catch(() => { /* The settings error stays next to the form. */ }); }}
                    onCancel={() => { setDraft(structuredClone(snapshot.profile)); setContentWidth(savedContentWidth); setSettingsVersion(value => value + 1); setError(null); setView('chat'); }} refreshOptions={() => { void refreshOptions(); }} />}
            </div>
        </div>
    </div>;
}
