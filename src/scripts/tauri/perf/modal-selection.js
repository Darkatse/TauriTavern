// @ts-check

/**
 * A modal dialog makes the rest of the page inert. Chromium keeps the selection there and, on every
 * frame, searches the page for a position to show it at; WebKit drops it. Focus entering a modal
 * dialog from outside drops it too, unless an editable element takes the selection. Engines without
 * `:modal` predate Chrome 105 and Safari 15.6.
 */
export function installModalSelectionRelease() {
    if (!CSS.supports('selector(:modal)')) {
        return;
    }
    document.addEventListener('focusin', ({ target, relatedTarget }) => {
        if (!(target instanceof Element)) {
            return;
        }
        const dialog = target.closest('dialog');
        if (!dialog?.matches(':modal') || target.matches(':read-write')) {
            return;
        }
        if (relatedTarget instanceof Node && dialog.contains(relatedTarget)) {
            return;
        }
        document.getSelection()?.removeAllRanges();
    }, true);
}
