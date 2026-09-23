import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { Icon } from './components';

const ITEM = 'button[role^="menuitem"]';

export function ComposerMenu({ className, state, label, title, name, open, disabled, onOpenChange, trigger, children }: {
    className: string; state?: string; label: string; title?: string | undefined; name: string;
    open: boolean; disabled: boolean; onOpenChange: (open: boolean) => void;
    trigger: ReactNode; children: ReactNode;
}) {
    const menuId = useId();
    const root = useRef<HTMLDivElement>(null);
    const button = useRef<HTMLButtonElement>(null);
    const pop = useRef<HTMLDivElement>(null);
    const glide = useRef<HTMLSpanElement>(null);
    // Keyboard and programmatic opens light the focused row at once; pointer opens wait for hover.
    const keyboard = useRef(true);
    const [lit, setLit] = useState(false);
    const rows = () => Array.from(pop.current?.querySelectorAll<HTMLButtonElement>(ITEM) ?? []);

    useLayoutEffect(() => {
        if (!open || !root.current || !pop.current) return;
        const composer = root.current.closest('.ttia-composer');
        if (!composer) throw new Error('in-app-agent: a composer menu must be inside the composer');
        const overflow = root.current.getBoundingClientRect().left + pop.current.offsetWidth - composer.getBoundingClientRect().right;
        pop.current.style.setProperty('--ttia-pop-x', `${-Math.max(0, overflow)}px`);
        setLit(keyboard.current);
        keyboard.current = true;
        const items = rows();
        (items.find(row => row.getAttribute('aria-checked') === 'true') ?? items[0])?.focus();
    }, [open]);
    useEffect(() => {
        if (!open) return;
        const dismiss = (event: PointerEvent) => {
            if (!(event.target instanceof Node && root.current?.contains(event.target))) onOpenChange(false);
        };
        document.addEventListener('pointerdown', dismiss, true);
        return () => document.removeEventListener('pointerdown', dismiss, true);
    }, [open, onOpenChange]);

    function close() {
        button.current?.focus({ preventScroll: true });
        onOpenChange(false);
    }
    function onKeyDown(event: KeyboardEvent) {
        const items = rows();
        const current = items.findIndex(row => row === document.activeElement);
        const move = (index: number) => {
            event.preventDefault(); setLit(true);
            items[(index + items.length) % items.length]?.focus();
        };
        if (event.key === 'ArrowDown') move(current + 1);
        else if (event.key === 'ArrowUp') move(current - 1);
        else if (event.key === 'Home') move(0);
        else if (event.key === 'End') move(items.length - 1);
        else if (event.key === 'Escape') { event.preventDefault(); close(); }
        else if (event.key === 'Tab') close();
    }
    function onPointerOver(event: ReactPointerEvent) {
        const row = event.target instanceof Element ? event.target.closest<HTMLButtonElement>(ITEM) : null;
        if (!row) return;
        if (row !== document.activeElement) row.focus({ preventScroll: true });
        setLit(true);
    }

    return <div className={`ttia-menu ${className}`} data-state={state} ref={root}>
        <button ref={button} type="button" className="ttia-chip" disabled={disabled}
            aria-haspopup="menu" aria-expanded={open} aria-controls={menuId} aria-label={label} title={title}
            onClick={event => {
                if (open) { close(); return; }
                keyboard.current = event.detail === 0; onOpenChange(true);
            }}
            onKeyDown={event => {
                if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
                event.preventDefault(); keyboard.current = true; onOpenChange(true);
            }}>{trigger}</button>
        <div ref={pop} className="ttia-menu-pop" id={menuId} role="menu" tabIndex={-1} aria-label={name} data-open={open}
            inert={!open} aria-hidden={!open} onKeyDown={onKeyDown}>
            {/* Choosing a row closes the menu first, so the row's own action can move focus onward. */}
            <div className="ttia-menu-list" onPointerOver={onPointerOver} onPointerLeave={() => setLit(false)} onClickCapture={event => {
                if (event.target instanceof Element && event.target.closest(ITEM)) close();
            }} onFocus={event => {
                if (!(event.target instanceof HTMLButtonElement) || !glide.current) return;
                glide.current.style.setProperty('--ttia-glide-y', `${event.target.offsetTop}px`);
                glide.current.style.setProperty('--ttia-glide-h', `${event.target.offsetHeight}px`);
            }}>
                <span ref={glide} className="ttia-glide" data-lit={open && lit} aria-hidden="true" />
                {children}
            </div>
        </div>
    </div>;
}

export function MenuRadio({ checked, label, detail, title, onSelect }: {
    checked: boolean; label: string; detail?: string | undefined; title?: string | undefined; onSelect: () => void;
}) {
    return <button type="button" tabIndex={-1} className="ttia-menu-item" role="menuitemradio" aria-checked={checked} title={title} onClick={onSelect}>
        <span className="ttia-menu-label">{label}</span>{detail && <span className="ttia-menu-detail" aria-hidden="true">{detail}</span>}<Icon name="check" />
    </button>;
}
