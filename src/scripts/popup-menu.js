/**
 * Shared disclosure behavior for action menus.
 * Business actions and positioning stay with the caller.
 * @param {HTMLElement} trigger
 * @param {HTMLElement} panel
 * @param {{ onOpen: () => void, closesOnClick: (target: Element) => boolean }} options
 * @returns {{ open: () => void, close: () => void, isOpen: () => boolean }}
 */
export function initPopupMenu(trigger, panel, { onOpen, closesOnClick }) {
    const doc = panel.ownerDocument;
    trigger.setAttribute('aria-controls', panel.id);
    trigger.setAttribute('aria-expanded', 'false');
    panel.setAttribute('aria-labelledby', trigger.id);
    panel.tabIndex = -1;

    function setOpen(open) {
        const focusInside = panel.contains(doc.activeElement);
        panel.style.display = open ? 'flex' : 'none';
        trigger.setAttribute('aria-expanded', String(open));
        if (open) {
            onOpen();
            panel.focus({ preventScroll: true });
        } else if (focusInside) {
            trigger.focus({ preventScroll: true });
        }
    }

    trigger.addEventListener('click', () => setOpen(panel.style.display === 'none'));
    for (const element of [trigger, panel]) {
        element.addEventListener('keydown', event => {
            if (event.key !== 'Escape' || event.defaultPrevented || event.isComposing || event.keyCode === 229
                || panel.style.display === 'none') return;
            event.preventDefault();
            event.stopPropagation();
            setOpen(false);
        });
    }
    // Close before an action opens a dialog, so that dialog restores a visible trigger.
    panel.addEventListener('click', event => {
        if (event.target instanceof Element && closesOnClick(event.target)) setOpen(false);
    }, { capture: true });
    doc.addEventListener('click', event => {
        if (panel.style.display !== 'none' && event.target instanceof Node
            && !panel.contains(event.target) && !trigger.contains(event.target)) setOpen(false);
    });
    return {
        open: () => setOpen(true),
        close: () => setOpen(false),
        isOpen: () => panel.style.display !== 'none',
    };
}
