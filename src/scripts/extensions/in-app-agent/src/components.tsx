import { useEffect, useRef, useState, memo, useMemo, type ReactNode } from 'react';
import { errorText, tr } from './i18n';
import type { AssistantActions } from './host';

export function Icon({ name }: { name: string }) {
    return <i className={`fa-solid fa-${name}`} aria-hidden="true" />;
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
export const Markdown = memo(function Markdown({ text, actions }: { text: string; actions: AssistantActions }) {
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
    return <><div className="ttia-markdown" ref={root} dangerouslySetInnerHTML={{ __html: html }} />
        {error != null && <ErrorNotice error={error} />}</>;
});
