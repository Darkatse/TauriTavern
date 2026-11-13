import { Compartment, EditorState } from '@codemirror/state';
import { defaultKeymap, history, historyKeymap, isolateHistory, undo, redo, undoDepth, redoDepth } from '@codemirror/commands';
import { EditorView, ViewPlugin, highlightSpecialChars, keymap, placeholder, showPanel } from '@codemirror/view';
import { openSearchPanel, closeSearchPanel, searchPanelOpen, searchKeymap, search } from '@codemirror/search';
import { createSearchPanel, editorIcon } from './editor-search-panel.js';

const theme = EditorView.theme({
    '&': {
        height: '100%',
        color: 'var(--SmartThemeBodyColor)',
        font: 'inherit',
    },
    '&.cm-focused': {
        outline: '1px solid var(--SmartThemeQuoteColor)',
    },
    '.cm-content': { caretColor: 'var(--SmartThemeBodyColor)' },
    '::selection': {
        backgroundColor: 'color-mix(in srgb, var(--SmartThemeBodyColor) 25%, transparent)',
        color: 'var(--SmartThemeBodyColor)',
    },
    '.cm-scroller': {
        fontFamily: 'inherit',
        lineHeight: 'inherit',
        overflow: 'auto',
    },
    '.cm-panels': {
        backgroundColor: 'transparent',
        color: 'inherit',
    },
    // Pages turn the toolbar off with `--tt-editor-tools: none`; find and replace still opens.
    '.cm-panels-top': {
        display: 'var(--tt-editor-tools, block)',
        borderBottom: '1px solid color-mix(in srgb, currentColor 12%, transparent)',
    },
    '.cm-panels-top:has(.cm-editor-search)': { display: 'block' },
    '.cm-editor-tools': {
        display: 'var(--tt-editor-tools, flex)',
        justifyContent: 'flex-end',
        gap: '2px',
        padding: '2px 5px',
    },
    '.cm-editor-tools button, .cm-editor-search button': {
        display: 'grid',
        placeItems: 'center',
        width: '24px',
        height: '24px',
        padding: '0',
        margin: '0',
        border: '0',
        borderRadius: '3px',
        background: 'transparent',
        color: 'inherit',
        fontSize: '12px',
        cursor: 'pointer',
    },
    '.cm-editor-tools button:last-child': { marginInlineStart: '4px' },
    '.cm-editor-tools button:hover:not(:disabled), .cm-editor-tools button[aria-expanded=true], .cm-editor-search button:hover:not(:disabled), .cm-editor-search button[aria-pressed=true]': {
        backgroundColor: 'color-mix(in srgb, currentColor 10%, transparent)',
    },
    '.cm-editor-tools button:focus-visible, .cm-editor-search button:focus-visible': {
        outline: '1px solid var(--SmartThemeQuoteColor)',
        outlineOffset: '-1px',
    },
    '.cm-editor-tools button:disabled, .cm-editor-search button:disabled': { opacity: '0.3', cursor: 'default' },
    '.cm-editor-search': {
        display: 'grid',
        gridTemplateColumns: '20px minmax(0, 1fr) repeat(3, 24px)',
        alignItems: 'center',
        gap: '4px 3px',
        width: 'min(100%, 340px)',
        boxSizing: 'border-box',
        marginInlineStart: 'auto',
        padding: '4px 6px',
        font: '12px/1.4 var(--mainFontFamily)',
    },
    '.cm-editor-search [hidden]': { display: 'none' },
    '.cm-search-field': {
        gridColumn: '2',
        display: 'flex',
        alignItems: 'center',
        minWidth: '0',
        paddingInlineEnd: '2px',
        border: '1px solid color-mix(in srgb, currentColor 18%, transparent)',
        borderRadius: '4px',
        backgroundColor: 'color-mix(in srgb, currentColor 4%, transparent)',
    },
    '.cm-search-field:focus-within, .cm-search-replace input:focus': { borderColor: 'var(--SmartThemeQuoteColor)' },
    '.cm-editor-search input': {
        width: '100%',
        minWidth: '0',
        height: '24px',
        boxSizing: 'border-box',
        padding: '2px 6px',
        margin: '0',
        border: '0',
        background: 'transparent',
        color: 'inherit',
        font: 'inherit',
        outline: 'none',
    },
    '.cm-editor-search .cm-search-option': { flexShrink: '0', width: '22px', height: '22px', font: '12px/1 monospace' },
    '.cm-editor-search button[aria-pressed=true]': {
        backgroundColor: 'color-mix(in srgb, var(--SmartThemeQuoteColor) 18%, transparent)',
        boxShadow: 'inset 0 0 0 1px color-mix(in srgb, var(--SmartThemeQuoteColor) 45%, transparent)',
    },
    '.cm-search-wholeWord': { textDecoration: 'underline', textUnderlineOffset: '3px' },
    '.cm-editor-tools button svg, .cm-editor-search button svg': {
        width: '14px',
        height: '14px',
        fill: 'none',
        stroke: 'currentColor',
        strokeWidth: '1.5',
        strokeLinecap: 'round',
        strokeLinejoin: 'round',
    },
    '.cm-editor-search .cm-search-expand': { width: '20px' },
    '.cm-search-expand[aria-expanded=true] svg': { transform: 'rotate(90deg)' },
    '.cm-search-replace': { gridColumn: '2 / -1', display: 'flex', alignItems: 'center', gap: '6px' },
    '.cm-search-replace input': {
        flex: '1',
        border: '1px solid color-mix(in srgb, currentColor 18%, transparent)',
        borderRadius: '4px',
        backgroundColor: 'color-mix(in srgb, currentColor 4%, transparent)',
    },
    '.cm-search-replace button': { width: 'auto', padding: '0 5px', whiteSpace: 'nowrap' },
    '.cm-search-status': { gridColumn: '2 / -1', fontSize: '12px', opacity: '0.8' },
    '.cm-search-status:empty': { display: 'none' },
    '.cm-search-field:has([aria-invalid=true])': { borderColor: 'var(--warning)' },
    '.cm-searchMatch': { backgroundColor: 'color-mix(in srgb, var(--SmartThemeQuoteColor) 25%, transparent)' },
    '.cm-searchMatch-selected': {
        backgroundColor: 'color-mix(in srgb, var(--SmartThemeQuoteColor) 50%, transparent)',
        outline: '1px solid var(--SmartThemeQuoteColor)',
    },
    '.cm-resize-handle': {
        position: 'absolute',
        right: '0',
        bottom: '0',
        zIndex: '1',
        width: '14px',
        height: '14px',
        touchAction: 'none',
        opacity: '0.45',
        background: 'linear-gradient(135deg, transparent 52%, currentColor 52% 60%, transparent 60% 74%, currentColor 74% 82%, transparent 82%)',
    },
});

