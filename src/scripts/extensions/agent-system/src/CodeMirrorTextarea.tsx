import { useEffect, useEffectEvent, useLayoutEffect, useRef } from 'react';

import { loadCodeMirrorEditor, reportAgentSystemError } from './host-api';
import type { CodeMirrorEditorHandle } from '../../../tauri/codemirror-editor.js';

type Props = {
    value: string;
    onChange?: (value: string) => void;
    label: string;
    className: string;
    readOnly?: boolean;
    disabled?: boolean;
    placeholder?: string;
    rows?: number;
};

export function CodeMirrorTextarea({ value, onChange, label, className, readOnly = false, disabled = false, placeholder, rows }: Props) {
    const source = useRef<HTMLTextAreaElement>(null);
    const editor = useRef<CodeMirrorEditorHandle | null>(null);
    const synced = useRef(value);
    const onEdit = useEffectEvent(() => {
        if (!editor.current?.flush() || !source.current) return;
        synced.current = source.current.value;
        onChange?.(source.current.value);
    });

    useEffect(() => {
        const textarea = source.current;
        if (!textarea) return;
        const controller = new AbortController();
        void loadCodeMirrorEditor()
            .then(runtime => runtime.mountCodeMirrorEditor(textarea, { signal: controller.signal, onChange: () => onEdit() }))
            .then(mounted => {
                if (controller.signal.aborted) {
                    mounted?.destroy();
                    return;
                }
                editor.current = mounted;
                synced.current = textarea.value;
            })
            .catch(error => { if (!controller.signal.aborted) reportAgentSystemError(error); });
        return () => {
            controller.abort();
            editor.current?.destroy();
            editor.current = null;
        };
    }, []);

    useLayoutEffect(() => {
        if (!editor.current || synced.current === value) return;
        editor.current.reset();
        synced.current = value;
    }, [value]);

    useLayoutEffect(() => {
        editor.current?.updateReadOnly();
    }, [readOnly, disabled]);

    return (
        <div className="ttas-codemirror-field">
            <textarea ref={source} className={className} aria-label={label} spellCheck={false}
                value={value} readOnly={readOnly} disabled={disabled} placeholder={placeholder} rows={rows}
                onChange={event => onChange?.(event.target.value)} />
        </div>
    );
}
