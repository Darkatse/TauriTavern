import { EditorState } from '@codemirror/state';
import { EditorView, minimalSetup } from 'codemirror';

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
});

export function createCodeMirrorView(parent, { doc, readOnly, ariaLabel, onChange }) {
    const createState = (value, disabled) => EditorState.create({
        doc: value,
        extensions: [
            minimalSetup,
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