/**
 * CodeMirror covers WebKit's native resize grip; dragging must also disable flex sizing.
 * @param {HTMLElement} host
 * @param {string} resize The source's computed `resize`.
 */
function resizeHandle(host, resize) {
    const vertical = ['both', 'vertical', 'block'].includes(resize);
    const horizontal = ['both', 'horizontal', 'inline'].includes(resize);
    if (!vertical && !horizontal) return [];
    const cursor = vertical && horizontal ? 'nwse-resize' : vertical ? 'ns-resize' : 'ew-resize';
    return ViewPlugin.define(view => {
        const handle = document.createElement('div');
        handle.className = 'cm-resize-handle';
        handle.style.cursor = cursor;
        handle.addEventListener('pointerdown', start => {
            if (start.button !== 0) return;
            start.preventDefault();
            handle.setPointerCapture(start.pointerId);
            // Convert viewport deltas to the host's unscaled dimensions.
            const scale = host.getBoundingClientRect().height / host.offsetHeight;
            const width = host.offsetWidth, height = host.offsetHeight;
            let pointer = start;
            let frame = 0;
            let sized = false;
            const apply = () => {
                frame = 0;
                if (!sized) host.style.flex = 'none';
                sized = true;
                if (vertical) host.style.height = `${height + (pointer.clientY - start.clientY) / scale}px`;
                if (horizontal) host.style.width = `${width + (pointer.clientX - start.clientX) / scale}px`;
            };
            const move = event => {
                pointer = event;
                frame ||= requestAnimationFrame(apply);
            };
            handle.addEventListener('pointermove', move);
            handle.addEventListener('lostpointercapture', () => {
                handle.removeEventListener('pointermove', move);
                if (frame) {
                    cancelAnimationFrame(frame);
                    apply();
                }
            }, { once: true });
        });
        view.dom.append(handle);
        return { destroy: () => handle.remove() };
    });
}

/**
 * Builds the editor on `host`. An isolated editor lives in a shadow root, so page selectors never match or
 * re-check its DOM. Raw text events end at the host; edits leave through `onChange`, and an open search
 * panel is reflected as `data-search-open` on the host.
 * Formatting is isolated from adjacent edits in undo history.
 */
