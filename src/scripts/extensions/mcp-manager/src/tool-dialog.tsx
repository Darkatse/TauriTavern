import {
    useImperativeHandle,
    useState,
    type ReactNode,
    type Ref,
} from 'react';
import { createRoot } from 'react-dom/client';

import {
    createConfirmPopup,
    errorText,
    isPopupAffirmative,
    tr,
    type McpTranslator,
} from './host';
import type { ToolDescriptionDialogInput } from './McpManagerApp';

// Popup assigns string custom buttons result values starting at 2.
const RESET_RESULT = 2;

type ToolDescriptionDialogHandle = {
    /** Commits the draft; resolves true only when the dialog may close. */
    submit: (reset: boolean) => Promise<boolean>;
};

/**
 * Folds an edited description into the saved override. Sibling property
 * overrides survive a description edit; an emptied description drops the whole
 * entry once nothing else remains.
 */
export function withDescription(
    override: TauriTavernToolDescriptionOverride | undefined,
    description: string,
): TauriTavernToolDescriptionOverride | null {
    const next: TauriTavernToolDescriptionOverride = { ...override };
    if (description.trim()) {
        next.description = description;
    } else {
        delete next.description;
    }
    return next.description !== undefined || Object.keys(next.properties ?? {}).length > 0
        ? next
        : null;
}

type ToolDescriptionDialogProps = ToolDescriptionDialogInput & {
    tr: McpTranslator;
    ref: Ref<ToolDescriptionDialogHandle>;
};

export function ToolDescriptionDialog({
    tool,
    override,
    save,
    tr,
    ref,
}: ToolDescriptionDialogProps): ReactNode {
    const [draft, setDraft] = useState(override?.description ?? '');
    const [error, setError] = useState('');
    const [saving, setSaving] = useState(false);

    useImperativeHandle(ref, () => ({
        submit: async reset => {
            setSaving(true);
            setError('');
            try {
                await save(reset ? null : withDescription(override, draft));
                return true;
            } catch (cause) {
                setError(errorText(cause, tr('unknownError')));
                return false;
            } finally {
                setSaving(false);
            }
        },
    }));

    const hasNativeAlias = tool.title !== undefined && tool.title !== tool.nativeName;

    return (
        <div className="tt-mcp-tool-dialog">
            <h3>
                {tool.title ?? tool.nativeName}
                {hasNativeAlias && <code>{tool.nativeName}</code>}
            </h3>

            {tool.description && (
                <div className="tt-mcp-field">
                    <label>{tr('serverDescription')}</label>
                    <p className="tt-mcp-tool-origin">{tool.description}</p>
                </div>
            )}

            <div className="tt-mcp-field">
                <label htmlFor="tt-mcp-tool-custom-description">{tr('customDescription')}</label>
                <textarea
                    id="tt-mcp-tool-custom-description"
                    className="text_pole"
                    rows={5}
                    value={draft}
                    disabled={saving}
                    onChange={event => setDraft(event.currentTarget.value)}
                />
                <small>{tr('customDescriptionHint')}</small>
            </div>

            {error && <p className="tt-mcp-error" role="alert">{error}</p>}
        </div>
    );
}

/**
 * Opens the per-tool description editor in a vanilla confirm popup. The dialog
 * owns its busy and error states; the popup closes only after a successful save.
 */
export async function openToolDialog({
    tool,
    override,
    save,
}: ToolDescriptionDialogInput): Promise<void> {
    const mount = document.createElement('div');
    let dialog: ToolDescriptionDialogHandle | null = null;

    const root = createRoot(mount);
    try {
        root.render(
            <ToolDescriptionDialog
                tool={tool}
                override={override}
                save={save}
                tr={tr}
                ref={handle => {
                    dialog = handle;
                }}
            />,
        );

        const popup = createConfirmPopup(mount, {
            okButton: tr('save'),
            cancelButton: tr('cancel'),
            ...(override ? { customButtons: [tr('resetCustomization')] } : {}),
            allowVerticalScrolling: true,
            onOpen: () => mount.querySelector<HTMLTextAreaElement>('#tt-mcp-tool-custom-description')?.focus(),
            onClosing: async popup => {
                const reset = popup.result === RESET_RESULT;
                if (!reset && !isPopupAffirmative(popup.result)) {
                    return true;
                }
                return (await dialog?.submit(reset)) === true;
            },
        });
        await popup.show();
    } finally {
        root.unmount();
    }
}
