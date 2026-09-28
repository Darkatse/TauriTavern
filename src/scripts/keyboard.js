import { initLegacyControls } from './legacy-controls.js';

export {
    INTERACTABLE_CONTROL_CLASS, CUSTOM_INTERACTABLE_CONTROL_CLASS,
    NOT_FOCUSABLE_CONTROL_CLASS, DISABLED_CONTROL_CLASS,
    registerInteractableType, isKeyboardInteractable, makeKeyboardInteractable,
} from './legacy-controls.js';

const initializedDocuments = new WeakSet();
const initializedScrollResetContainers = new WeakSet();

/** @param {Element} container */
function applyScrollResetBehavior(container) {
    if (initializedScrollResetContainers.has(container)) return;
    initializedScrollResetContainers.add(container);
    container.addEventListener('focusout', () => {
        setTimeout(() => {
            if (!container.contains(container.ownerDocument.activeElement)) {
                container.scrollTop = 0;
                container.scrollLeft = 0;
            }
        }, 0);
    });
}

/** @param {Element} root */
function initializeScrollResetBehaviors(root) {
    if (root.matches('.scroll-reset-container')) applyScrollResetBehavior(root);
    root.querySelectorAll('.scroll-reset-container').forEach(applyScrollResetBehavior);
}

export function initKeyboard() {
    initLegacyControls();
    if (initializedDocuments.has(document)) return;
    initializedDocuments.add(document);
    initializeScrollResetBehaviors(document.body);
    const observer = new MutationObserver(mutations => {
        for (const mutation of mutations) {
            if (mutation.type === 'childList') {
                for (const node of mutation.addedNodes) {
                    if (node instanceof Element) initializeScrollResetBehaviors(node);
                }
            } else if (mutation.target instanceof Element && mutation.target.matches('.scroll-reset-container')) {
                applyScrollResetBehavior(mutation.target);
            }
        }
    });
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['class'] });
}
