import { useEffect, useRef, useState, memo, useMemo, type ReactNode } from 'react';
import { errorText, tr } from './i18n';
import type { AssistantActions } from './host';

export function Icon({ name }: { name: string }) {
    return <i className={`fa-solid fa-${name}`} aria-hidden="true" />;
}
export function NewConversationIcon() {
    return <svg width="1.1em" height="1.1em" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="0.6" strokeLinejoin="round" aria-hidden="true" focusable="false">
        <path d="M8 0.599609C3.91309 0.599609 0.599609 3.91309 0.599609 8C0.599609 9.13376 0.855461 10.2098 1.3125 11.1719L1.5918 11.7588L2.76562 11.2012L2.48633 10.6143C2.11034 9.82278 1.90039 8.93675 1.90039 8C1.90039 4.63106 4.63106 1.90039 8 1.90039C11.3689 1.90039 14.0996 4.63106 14.0996 8C14.0996 11.3689 11.3689 14.0996 8 14.0996C7.31041 14.0996 6.80528 14.0514 6.35742 13.9277C5.91623 13.8059 5.49768 13.6021 4.99707 13.2529C4.26492 12.7422 3.21611 12.5616 2.35156 13.1074L2.33789 13.1162L2.32422 13.126L1.58789 13.6436L2.01953 14.9297L3.0459 14.207C3.36351 14.0065 3.83838 14.0294 4.25293 14.3184C4.84547 14.7317 5.39743 15.011 6.01172 15.1807C6.61947 15.3485 7.25549 15.4004 8 15.4004C12.0869 15.4004 15.4004 12.0869 15.4004 8C15.4004 3.91309 12.0869 0.599609 8 0.599609ZM7.34473 4.93945V7.34961H4.93945V8.65039H7.34473V11.0605H8.64551V8.65039H11.0605V7.34961H8.64551V4.93945H7.34473Z" fill="currentColor" />
    </svg>;
}
export function ErrorNotice({ error, retry }: { error: unknown; retry?: () => void }) {
    return <div className="ttia-error" role="alert">
        <Icon name="circle-exclamation" /><div><p>{errorText(error)}</p>
            {retry && <button type="button" onClick={retry}>{tr('retry')}</button>}
        </div>
    </div>;
}
export function Disclosure({ label, children, className = '' }: { label: ReactNode; children: ReactNode; className?: string }) {
    const [open, setOpen] = useState(false);
    return <div className={`ttia-fold ${open ? 'is-open' : ''} ${className}`}>
        <button className="ttia-fold-toggle" type="button" aria-expanded={open} data-assistant-disclosure onClick={() => setOpen(!open)}>
            <Icon name="chevron-right" />{label}
        </button>
        <div className="ttia-fold-body" inert={!open} aria-hidden={!open}><div>{children}</div></div>
    </div>;
}
export function CopyButton({ text, copy }: { text: string; copy: AssistantActions['copy'] }) {
    const [copied, setCopied] = useState(false);
    const [error, setError] = useState<unknown>(null);
    useEffect(() => {
        if (!copied) return;
        const timer = setTimeout(() => setCopied(false), 1200);
        return () => clearTimeout(timer);
    }, [copied]);
    return <span className="ttia-copy">
        <button type="button" aria-label={tr(copied ? 'copied' : 'copy')} title={tr(copied ? 'copied' : 'copy')}
            onClick={() => { setError(null); void copy(text).then(() => setCopied(true)).catch(setError); }}>
            <Icon name={copied ? 'check' : 'copy'} /><span>{tr(copied ? 'copied' : 'copy')}</span>
        </button>{error != null && <span role="alert">{errorText(error)}</span>}
    </span>;
}
export function useNow(intervalMs: number, enabled: boolean): number {
    const [now, setNow] = useState(() => Date.now());
    useEffect(() => {
        if (!enabled) return;
        const timer = setInterval(() => setNow(Date.now()), intervalMs);
        return () => clearInterval(timer);
    }, [enabled, intervalMs]);
    return now;
}
export function formatTick(ms: number): string {
    const seconds = Math.max(0, Math.floor(ms / 1000));
    if (seconds < 60) return `${seconds}s`;
    return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}
export function formatDuration(ms: number): string {
    const seconds = Math.max(0, Math.round(ms / 1000));
    if (seconds < 60) return `${seconds}s`;
    return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}
export const Markdown = memo(function Markdown({ text, actions, streaming }: { text: string; actions: AssistantActions; streaming?: boolean }) {
    const html = useMemo(() => actions.markdown(text), [actions, text]);
    const root = useRef<HTMLDivElement>(null);
    const [error, setError] = useState<unknown>(null);
    useEffect(() => {
        const element = root.current;
        if (!element) return;
        const click = (event: MouseEvent) => {
            const anchor = event.target instanceof Element ? event.target.closest('a') : null;
            if (!anchor) return;
            event.preventDefault();
            void actions.openLink(anchor.href).catch(setError);
        };
        element.addEventListener('click', click);
        return () => element.removeEventListener('click', click);
    }, [actions]);
    return <><div className={`ttia-markdown${streaming ? ' is-streaming' : ''}`} ref={root} dangerouslySetInnerHTML={{ __html: html }} />
        {error != null && <ErrorNotice error={error} />}</>;
});
