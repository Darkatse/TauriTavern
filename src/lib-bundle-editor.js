import { EditorState } from '@codemirror/state';
import { undo, redo, undoDepth, redoDepth } from '@codemirror/commands';
import { keymap, showPanel } from '@codemirror/view';
import { openSearchPanel, closeSearchPanel, searchPanelOpen, searchKeymap, search } from '@codemirror/search';
import { EditorView, minimalSetup } from 'codemirror';
import { createSearchPanel } from './editor-search-panel.js';

const theme = EditorView.theme({
    '&': {
        height: '100%',
        color: 'var(--SmartThemeBodyColor)',
        font: 'inherit',
    },
    '&.cm-focused': {
        outline: '1px solid var(--SmartThemeQuoteColor)',
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
    '.cm-panels-top': {
        borderBottom: '1px solid color-mix(in srgb, currentColor 12%, transparent)',
    },
    '.cm-editor-tools': {
        display: 'flex',
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
    '.cm-editor-search button svg': {
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
});

export function createCodeMirrorView(parent, { doc, readOnly, ariaLabel, onChange, onCopy, phrases = {} }) {
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
            const glyph = document.createElement('i');
            glyph.className = `fa-solid ${icon}`;
            glyph.setAttribute('aria-hidden', 'true');
            button.append(glyph);
            button.addEventListener('mousedown', event => event.preventDefault());
            button.addEventListener('click', () => {
                view.focus();
                action();
            });
            dom.append(button);
            return button;
        }

        const undoButton = button('Undo', 'fa-rotate-left', () => undo(view));
        const redoButton = button('Redo', 'fa-rotate-right', () => redo(view));
        const searchButton = button('Find and replace', 'fa-magnifying-glass', () => {
            searchPanelOpen(view.state) ? closeSearchPanel(view) : openSearchPanel(view);
        });
        button('Copy all', 'fa-copy', () => onCopy(view.state.doc.toString()));
        const update = () => {
            undoButton.disabled = view.state.readOnly || undoDepth(view.state) === 0;
            redoButton.disabled = view.state.readOnly || redoDepth(view.state) === 0;
            searchButton.setAttribute('aria-expanded', String(searchPanelOpen(view.state)));
        };
        update();
        return { dom, top: true, update };
    }

    const createState = (value, disabled) => EditorState.create({
        doc: value,
        extensions: [
            minimalSetup,
            keymap.of(searchKeymap),
            showPanel.of(toolbar),
            search({ top: true, createPanel: createSearchPanel }),
            EditorState.phrases.of(phrases),
            EditorState.readOnly.of(disabled),
            EditorView.lineWrapping,
            EditorView.editable.of(!disabled),
            EditorView.contentAttributes.of({ 'aria-label': ariaLabel }),
            EditorView.updateListener.of(update => update.docChanged && onChange?.()),
            theme,
        ],
    });

    const view = new EditorView({ state: createState(doc, readOnly), parent });

    return {
        getValue: () => view.state.doc.toString(),
        reset(value, disabled = false) {
            view.setState(createState(value, disabled));
        },
        focus: () => view.focus(),
        requestMeasure: () => view.requestMeasure(),
        destroy: () => view.destroy(),
    };
}
