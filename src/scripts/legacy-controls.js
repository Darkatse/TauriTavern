/** DOM-only compatibility for the old controls shared by login and the main app. */
export const INTERACTABLE_CONTROL_CLASS = 'interactable';
export const CUSTOM_INTERACTABLE_CONTROL_CLASS = 'custom_interactable';
export const NOT_FOCUSABLE_CONTROL_CLASS = 'not_focusable';
export const DISABLED_CONTROL_CLASS = 'disabled';

// These rows retain their existing Enter action without claiming that their whole
// contents, including independent child controls, form one button.
const rowSelectors = [
    '.group_select', '.character_select', '.bogus_folder_select',
    '.swipe_picker_block', '.avatar-container', '.bg_example', '.select_chat_block',
    '#userList .userSelect',
].join(',');

let interactableSelectors = [
    '.interactable', '.custom_interactable', '.menu_button', '.right_menu_button',
    '.drawer-icon', '.inline-drawer-icon', '.paginationjs-pages li a',
    rowSelectors, '.tag .tag_remove', '.jg-menu .jg-button',
    '.bg_example .mobile-only-menu-toggle', '#options a', '.mes_button',
    '.extraMesButtons>div:not(.mes_button)', '.swipe_left', '.swipe_right',
    '.stscript_btn',
    '.select2_choice_clickable+span.select2-container .select2-selection__choice__display',
    '.avatar_load_preview', '#show_more_messages',
    '.select_chat_block .exportRawChatButton', '.select_chat_block .exportChatButton',
    '.select_chat_block .PastChat_cross', '.select_chat_block .renameChatButton',
    '#extensionsMenu .list-group-item',
].join(',');

// Only concrete collection structures supply list semantics. Generic styling
// such as .list-group, and jQuery UI's tabs, do not belong to this adapter.
const structuralRoles = [
    [[
        '#rm_print_characters_block', '#rm_group_members', '#rm_group_add_members',
        '.tag_view_list_tags', '.secretKeyManagerList', '.recentChatList',
        '.dataMaidCategoryContent', '#userList', '.bg_list',
    ].join(','), 'list'],
    [[
        '#rm_print_characters_block .entity_block', '#rm_group_members .group_member',
        '#rm_group_add_members .group_member', '.tag_view_list_tags .tag_view_item',
        '.secretKeyManagerList .secretKeyManagerItem', '.recentChatList .recentChat',
        '.dataMaidCategoryContent .dataMaidItem', '#userList .userSelect',
        '.bg_list .bg_example',
    ].join(','), 'listitem'],
    ['.jg-menu', 'toolbar'],
    ['#toast-container .toast', 'status'],
];
const structuralSelectors = structuralRoles.map(([selector]) => selector).join(',');
const nativeControls = 'button,input,select,textarea,a[href],area[href],summary,iframe,object,embed,audio[controls],video[controls]';
const initializedDocuments = new WeakSet();
const inferredRoles = new WeakMap();
const addedTabIndices = new WeakSet();

/** @param {Element} control */
export function isKeyboardInteractable(control) {
    return control.matches(interactableSelectors);
}

/**
 * Unavailability belongs to the control, not arbitrary business-state ancestors.
 * Native :disabled also preserves the browser's disabled fieldset semantics.
 * @param {Element} control
 * @returns {boolean}
 */
export function isControlDisabled(control) {
    if (control.matches(':disabled') || control.classList.contains(DISABLED_CONTROL_CLASS)
        || control.getAttribute('aria-disabled') === 'true') {
        return true;
    }
    const associatedControl = control.localName === 'label'
        ? /** @type {HTMLLabelElement} */ (control).control : null;
    return Boolean(associatedControl && isControlDisabled(associatedControl));
}

/** @param {Element} control */
function isEditable(control) {
    const editable = control.closest('[contenteditable]');
    return editable !== null && editable.getAttribute('contenteditable') !== 'false';
}

/** @param {Element} control */
function isNativeActionContent(control) {
    // Old icon classes can remain inside a native trigger without creating a
    // second action. Labels retain their explicit form-control proxy behavior.
    return control.localName !== 'label' && Boolean(control.parentElement?.closest('button,a[href],summary'));
}

/** @param {Element} control */
function needsLegacyActivation(control) {
    return isKeyboardInteractable(control) && !control.matches(nativeControls)
        && !isEditable(control) && !isNativeActionContent(control);
}

/** @param {Element} control */
function getFallbackRole(control) {
    // Add semantics only to the neutral elements used by the old markup. This
    // leaves labels, headings, images and native controls with their own meaning.
    if (!control.matches('div,span,i,a:not([href])') || isEditable(control) || isNativeActionContent(control)) return null;
    // Menu groups are layout, even when an extension opts the wrapper into focus.
    if (control.matches('#extensionsMenu .extension_container')) return null;
    for (const [selector, role] of structuralRoles) {
        if (control.matches(selector)) return role;
    }
    return isKeyboardInteractable(control) && !control.matches(rowSelectors) ? 'button' : null;
}

