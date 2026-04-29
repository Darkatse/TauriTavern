/* All selectors that should act as interactables / keyboard buttons by default */
const interactableSelectors = [
    '.interactable', // Main interactable class for ALL interactable controls (can also be manually added in code, so that's why its listed here)
    '.custom_interactable', // Manually made interactable controls via code (see 'makeKeyboardInteractable()')
    '.menu_button', // General menu button in ST
    '.right_menu_button', // Button-likes in many menus
    '.drawer-icon', // Main "menu bar" icons
    '.inline-drawer-icon', // Buttons/icons inside the drawer menus
    '.paginationjs-pages li a', // Pagination buttons
    '.group_select, .character_select, .bogus_folder_select', // Cards to select char, group or folder in character list and other places
    '.avatar-container', // Persona list blocks
    '.tag .tag_remove', // Remove button in removable tags
    '.bg_example', // Background elements in the background menu
    '.bg_example .jg-button, .bg_example .mobile-only-menu-toggle', // The inline buttons on the backgrounds
    '#options a', // Option entries in the popup options menu
    '.mes_buttons .mes_button', // Small inline buttons on the chat messages
    '.extraMesButtons>div:not(.mes_button)', // The extra/extension buttons inline on the chat messages
    '.swipe_left, .swipe_right', // Swipe buttons on the last message
    '.mes_stop', // Stop generation button in the chat composer
    '.stscript_btn', // STscript buttons in the chat bar
    '.select2_choice_clickable+span.select2-container .select2-selection__choice__display', // select2 control elements if they are meant to be clickable
    '.avatar_load_preview', // Char display avatar selection
    '.bg_tabs_list .bg_tab_button', // Background tabs
];

if (CSS.supports('selector(:has(*))')) {
    // Option entries in the extension menu popup that are coming from extensions
    interactableSelectors.push('#extensionsMenu div:has(.extensionsMenuExtensionButton)');
}

export const INTERACTABLE_CONTROL_CLASS = 'interactable';
export const CUSTOM_INTERACTABLE_CONTROL_CLASS = 'custom_interactable';

export const NOT_FOCUSABLE_CONTROL_CLASS = 'not_focusable';
export const DISABLED_CONTROL_CLASS = 'disabled';

/**
 * @param {Element} element
 * @returns {boolean}
 */
function hasDisabledOrNotFocusableAncestor(element) {
    let currentElement = element;
    while (currentElement) {
        if (currentElement.classList.contains(NOT_FOCUSABLE_CONTROL_CLASS) || currentElement.classList.contains(DISABLED_CONTROL_CLASS)) {
            return true;
        }
        currentElement = currentElement.parentElement;
    }
    return false;
}

/**
 * @param {HTMLElement} element
 * @returns {boolean}
 */
function isNativeKeyboardControl(element) {
    return element.matches('button, input, select, textarea, summary, a[href]');
}

/**
 * @param {HTMLElement} element
 * @returns {boolean}
 */
function isEditableTarget(element) {
    return element.matches('input, textarea, select') || element.isContentEditable;
}

/**
 * @param {KeyboardEvent} event
 * @returns {boolean}
 */
function isActivationKey(event) {
    return event.key === 'Enter' || event.key === ' ' || event.key === 'Spacebar';
}

/**
 * @param {HTMLElement} start
 * @returns {HTMLElement | null}
 */
function findKeyboardInteractable(start) {
    let target = start;
    while (target) {
        if (isKeyboardInteractable(target)) {
            return target;
        }
        target = target.parentElement;
    }
    return null;
}

/**
 * @param {HTMLElement} element
 * @returns {string}
 */
function getAccessibleNameFromMetadata(element) {
    const title = element.getAttribute('title')?.trim();
    if (title) {
        return title;
    }

    const tooltip = element.getAttribute('data-tooltip')?.trim();
    if (tooltip) {
        return tooltip.split('\n')[0].trim();
    }

    return '';
}

/**
 * @param {HTMLElement} interactable
 */
function syncInteractableAccessibility(interactable) {
    if (!isNativeKeyboardControl(interactable) && !interactable.hasAttribute('role') && !interactable.matches('.bg_tabs_list .bg_tab_button')) {
        interactable.setAttribute('role', 'button');
    }

    if (!interactable.hasAttribute('aria-label') && !interactable.hasAttribute('aria-labelledby')) {
        const accessibleName = getAccessibleNameFromMetadata(interactable);
        if (accessibleName) {
            interactable.setAttribute('aria-label', accessibleName);
        }
    }

    if (hasDisabledOrNotFocusableAncestor(interactable)) {
        interactable.setAttribute('aria-disabled', 'true');
    } else {
        interactable.removeAttribute('aria-disabled');
    }
}

/**
 * An observer that will check if any new interactables or scroll reset containers are added to the body
 * @type {MutationObserver}
 */
const observer = new MutationObserver(mutations => {
    mutations.forEach(mutation => {
        if (mutation.type === 'childList') {
            mutation.addedNodes.forEach(handleNodeChange);
        }
        if (mutation.type === 'attributes') {
            const target = mutation.target;
            if (mutation.attributeName === 'class' && target instanceof Element) {
                handleNodeChange(target);
            }
        }
    });
});

/**
 * Function to handle node changes (added or modified nodes)
 * @param {Element} node
 */
function handleNodeChange(node) {
    if (node.nodeType === Node.ELEMENT_NODE && node instanceof Element) {
        // Handle keyboard interactables
        if (isKeyboardInteractable(node)) {
            makeKeyboardInteractable(node);
        }
        initializeInteractables(node);

        // Handle scroll reset containers
        if (node.classList.contains('scroll-reset-container')) {
            applyScrollResetBehavior(node);
        }
        initializeScrollResetBehaviors(node);
    }
}

