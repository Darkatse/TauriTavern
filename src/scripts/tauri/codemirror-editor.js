// @ts-check

import { getCodeMirrorEditor } from '../../lib.js';
import { callGenericPopup, POPUP_TYPE } from '../popup.js';

/** @type {boolean | null} */
let enabled = null;

/** @type {WeakMap<HTMLTextAreaElement, any>} */
const mountedEditors = new WeakMap();

/** @param {Record<string, any>} settings */
export function initializeCodeMirrorEditor(settings) {
    const value = settings?.codemirror_editor_enabled;
    if (typeof value !== 'boolean') {
        throw new TypeError('CodeMirror editor setting must be a boolean');
    }
    enabled = value;
}

/**
 * @param {HTMLTextAreaElement} source
 * @param {{ onChange?: () => void }} [options]
 */
export async function mountCodeMirrorEditor(source, { onChange } = {}) {
    if (enabled === null) {
        throw new Error('CodeMirror editor is not initialized');
    }
    if (!enabled) {
        return null;
    }
    if (!(source instanceof HTMLTextAreaElement)) {
        throw new TypeError('CodeMirror source must be a textarea');
    }

    const existing = mountedEditors.get(source);
    if (existing) {
        return existing;
    }

    const { createCodeMirrorView } = await getCodeMirrorEditor();
    if (!source.isConnected) {
        return null;
    }
    const loadedEditor = mountedEditors.get(source);
    if (loadedEditor) {
        return loadedEditor;
    }

    const sourceStyle = getComputedStyle(source);
    const height = source.offsetHeight;
    const wrapper = document.createElement('div');
    wrapper.className = 'tt-codemirror-editor';
    wrapper.style.flex = sourceStyle.flex;
    wrapper.style.font = sourceStyle.font;
    wrapper.style.textAlign = sourceStyle.textAlign;
    wrapper.style.width = '100%';
    wrapper.style.height = `${height}px`;
    wrapper.style.overflow = 'hidden';
    source.insertAdjacentElement('afterend', wrapper);

    const sourceDisplay = source.style.display;
    const label = source.labels?.[0]?.textContent?.trim();

    const editor = createCodeMirrorView(wrapper, {
        doc: source.value,
        readOnly: source.disabled || source.readOnly,
        ariaLabel: label || source.placeholder || 'Text editor',
        onChange,
    });

    const handle = {
        wrapper,
        height,
        focus: editor.focus,
        requestMeasure: editor.requestMeasure,
        reset() {
            editor.reset(source.value, source.disabled || source.readOnly);
        },
        flush({ input = false } = {}) {
            const value = editor.getValue();
            if (source.value === value) {
                return false;
            }
            source.value = value;
            input && source.dispatchEvent(new Event('input', { bubbles: true }));
            return true;
        },
        destroy() {
            mountedEditors.delete(source);
            editor.destroy();
            wrapper.remove();
            source.style.display = sourceDisplay;
        },
    };

    mountedEditors.set(source, handle);
    source.style.display = 'none';
    return handle;
}

/** @param {HTMLTextAreaElement} source */
export function getMountedCodeMirrorEditor(source) {
    return mountedEditors.get(source) ?? null;
}

/** @param {HTMLTextAreaElement} source */
export async function showCodeMirrorEditorFullscreen(source) {
    const editor = mountedEditors.get(source);
    if (!editor) {
        return false;
    }

    const host = document.createElement('div');
    host.className = 'height100p wide100p flex-container flexFlowColumn';
    host.append(editor.wrapper);
    editor.wrapper.style.height = '100%';
    editor.requestMeasure();

    try {
        await callGenericPopup(host, POPUP_TYPE.TEXT, '', { wide: true, large: true });
    } finally {
        editor.wrapper.style.height = `${editor.height}px`;
        if (source.isConnected) {
            source.insertAdjacentElement('afterend', editor.wrapper);
            editor.requestMeasure();
        } else {
            editor.destroy();
        }
    }

    return true;
}
