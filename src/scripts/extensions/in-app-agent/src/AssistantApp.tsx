import { useEffect, useRef, useState, useSyncExternalStore, type CSSProperties } from 'react';
import { findModelTargetForBinding } from '../../../tauritavern/agent/model-target-llm-connection.js';
import type { AssistantActions, AssistantController, SettingsOptions } from './host';
import type { AssistantDrawer } from './drawer';
import type { AssistantSnapshot } from './controller';
import { tr, type MessageKey } from './i18n';
import { ErrorNotice, Icon } from './components';
import { Settings, Setup } from './Settings';
import { Transcript, toolName } from './Transcript';

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
function RunStatus({ snapshot, cancelling }: { snapshot: AssistantSnapshot; cancelling: boolean }) {
    const run = snapshot.run;
    if (!run) return null;
    if (run.active) return <div className="ttia-status" role="status"><span className="ttia-pulse" />{cancelling ? tr('stopping') : statusLabel(snapshot)}</div>;
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
    const visible = surface.open && surface.assistant;
    const [view, setView] = useState<'chat' | 'settings'>('chat');
    const [text, setText] = useState('');
    const [options, setOptions] = useState<SettingsOptions | null>(null);
    const [optionsError, setOptionsError] = useState<unknown>(null);
    const [draft, setDraft] = useState(() => structuredClone(snapshot.profile));
    const [contentWidth, setContentWidth] = useState(actions.contentWidthPercent);
    const [savedContentWidth, setSavedContentWidth] = useState(actions.contentWidthPercent);
    const [settingsVersion, setSettingsVersion] = useState(0);
    const [operation, setOperation] = useState<'send' | 'save' | null>(null);
    const [error, setError] = useState<unknown>(null);
    const [notice, setNotice] = useState('');
    const [cancelling, setCancelling] = useState(false);
    const input = useRef<HTMLTextAreaElement>(null);
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
        if (current.wasActive && !snapshot.run?.active && !visible) current.unread = true;
        if (visible) current.unread = false;
        current.wasActive = Boolean(snapshot.run?.active);
        drawer.setActivity(current.wasActive ? 'working' : current.unread ? 'unread' : '');
    }, [drawer, snapshot.run, visible]);
    useEffect(() => {
        if (visible && view === 'chat' && snapshot.initialized && !actions.isMobile()) input.current?.focus({ preventScroll: true });
    }, [visible, view, snapshot.initialized, actions]);
    useEffect(() => {
        const element = input.current;
        if (!element || !visible) return;
        element.style.height = 'auto';
        element.style.height = `${element.scrollHeight}px`;
    }, [text, visible]);
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
            setDraft(structuredClone(profile)); setView('chat'); setNotice(tr('saved'));
        } catch (failure) { setError(failure); throw failure; }
        finally { setOperation(null); }
    }
    async function send() {
        if (!text.trim() || snapshot.busy || snapshot.run?.active || !snapshot.profileSaved || operation) return;
        setOperation('send'); setError(null); setNotice(''); setCancelling(false);
        try { await controller.send(text); setText(''); }
        catch (failure) { setError(failure); }
        finally { setOperation(null); }
    }
    async function stop() {
        setCancelling(true); setError(null);
        try { await controller.cancel(); }
        catch (failure) { setError(failure); setCancelling(false); }
    }
    useEffect(() => {
        const keyDown = (event: KeyboardEvent) => {
            // Consume after child controls, before SillyTavern's document shortcuts.
            event.stopPropagation();
            if (event.key !== 'Escape' || event.isComposing || event.keyCode === 229) return;
            event.preventDefault();
            if (view === 'settings') setView('chat'); else drawer.close();
        };
        drawer.mount.addEventListener('keydown', keyDown);
        return () => drawer.mount.removeEventListener('keydown', keyDown);
    }, [drawer, view]);
    const model = options && findModelTargetForBinding(options.models, snapshot.profile.model);
    const canSend = snapshot.initialized && snapshot.profileSaved && Boolean(text.trim()) && !snapshot.run?.active && !snapshot.busy && !operation;
    const issue = error ?? snapshot.error;
    return <div className="ttia-root" data-visible={visible} data-mobile={actions.isMobile()} style={{ '--ttia-content-width': `${contentWidth}%` } as CSSProperties}>
        <button hidden={surface.assistant} type="button" className="ttia-return" onClick={() => drawer.setMode('assistant')}><Icon name="arrow-left" />{tr('back')}</button>
        <div className="ttia-assistant" hidden={!surface.assistant}>
            <header className="ttia-header"><div className="ttia-heading">
                {view === 'settings' ? <button type="button" className="ttia-icon-button" aria-label={tr('conversation')} onClick={() => setView('chat')}><Icon name="arrow-left" /></button>
                    : <span className="ttia-brand" aria-hidden="true">A<span /></span>}
                <strong>{tr(view === 'settings' ? 'settings' : 'title')}</strong>
            </div><div className="ttia-header-actions">
                <button type="button" className="ttia-advanced" onClick={() => drawer.setMode('advanced')}>{tr('advanced')}</button>
                {view === 'chat' && <button type="button" className="ttia-icon-button" aria-label={tr('settings')} title={tr('settings')} disabled={!snapshot.initialized}
                    onClick={() => { setView('settings'); void refreshOptions(); }}><Icon name="sliders" /></button>}
                <button type="button" className="ttia-icon-button" aria-label={tr('close')} onClick={() => drawer.close()}><Icon name="xmark" /></button>
            </div></header>
            <div className="ttia-chat-view" hidden={view !== 'chat'}>
                {!snapshot.initialized ? <div className="ttia-loading"><span className="ttia-pulse" />{tr('loading')}
                    {issue != null && <ErrorNotice error={issue} retry={() => { setError(null); void initialize().catch(setError); }} />}</div>
                    : !snapshot.profileSaved ? <div className="ttia-setup-scroll">{options ? <Setup options={options} profile={snapshot.profile} actions={actions} busy={snapshot.busy} onSave={save} />
                        : optionsError != null ? <ErrorNotice error={optionsError} retry={() => { void refreshOptions(); }} /> : <div className="ttia-loading">{tr('loading')}</div>}</div>
                        : <div className="ttia-conversation-area">
                            {snapshot.messages.length === 0 && !snapshot.run && <div className="ttia-welcome"><div className="ttia-emblem" aria-hidden="true">A<span /></div>
                            <h2>{tr('welcome')}</h2><p>{tr('welcomeNote')}</p>
                            {!text && <div className="ttia-suggestions">{([
                                { key: 'suggestLogs', icon: 'magnifying-glass' }, { key: 'suggestExtension', icon: 'puzzle-piece' }, { key: 'suggestSettings', icon: 'sliders' },
                            ] as const).map(({ key, icon }) =>
                                <button type="button" key={key} onClick={() => { setText(tr(key)); input.current?.focus(); }}><Icon name={icon} /><span>{tr(key)}</span><Icon name="arrow-up" /></button>)}</div>}
                            </div>}
                            <div className="ttia-history-area" hidden={snapshot.messages.length === 0 && !snapshot.run}>
                                <Transcript snapshot={snapshot} controller={controller} actions={actions} visible={visible && view === 'chat' && (snapshot.messages.length > 0 || snapshot.run !== null)} />
                            </div>
                        </div>}
                <div className="ttia-bottom">
                    <RunStatus snapshot={snapshot} cancelling={cancelling && snapshot.run?.active === true} />
                    {snapshot.initialized && issue != null && <ErrorNotice error={issue} retry={() => {
                        setError(null); void controller.refresh().catch(setError);
                    }} />}
                    {notice && <p className="ttia-notice" role="status">{notice}</p>}
                    <form className={`ttia-composer ${snapshot.run?.active ? 'is-running' : ''}`} onSubmit={event => { event.preventDefault(); void send(); }}>
                        <textarea ref={input} rows={2} aria-label={tr('composer')} placeholder={tr('placeholder')} value={text} readOnly={operation === 'send'}
                            onChange={event => setText(event.target.value)} onKeyDown={event => {
                                if (event.key === 'Enter' && !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey
                                    && !event.nativeEvent.isComposing && event.nativeEvent.keyCode !== 229 && actions.shouldSendOnEnter()) {
                                    event.preventDefault(); if (canSend) void send();
                                }
                            }} />
                        <div className="ttia-composer-footer"><button type="button" className="ttia-model" disabled={!snapshot.initialized} onClick={() => { setView('settings'); void refreshOptions(); }}>
                            <Icon name="circle-nodes" /><span>{model?.name || snapshot.profile.model.modelId || tr('model')}</span><Icon name="chevron-right" /></button>
                            {snapshot.run?.active ? <button type="button" className="ttia-send is-stop" aria-label={tr('stop')} title={tr('stop')} disabled={cancelling} onClick={() => { void stop(); }}><Icon name="stop" /></button>
                                : <button type="submit" className="ttia-send" aria-label={tr('send')} title={tr('send')} disabled={!canSend}><Icon name={operation === 'send' ? 'ellipsis' : 'arrow-up'} /></button>}
                        </div>
                    </form>
                    <div className="ttia-input-hint">{operation === 'send' ? tr('preparing') : snapshot.run?.active ? tr('draftHint') : tr(actions.shouldSendOnEnter() ? 'sendHint' : 'newlineHint')}</div>
                </div>
            </div>
            <div className="ttia-settings-view" hidden={view !== 'settings'}>
                {snapshot.run?.active && <div className="ttia-settings-running"><RunStatus snapshot={snapshot} cancelling={cancelling} />
                    <button type="button" disabled={cancelling} onClick={() => { void stop(); }}>{tr('stop')}</button></div>}
                {optionsError != null && <ErrorNotice error={optionsError} retry={() => { void refreshOptions(); }} />}
                {options && <Settings key={settingsVersion} draft={draft} setDraft={setDraft} options={options} actions={actions} busy={snapshot.busy || operation === 'save'}
                    contentWidth={contentWidth} onContentWidthChange={setContentWidth}
                    error={error} dirty={!snapshot.profileSaved || JSON.stringify(draft) !== JSON.stringify(snapshot.profile) || contentWidth !== savedContentWidth}
                    onSave={() => { void save(draft).catch(() => { /* The settings error stays next to the form. */ }); }}
                    onCancel={() => { setDraft(structuredClone(snapshot.profile)); setContentWidth(savedContentWidth); setSettingsVersion(value => value + 1); setError(null); setView('chat'); }} refreshOptions={() => { void refreshOptions(); }} />}
            </div>
        </div>
    </div>;
}
