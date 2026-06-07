const EXPAND_BUTTON_ID = 'send_textarea_maximize';
const EDITOR_DIALOG_ID = 'tt-chat-input-editor';
const EDITOR_TITLE_ID = 'tt-chat-input-editor-title';
const HEIGHT_EPSILON_PX = 1;

function requireElement(id, type) {
    const element = document.getElementById(id);
    if (!(element instanceof type)) {
        throw new Error(`Expected #${id} to be ${type.name}`);
    }
    return element;
}

function parsePixelValue(value, propertyName) {
    const number = Number.parseFloat(value);
    if (!Number.isFinite(number)) {
        throw new Error(`Expected ${propertyName} to resolve to a pixel value, got: ${value}`);
    }
    return number;
}

function createIconButton({ className, icon, title }) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = className;
    button.title = title;
    button.setAttribute('aria-label', title);
    button.dataset.i18n = `[title]${title};[aria-label]${title}`;

    const iconElement = document.createElement('i');
    iconElement.className = `fa-solid ${icon}`;
    iconElement.setAttribute('aria-hidden', 'true');
    button.appendChild(iconElement);

    return button;
}

function createEditorDialog(sourceTextarea) {
    const dialog = document.createElement('dialog');
    dialog.id = EDITOR_DIALOG_ID;
    dialog.className = 'tt-chat-input-editor';
    dialog.setAttribute('data-tt-mobile-surface', 'fullscreen-window');
    dialog.setAttribute('aria-labelledby', EDITOR_TITLE_ID);

    const surface = document.createElement('div');
    surface.className = 'tt-chat-input-editor__surface';

    const header = document.createElement('header');
    header.className = 'tt-chat-input-editor__header';

    const title = document.createElement('div');
    title.id = EDITOR_TITLE_ID;
    title.className = 'tt-chat-input-editor__title';
    title.dataset.i18n = 'Send a message';
    title.textContent = 'Send a message';

    const actions = document.createElement('div');
    actions.className = 'tt-chat-input-editor__actions';

    const collapseButton = createIconButton({
        className: 'tt-chat-input-editor__button menu_button menu_button_icon result-control',
        icon: 'fa-compress',
        title: 'Close popup',
    });
    collapseButton.dataset.ttChatInputEditorAction = 'collapse';

    const sendButton = createIconButton({
        className: 'tt-chat-input-editor__button tt-chat-input-editor__button--send menu_button menu_button_icon result-control',
        icon: 'fa-paper-plane',
        title: 'Send a message',
    });
    sendButton.dataset.ttChatInputEditorAction = 'send';

    actions.append(collapseButton, sendButton);
    header.append(title, actions);

    const textarea = document.createElement('textarea');
    textarea.className = 'tt-chat-input-editor__textarea text_pole textarea_compact mdHotkeys';
    textarea.autocomplete = 'off';
    textarea.placeholder = sourceTextarea.placeholder;
    textarea.spellcheck = sourceTextarea.spellcheck;
    textarea.setAttribute('aria-label', sourceTextarea.getAttribute('aria-label') || 'Send a message');

    surface.append(header, textarea);
    dialog.append(surface);
    document.body.appendChild(dialog);

    return { dialog, textarea, collapseButton, sendButton };
}

/**
 * Installs the fullscreen editor for the main chat input.
 *
 * The fullscreen textarea is a temporary editing surface. It only commits back
 * to #send_textarea on close/send, preserving the upstream chat input event
 * contract without running the expensive composer input pipeline on every key.
 *
 * @param {object} deps
 * @param {HTMLTextAreaElement} deps.sendTextArea
 * @param {() => Promise<void>|void} deps.sendMessage
 * @param {() => boolean} deps.isMobile
 */