export function createCodeMirrorView(host, { isolated, doc, readOnly, ariaLabel, describedBy = null, onChange, onCopy, phrases = {}, placeholder: hint = '', selection = undefined, resize = 'none', format = null }) {
    const parent = isolated ? host.attachShadow({ mode: 'open' }) : host;
    for (const type of ['beforeinput', 'input']) parent.addEventListener(type, event => event.stopPropagation());
    const permissions = new Compartment();
    const editability = disabled => [
        EditorState.readOnly.of(disabled),
        EditorView.editable.of(!disabled),
        EditorView.contentAttributes.of({ 'aria-readonly': String(disabled), tabindex: '0' }),
    ];
    const naming = new Compartment();
    let name = ariaLabel;
    const nameAttribute = () => EditorView.contentAttributes.of({ 'aria-label': name });

    function toolbar(view) {
        const dom = document.createElement('div');
        dom.className = 'cm-editor-tools';
        dom.setAttribute('role', 'group');
        dom.setAttribute('aria-label', view.state.phrase('Editor tools'));

        function button(label, icon, action) {
            const button = document.createElement('button');
            button.type = 'button';
            button.title = view.state.phrase(label);
            button.setAttribute('aria-label', button.title);
            button.append(editorIcon(icon));
            button.addEventListener('mousedown', event => event.preventDefault());
            button.addEventListener('click', () => {
                view.focus();
                action();
            });
            dom.append(button);
            return button;
        }

        const formatButton = format && button(format.label, 'M5.5 2.5c-1.5 0-2 .5-2 2v2c0 1-.5 1.5-1.5 1.5 1 0 1.5.5 1.5 1.5v2c0 1.5.5 2 2 2M10.5 2.5c1.5 0 2 .5 2 2v2c0 1 .5 1.5 1.5 1.5-1 0-1.5.5-1.5 1.5v2c0 1.5-.5 2-2 2', () => {
            const text = view.state.doc.toString();
            const formatted = format.run(text);
            if (formatted === text) return;
            view.dispatch({ changes: { from: 0, to: text.length, insert: formatted }, annotations: isolateHistory.of('full') });
        });
        const undoButton = button('Undo', 'M3.5 6.5h6a3.5 3.5 0 0 1 0 7H6M6 3.5l-3 3 3 3', () => undo(view));
        const redoButton = button('Redo', 'M12.5 6.5h-6a3.5 3.5 0 0 0 0 7H10M10 3.5l3 3-3 3', () => redo(view));
        const searchButton = button('Find and replace', 'M7 2.5a4.5 4.5 0 1 0 0 9 4.5 4.5 0 1 0 0-9ZM10.2 10.2l3.3 3.3', () => {
            searchPanelOpen(view.state) ? closeSearchPanel(view) : openSearchPanel(view);
        });
        button('Copy all', 'M5.5 5.5h8v8h-8ZM2.5 10.5v-8h8', () => onCopy(view.state.doc.toString()));
        const update = () => {
            const searching = searchPanelOpen(view.state);
            if (formatButton) formatButton.disabled = view.state.readOnly;
            undoButton.disabled = view.state.readOnly || undoDepth(view.state) === 0;
            redoButton.disabled = view.state.readOnly || redoDepth(view.state) === 0;
            searchButton.setAttribute('aria-expanded', String(searching));
            host.toggleAttribute('data-search-open', searching);
        };
        update();
        return { dom, top: true, update };
    }

    const createState = (value, disabled, selection = undefined) => EditorState.create({
        doc: value,
        selection,
        extensions: [
            // No drawSelection: the native caret keeps each keystroke to one frame.
            highlightSpecialChars(),
            history(),
            keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap]),
            showPanel.of(toolbar),
            search({ top: true, createPanel: createSearchPanel }),
            EditorState.phrases.of(phrases),
            permissions.of(editability(disabled)),
            naming.of(nameAttribute()),
            EditorView.lineWrapping,
            placeholder(hint),
            EditorView.updateListener.of(update => update.docChanged && onChange?.()),
            resizeHandle(host, resize),
            theme,
        ],
    });

    const view = new EditorView({ state: createState(doc, readOnly, selection), parent });
    // Element references, unlike IDs, reach out of a shadow root, and read the elements' current text.
    if (describedBy) view.contentDOM.ariaDescribedByElements = describedBy;

    return {
        getValue: () => view.state.doc.toString(),
        getSelection: () => view.state.selection.main,
        reset(value, disabled = false) {
            view.setState(createState(value, disabled));
        },
        setReadOnly(disabled) {
            if (view.state.readOnly !== disabled) view.dispatch({ effects: permissions.reconfigure(editability(disabled)) });
        },
        setAriaLabel(label) {
            if (name === label) return;
            name = label;
            view.dispatch({ effects: naming.reconfigure(nameAttribute()) });
        },
        focus: () => view.focus(),
        requestMeasure: () => view.requestMeasure(),
        destroy: () => view.destroy(),
    };
}
