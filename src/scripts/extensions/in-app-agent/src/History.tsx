import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import type { AssistantSnapshot } from './controller';
import type { AssistantActions, AssistantController } from './host';
import { ErrorNotice, Icon, NewConversationIcon } from './components';
import { tr } from './i18n';

export function sessionTitle(session: TauriTavernAgentSession): string {
    return session.title ?? tr('untitledConversation');
}

export function History({ snapshot, controller, actions, onOpen, onNew }: {
    snapshot: AssistantSnapshot; controller: AssistantController; actions: AssistantActions;
    onOpen: (id: string) => void; onNew: () => void;
}) {
    const [query, setQuery] = useState('');
    const [menu, setMenu] = useState<string | null>(null);
    const [editing, setEditing] = useState<{ id: string; title: string } | null>(null);
    const [working, setWorking] = useState<string | null>(null);
    const [error, setError] = useState<unknown>(null);
    const [now, setNow] = useState(Date.now);
    const input = useRef<HTMLInputElement>(null);
    useEffect(() => { input.current?.focus(); input.current?.select(); }, [editing?.id]);
    useEffect(() => {
        const timer = window.setInterval(() => setNow(Date.now()), 60_000);
        return () => window.clearInterval(timer);
    }, []);
    const search = query.trim().toLocaleLowerCase();
    const sessions = snapshot.sessions.filter(session => sessionTitle(session).toLocaleLowerCase().includes(search));
    const locale = document.documentElement.lang || undefined;
    const date = useMemo(() => new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }), [locale]);
    const relative = useMemo(() => new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }), [locale]);
    const issue = error ?? snapshot.error;
    function displayTime(at: Date): string {
        const minutes = Math.max(0, Math.floor((now - at.getTime()) / 60_000));
        if (minutes < 1) return relative.format(0, 'second');
        if (minutes < 60) return relative.format(-minutes, 'minute');
        if (minutes < 1440) return relative.format(-Math.floor(minutes / 60), 'hour');
        if (minutes < 10080) return relative.format(-Math.floor(minutes / 1440), 'day');
        return date.format(at);
    }
    function closeMenu(event: KeyboardEvent) {
        if (event.key === 'Escape') {
            event.stopPropagation();
            event.currentTarget.closest('.ttia-session-menu')?.querySelector<HTMLButtonElement>(':scope > button')?.focus();
            setMenu(null);
        }
    }

    async function rename() {
        if (!editing || !editing.title.trim()) return;
        setWorking(editing.id); setError(null);
        try { await controller.renameSession(editing.id, editing.title.trim()); setEditing(null); }
        catch (failure) { setError(failure); }
        finally { setWorking(null); }
    }
    async function remove(session: TauriTavernAgentSession) {
        setMenu(null); setWorking(session.id); setError(null);
        try {
            if (await actions.confirmDeleteSession(sessionTitle(session))) await controller.deleteSession(session.id);
        } catch (failure) { setError(failure); }
        finally { setWorking(null); }
    }
    return <section className="ttia-session-history" aria-label={tr('history')}>
        <div className="ttia-history-toolbar">
            <label className="ttia-history-search"><Icon name="magnifying-glass" />
                <input type="search" aria-label={tr('searchConversations')} placeholder={tr('searchConversations')} value={query} onChange={event => setQuery(event.target.value)} />
            </label>
            <button type="button" className="ttia-history-new" onClick={onNew}><NewConversationIcon />{tr('newConversation')}</button>
        </div>
        {issue != null && <ErrorNotice error={issue} retry={() => { setError(null); void controller.refreshSessions().catch(setError); }} />}
        {sessions.length === 0 ? <div className="ttia-history-empty"><Icon name="comments" />
            <p>{tr(search ? 'noMatchingConversations' : 'noConversations')}</p>
            {!search && <span>{tr('historyEmptyNote')}</span>}
        </div> : <ul className="ttia-session-list">
            {sessions.map(session => {
                const running = snapshot.activeRun?.sessionId === session.id || snapshot.sendingSessionId === session.id;
                const current = snapshot.sessionId === session.id;
                const timestamp = session.lastUsedAt ?? session.createdAt;
                const at = new Date(timestamp);
                return <li key={session.id} className="ttia-session-row" data-current={current}>
                    {editing?.id === session.id ? <form className="ttia-session-rename" onSubmit={event => { event.preventDefault(); void rename(); }}>
                        <input ref={input} aria-label={tr('conversationTitle')} value={editing.title} disabled={working !== null}
                            onChange={event => setEditing({ id: session.id, title: event.target.value })} onKeyDown={event => {
                                if (event.key === 'Escape' && !event.nativeEvent.isComposing) { event.stopPropagation(); setEditing(null); }
                            }} />
                        <button type="submit" className="ttia-icon-button" aria-label={tr('saveTitle')} disabled={!editing.title.trim() || working !== null || snapshot.busy}><Icon name="check" /></button>
                        <button type="button" className="ttia-icon-button" aria-label={tr('cancel')} disabled={working !== null} onClick={() => setEditing(null)}><Icon name="xmark" /></button>
                    </form> : <>
                        <button type="button" className="ttia-session-open" aria-current={current ? 'page' : undefined} onClick={() => onOpen(session.id)}>
                            <span className="ttia-session-mark"><Icon name="comment" /></span>
                            <strong className="ttia-session-title" title={sessionTitle(session)}>{sessionTitle(session)}</strong>
                            {current && <span className="ttia-session-tag">{tr('currentConversation')}</span>}
                            {running && <span className="ttia-session-tag is-running"><span className="ttia-pulse" />{tr('runningConversation')}</span>}
                            <time className="ttia-session-time" dateTime={timestamp} title={date.format(at)}>{displayTime(at)}</time>
                        </button>
                        <div className="ttia-session-menu" onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) setMenu(null); }}>
                            <button type="button" className="ttia-icon-button" aria-label={tr('conversationActions', { title: sessionTitle(session) })}
                                aria-expanded={menu === session.id} disabled={working !== null || snapshot.busy}
                                onKeyDown={closeMenu} onClick={() => setMenu(menu === session.id ? null : session.id)}><Icon name="ellipsis" /></button>
                            {menu === session.id && <div className="ttia-session-menu-items">
                                <button type="button" onKeyDown={closeMenu} onClick={() => { setMenu(null); setEditing({ id: session.id, title: sessionTitle(session) }); }}><Icon name="pen" />{tr('renameConversation')}</button>
                                <button type="button" className="ttia-danger" disabled={running} title={running ? tr('stopBeforeDelete') : undefined}
                                    onKeyDown={closeMenu} onClick={() => { void remove(session); }}><Icon name="trash-can" />{tr('deleteConversation')}</button>
                            </div>}
                        </div>
                    </>}
                </li>;
            })}
        </ul>}
    </section>;
}