/**
 * Registers an interactable class (for example for an extension) and makes it keyboard interactable.
 * Optionally apply the 'not_focusable' and 'disabled' classes if needed.
 *
 * @param {string} interactableSelector - The CSS selector for the interactable (Supports class combinations, chained via dots like <c>tag.actionable</c>, and sub selectors)
 * @param {object} [options={}] - Optional settings for the interactable
 * @param {boolean} [options.disabledByDefault=false] - Whether interactables of this class should be disabled by default
 * @param {boolean} [options.notFocusableByDefault=false] - Whether interactables of this class should not be focusable by default
 */
export function registerInteractableType(interactableSelector, { disabledByDefault = false, notFocusableByDefault = false } = {}) {
    interactableSelectors.push(interactableSelector);

    const interactables = document.querySelectorAll(interactableSelector);

    if (disabledByDefault || notFocusableByDefault) {
        interactables.forEach(interactable => {
            if (disabledByDefault) interactable.classList.add(DISABLED_CONTROL_CLASS);
            if (notFocusableByDefault) interactable.classList.add(NOT_FOCUSABLE_CONTROL_CLASS);
        });
    }

    makeKeyboardInteractable(...interactables);
}

/**
 * Checks if the given control is a keyboard-enabled interactable.
 *
 * @param {Element} control - The control element to check
 * @returns {boolean} Returns true if the control is a keyboard interactable, false otherwise
 */
export function isKeyboardInteractable(control) {
    // Check if this control matches any of the selectors
    return interactableSelectors.some(selector => control.matches(selector));
}

/**
 * Makes all the given controls keyboard interactable and sets their state.
 * If the control doesn't have any of the classes, it will be set to a custom-enabled keyboard interactable.
 *
 * @param {Element[]} interactables - The controls to make interactable and set their state
 */
export function makeKeyboardInteractable(...interactables) {
    interactables.forEach(interactable => {
        // If this control doesn't have any of the classes, lets say the caller knows this and wants this to be a custom-enabled keyboard control.
        if (!isKeyboardInteractable(interactable)) {
            interactable.classList.add(CUSTOM_INTERACTABLE_CONTROL_CLASS);
        }

        // Just for CSS styling and future reference, every keyboard interactable control should have a common class
        if (!interactable.classList.contains(INTERACTABLE_CONTROL_CLASS)) {
            interactable.classList.add(INTERACTABLE_CONTROL_CLASS);
        }

        syncInteractableAccessibility(interactable);

        // Set/remove the tabindex accordingly to the classes. Remembering if it had a custom value.
        if (!hasDisabledOrNotFocusableAncestor(interactable)) {
            if (!interactable.hasAttribute('tabindex')) {
                const tabIndex = interactable.getAttribute('data-original-tabindex') ?? '0';
                interactable.setAttribute('tabindex', tabIndex);
            }
        } else {
            if (interactable.hasAttribute('tabindex') && !interactable.hasAttribute('data-original-tabindex')) {
                interactable.setAttribute('data-original-tabindex', interactable.getAttribute('tabindex'));
            }
            interactable.removeAttribute('tabindex');
        }
    });
}

/**
 * Initializes the focusability of controls on the given element or the document
 *
 * @param {Element|Document} [element=document] - The element on which to initialize the interactable state. Defaults to the document.
 */
function initializeInteractables(element = document) {
    const interactables = getAllInteractables(element);
    makeKeyboardInteractable(...interactables);
}

/**
 * Queries all interactables within the given element based on the given selectors and returns them as an array
 *
 * @param {Element|Document} element - The element within which to query the interactables
 * @returns {HTMLElement[]} An array containing all the interactables that match the given selectors
 */
function getAllInteractables(element) {
    // Query each selector individually and combine all to a big array to return
    return [].concat(...interactableSelectors.map(selector => Array.from(element.querySelectorAll(`${selector}`))));
}

/**
 * Function to apply scroll reset behavior to a container
 * @param {Element} container - The container
 */
const applyScrollResetBehavior = (container) => {
    container.addEventListener('focusout', (e) => {
        setTimeout(() => {
            const focusedElement = document.activeElement;
            if (!container.contains(focusedElement)) {
                container.scrollTop = 0;
                container.scrollLeft = 0;
            }
        }, 0);
    });
};

/**
 * Initializes the scroll reset behavior on the given element or the document
 *
 * @param {Element|Document} [element=document] - The element on which to initialize the scroll reset behavior. Defaults to the document.
 */
function initializeScrollResetBehaviors(element = document) {
    const scrollResetContainers = element.querySelectorAll('.scroll-reset-container');
    scrollResetContainers.forEach(container => applyScrollResetBehavior(container));
}

/**
 * Handles keydown events on the document to trigger click on Enter/Space key press for interactables
 *
 * @param {KeyboardEvent} event - The keyboard event
 */
function handleGlobalKeyDown(event) {
    if (!isActivationKey(event)) {
        return;
    }

    if (!(event.target instanceof HTMLElement)) {
        return;
    }

    if (event.altKey || event.ctrlKey || event.shiftKey || event.metaKey || isEditableTarget(event.target)) {
        return;
    }

    const target = findKeyboardInteractable(event.target);
    if (!target || hasDisabledOrNotFocusableAncestor(target) || isNativeKeyboardControl(target)) {
        return;
    }

    event.preventDefault();
    target.click();
}

/**
 * Initializes several keyboard functionalities for ST
 */
export function initKeyboard() {
    // Start observing the body for added elements and attribute changes
    observer.observe(document.body, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: ['class'],
    });

    // Initialize already existing controls
    initializeInteractables();
    initializeScrollResetBehaviors();

    // Add a global keydown listener
    document.addEventListener('keydown', handleGlobalKeyDown);
}