/** @param {Element} control */
function syncControl(control) {
    const previousRole = inferredRoles.get(control);
    const currentRole = control.getAttribute('role');
    const fallbackRole = getFallbackRole(control);
    if (!control.hasAttribute('role') || currentRole === previousRole) {
        if (fallbackRole) {
            if (currentRole !== fallbackRole) control.setAttribute('role', fallbackRole);
            inferredRoles.set(control, fallbackRole);
        } else if (previousRole) {
            control.removeAttribute('role');
            inferredRoles.delete(control);
        }
    } else {
        inferredRoles.delete(control);
    }

    if (!isKeyboardInteractable(control)) return;
    if (!control.classList.contains(INTERACTABLE_CONTROL_CLASS)) {
        control.classList.add(INTERACTABLE_CONTROL_CLASS);
    }
    if (!needsLegacyActivation(control)) {
        if (addedTabIndices.has(control) && control.getAttribute('tabindex') === '0') {
            control.removeAttribute('tabindex');
        }
        addedTabIndices.delete(control);
        return;
    }

    if (isControlDisabled(control) || control.closest(`.${NOT_FOCUSABLE_CONTROL_CLASS}`)) {
        if (control.hasAttribute('tabindex')) {
            control.setAttribute('data-original-tabindex', control.getAttribute('tabindex'));
            control.removeAttribute('tabindex');
        }
    } else if (!control.hasAttribute('tabindex')) {
        const original = control.getAttribute('data-original-tabindex');
        control.setAttribute('tabindex', original ?? '0');
        if (original === null) addedTabIndices.add(control);
    }
}

/** @param {Element|Document} root */
function initializeSubtree(root) {
    if (root instanceof Element) syncControl(root);
    root.querySelectorAll(`${interactableSelectors},${structuralSelectors}`).forEach(syncControl);
}

/**
 * Keeps the existing extension API: defaults apply to controls present at registration.
 * @param {string} selector
 * @param {{disabledByDefault?: boolean, notFocusableByDefault?: boolean}} [options]
 */
export function registerInteractableType(selector, { disabledByDefault = false, notFocusableByDefault = false } = {}) {
    // Validate before changing the shared selector, so a bad registration cannot
    // break every later scan.
    const controls = document.querySelectorAll(selector);
    interactableSelectors += `,${selector}`;
    for (const control of controls) {
        if (disabledByDefault) control.classList.add(DISABLED_CONTROL_CLASS);
        if (notFocusableByDefault) control.classList.add(NOT_FOCUSABLE_CONTROL_CLASS);
        syncControl(control);
    }
}

/** @param {Element[]} controls */
export function makeKeyboardInteractable(...controls) {
    for (const control of controls) {
        if (!isKeyboardInteractable(control)) control.classList.add(CUSTOM_INTERACTABLE_CONTROL_CLASS);
        syncControl(control);
    }
}

/** @param {KeyboardEvent} event */
function isUnmodifiedActivation(event) {
    return !event.defaultPrevented && !event.isComposing
        && !event.altKey && !event.ctrlKey && !event.shiftKey && !event.metaKey;
}

/** Install once for both the login and main-app entry points. */
export function initLegacyControls() {
    const doc = document;
    if (initializedDocuments.has(doc)) return;
    initializedDocuments.add(doc);
    initializeSubtree(doc.body);

    const observer = new MutationObserver(mutations => {
        for (const mutation of mutations) {
            if (mutation.type === 'childList') {
                for (const node of mutation.addedNodes) {
                    if (node instanceof Element) initializeSubtree(node);
                }
                continue;
            }
            const control = /** @type {Element} */ (mutation.target);
            if (mutation.attributeName === 'href' && control.localName === 'a') {
                initializeSubtree(control);
            } else {
                syncControl(control);
            }
            // Labels can proxy controls elsewhere in the document.
            if ('labels' in control) {
                for (const label of /** @type {HTMLInputElement} */ (control).labels ?? []) syncControl(label);
            }
            if (mutation.attributeName === 'class') {
                const wasNotFocusable = mutation.oldValue?.split(/\s+/).includes(NOT_FOCUSABLE_CONTROL_CLASS) ?? false;
                if (wasNotFocusable !== control.classList.contains(NOT_FOCUSABLE_CONTROL_CLASS)) {
                    control.querySelectorAll(interactableSelectors).forEach(syncControl);
                }
            }
        }
    });
    observer.observe(doc.body, {
        childList: true, subtree: true, attributes: true, attributeOldValue: true,
        attributeFilter: ['class', 'role', 'disabled', 'aria-disabled', 'href', 'contenteditable', 'for'],
    });

    /** @type {HTMLElement|null} */
    let spaceTarget = null;
    doc.addEventListener('keydown', event => {
        if (event.key !== 'Enter' && event.key !== ' ') return;
        const target = event.target;
        if (!isUnmodifiedActivation(event) || !(target instanceof HTMLElement) || !needsLegacyActivation(target)) return;
        if (isControlDisabled(target)) {
            event.preventDefault();
            return;
        }
        if (event.key === 'Enter') {
            event.preventDefault();
            target.click();
        } else if (target.localName === 'label' || (target.getAttribute('role') ?? getFallbackRole(target)) === 'button') {
            event.preventDefault();
            if (!event.repeat) spaceTarget = target;
        }
    });
    doc.addEventListener('keyup', event => {
        if (event.key !== ' ') return;
        const target = spaceTarget;
        spaceTarget = null;
        if (target && isUnmodifiedActivation(event) && event.target === target && doc.activeElement === target
            && needsLegacyActivation(target) && !isControlDisabled(target)) {
            event.preventDefault();
            target.click();
        }
    });
    doc.addEventListener('focusout', event => {
        if (event.target === spaceTarget) spaceTarget = null;
    });
}