export function installChatInputFullscreenEditor({ sendTextArea, sendMessage, isMobile }) {
    if (!(sendTextArea instanceof HTMLTextAreaElement)) {
        throw new Error('sendTextArea must be an HTMLTextAreaElement');
    }
    if (typeof sendMessage !== 'function') {
        throw new Error('sendMessage must be a function');
    }
    if (typeof isMobile !== 'function') {
        throw new Error('isMobile must be a function');
    }

    const leftSendForm = requireElement('leftSendForm', HTMLElement);
    const expandButton = createIconButton({
        className: 'tt-chat-input-expand-button interactable',
        icon: 'fa-maximize',
        title: 'Expand the editor',
    });
    expandButton.id = EXPAND_BUTTON_ID;
    expandButton.hidden = true;
    leftSendForm.appendChild(expandButton);

    const { dialog, textarea, collapseButton, sendButton } = createEditorDialog(sendTextArea);

    let pendingButtonFrame = 0;
    let pendingSourceFocus = false;
    let closingReason = '';

    const queueButtonVisibilityUpdate = () => {
        if (pendingButtonFrame) {
            return;
        }

        pendingButtonFrame = requestAnimationFrame(() => {
            pendingButtonFrame = 0;
            const minHeight = parsePixelValue(getComputedStyle(sendTextArea).minHeight, '#send_textarea min-height');
            expandButton.hidden = !sendTextArea.value || sendTextArea.scrollHeight <= minHeight + HEIGHT_EPSILON_PX;
        });
    };

    const commitEditorValue = () => {
        if (sendTextArea.value !== textarea.value) {
            sendTextArea.value = textarea.value;
            sendTextArea.dispatchEvent(new Event('input', { bubbles: true }));
        }

        sendTextArea.setSelectionRange(textarea.selectionStart, textarea.selectionEnd);
        queueButtonVisibilityUpdate();
    };

    const closeEditor = ({ focusSource = false, reason = 'collapse' } = {}) => {
        if (!dialog.open) {
            return;
        }

        pendingSourceFocus = focusSource;
        closingReason = reason;
        dialog.close();
    };

    const sendFromEditor = async () => {
        commitEditorValue();
        closeEditor({ focusSource: false, reason: 'send' });
        await sendMessage();
    };

    const openEditor = () => {
        if (typeof dialog.showModal !== 'function') {
            throw new Error('HTML dialog modal API is required for the chat input fullscreen editor.');
        }

        textarea.value = sendTextArea.value;
        textarea.placeholder = sendTextArea.placeholder;

        if (!dialog.open) {
            dialog.showModal();
        }

        textarea.focus();
        textarea.setSelectionRange(sendTextArea.selectionStart, sendTextArea.selectionEnd);
    };

    expandButton.addEventListener('click', (event) => {
        event.preventDefault();
        openEditor();
    });

    collapseButton.addEventListener('click', () => {
        closeEditor({ focusSource: !isMobile(), reason: 'collapse' });
    });

    sendButton.addEventListener('click', () => {
        void sendFromEditor();
    });

    textarea.addEventListener('keydown', (event) => {
        if (event.key === 'Enter' && (event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey && !event.isComposing) {
            event.preventDefault();
            void sendFromEditor();
            return;
        }

        if (event.key === 'Escape') {
            event.preventDefault();
            closeEditor({ focusSource: !isMobile(), reason: 'cancel' });
        }
    });

    dialog.addEventListener('keydown', (event) => {
        event.stopPropagation();
    });

    dialog.addEventListener('cancel', (event) => {
        event.preventDefault();
        closeEditor({ focusSource: !isMobile(), reason: 'cancel' });
    });

    dialog.addEventListener('close', () => {
        if (closingReason !== 'send') {
            commitEditorValue();
        }

        closingReason = '';
        if (pendingSourceFocus) {
            sendTextArea.focus();
        }
        pendingSourceFocus = false;
    });

    sendTextArea.addEventListener('input', queueButtonVisibilityUpdate);
    queueButtonVisibilityUpdate();

    return {
        open: openEditor,
        close: closeEditor,
        refresh: queueButtonVisibilityUpdate,
    };
}
