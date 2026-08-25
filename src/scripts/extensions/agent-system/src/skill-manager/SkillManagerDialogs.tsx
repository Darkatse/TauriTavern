import { useLayoutEffect, useRef } from 'react';

import type { SkillManagerController, SkillManagerSnapshot, SkillManagerTr } from './SkillManagerContract';
import { buildSkillFileTree, SkillFileTree, SkillFileViewer } from './SkillManagerFiles';

function useModalDialog(options: {
    open: boolean;
    unsupportedMessage: string;
    onShowFailed: (error: unknown) => void;
}) {
    const { open, unsupportedMessage, onShowFailed } = options;
    const ref = useRef<HTMLDialogElement>(null);
    useLayoutEffect(() => {
        const dialog = ref.current;
        if (!dialog) return;
        if (!open) {
            if (dialog.open) dialog.close();
            return;
        }
        if (typeof dialog.showModal !== 'function') {
            onShowFailed(new Error(unsupportedMessage));
            return;
        }
        if (!dialog.open) {
            try {
                dialog.showModal();
            } catch (error) {
                onShowFailed(error);
            }
        }
    }, [open, onShowFailed, unsupportedMessage]);
    return ref;
}

function importSourceLabel(kind: string, tr: SkillManagerTr): string {
    if (kind === 'manual') return tr('skillImportSourceManual');
    if (kind === 'download') return tr('skillImportSourceDownload');
    if (kind === 'directory') return tr('skillImportSourceDirectory');
    return tr('skillImportSourceArchive');
}

export function SkillScopeDialog(props: {
    snapshot: SkillManagerSnapshot;
    controller: SkillManagerController;
    tr: SkillManagerTr;
}) {
    const { snapshot, controller, tr } = props;
    const state = snapshot.scopeDialog;
    const ref = useModalDialog({
        open: state.mode !== '',
        unsupportedMessage: tr('skillScopeDialogUnsupported'),
        onShowFailed: error => controller.dialogShowFailed('scope', error),
    });
    const targets = state.mode === 'move'
        ? snapshot.sections.filter(section => section.available && section.id !== state.sourceSectionId)
        : snapshot.sections.filter(section => section.available);
    const title = state.mode === 'move' ? tr('selectMoveScope') : tr('selectImportScope');

    return (
        <dialog
            ref={ref}
            className="ttas-scope-dialog"
            data-tt-mobile-surface="fullscreen-window"
            onCancel={(event) => { event.preventDefault(); controller.closeScopeDialog(); }}
            onClose={controller.closeScopeDialog}
        >
            {state.mode && (
                <div className="ttas-root ttas-scope-picker">
                    <header className="ttas-scope-picker-head">
                        <div>
                            <strong>{title}</strong>
                            <small>{state.mode === 'move' ? state.skill.displayName || state.skill.name : importSourceLabel(state.importKind, tr)}</small>
                        </div>
                        <button type="button" className="menu_button menu_button_icon ttas-close-button" title={tr('close')} aria-label={tr('close')} onClick={controller.closeScopeDialog}>
                            <i className="fa-solid fa-xmark"></i>
                        </button>
                    </header>
                    {state.mode === 'import' && snapshot.supportsDirectoryImport && (state.importKind === 'archive' || state.importKind === 'directory') && (
                        <div className="ttas-skill-import-kind" role="radiogroup" aria-label={tr('importSkill')}>
                            <label aria-label={tr('skillImportSourceArchive')} className={`ttas-scope-option${state.importKind === 'archive' ? ' active' : ''}`}>
                                <input type="radio" value="archive" checked={state.importKind === 'archive'} onChange={() => controller.setScopeImportKind('archive')} />
                                <i className="fa-solid fa-file-zipper"></i>
                                <span><strong>{tr('skillImportSourceArchive')}</strong></span>
                            </label>
                            <label aria-label={tr('skillImportSourceDirectory')} className={`ttas-scope-option${state.importKind === 'directory' ? ' active' : ''}`}>
                                <input type="radio" value="directory" checked={state.importKind === 'directory'} onChange={() => controller.setScopeImportKind('directory')} />
                                <i className="fa-solid fa-folder-open"></i>
                                <span><strong>{tr('skillImportSourceDirectory')}</strong></span>
                            </label>
                            <small
                                className={`ttas-skill-import-directory-hint${state.importKind === 'directory' ? ' visible' : ''}`}
                                aria-hidden={state.importKind !== 'directory'}
                            >
                                {tr('skillImportDirectoryHint')}
                            </small>
                        </div>
                    )}
                    <div className="ttas-scope-list">
                        {targets.map(target => (
                            <label key={target.id} aria-label={tr(target.labelKey)} className={`ttas-scope-option${state.selectedSectionId === target.id ? ' active' : ''}`}>
                                <input
                                    type="radio"
                                    value={target.id}
                                    checked={state.selectedSectionId === target.id}
                                    onChange={() => controller.setScopeDialogTarget(target.id)}
                                />
                                <i className={`fa-solid ${target.icon}`}></i>
                                <span><strong>{tr(target.labelKey)}</strong><small>{target.subtitle}</small></span>
                            </label>
                        ))}
                    </div>
                    <footer className="ttas-scope-picker-actions">
                        <button type="button" className="menu_button menu_button_icon" onClick={controller.closeScopeDialog}>
                            <i className="fa-solid fa-xmark"></i><span>{tr('cancel')}</span>
                        </button>
                        <button type="button" className="menu_button menu_button_icon ttas-primary-button" onClick={controller.confirmScopeDialog}>
                            <i className="fa-solid fa-check"></i><span>{state.mode === 'move' ? tr('moveToScope') : tr('continue')}</span>
                        </button>
                    </footer>
                </div>
            )}
        </dialog>
    );
}

export function SkillSourceDialog(props: {
    snapshot: SkillManagerSnapshot;
    controller: SkillManagerController;
    tr: SkillManagerTr;
}) {
    const { snapshot, controller, tr } = props;
    const state = snapshot.sourceDialog;
    const ref = useModalDialog({
        open: state.mode !== '',
        unsupportedMessage: tr('skillImportSourceDialogUnsupported'),
        onShowFailed: error => controller.dialogShowFailed('source', error),
    });
    const section = state.mode ? snapshot.sections.find(item => item.id === state.sectionId) : null;
    const download = state.mode === 'download';

    return (
        <dialog
            ref={ref}
            className="ttas-scope-dialog ttas-skill-source-dialog"
            data-tt-mobile-surface="fullscreen-window"
            onCancel={(event) => { event.preventDefault(); controller.closeSourceDialog(); }}
            onClose={controller.closeSourceDialog}
        >
            {state.mode && (
                <div className="ttas-root ttas-scope-picker">
                    <header className="ttas-scope-picker-head">
                        <div>
                            <strong>{tr(download ? 'downloadSkillImport' : 'newSkillImport')}</strong>
                            {section && <small>{tr('importTargetScope')}: {tr(section.labelKey)}</small>}
                        </div>
                        <button type="button" className="menu_button menu_button_icon ttas-close-button" disabled={state.loading} title={tr('close')} aria-label={tr('close')} onClick={controller.closeSourceDialog}>
                            <i className="fa-solid fa-xmark"></i>
                        </button>
                    </header>
                    <div className="ttas-skill-source-body">
                        <label className="ttas-field ttas-skill-source-field">
                            <span>{tr(download ? 'skillDownloadUrl' : 'skillMdContent')}</span>
                            {download ? (
                                <input
                                    className="text_pole"
                                    type="url"
                                    disabled={state.loading}
                                    placeholder={tr('skillDownloadUrlPlaceholder')}
                                    value={state.url}
                                    onChange={event => controller.setSourceUrl(event.target.value)}
                                />
                            ) : (
                                <textarea
                                    className="text_pole textarea_compact ttas-skill-source-textarea"
                                    rows={16}
                                    spellCheck={false}
                                    disabled={state.loading}
                                    placeholder={tr('skillMdContentPlaceholder')}
                                    value={state.content}
                                    onChange={event => controller.setSourceContent(event.target.value)}
                                />
                            )}
                        </label>
                    </div>
                    <footer className="ttas-scope-picker-actions">
                        <button type="button" className="menu_button menu_button_icon" disabled={state.loading} onClick={controller.closeSourceDialog}>
                            <i className="fa-solid fa-xmark"></i><span>{tr('cancel')}</span>
                        </button>
                        <button type="button" className="menu_button menu_button_icon ttas-primary-button" disabled={state.loading} onClick={controller.confirmSourceDialog}>
                            <i className={`fa-solid ${state.loading ? 'fa-spinner fa-spin' : 'fa-check'}`}></i>
                            <span>{tr(download ? 'download' : 'confirm')}</span>
                        </button>
                    </footer>
                </div>
            )}
        </dialog>
    );
}

export function SkillPreviewDialog(props: {
    snapshot: SkillManagerSnapshot;
    controller: SkillManagerController;
    tr: SkillManagerTr;
}) {
    const { snapshot, controller, tr } = props;
    const preview = snapshot.preview;
    const ref = useModalDialog({
        open: preview !== null,
        unsupportedMessage: tr('skillPreviewDialogUnsupported'),
        onShowFailed: error => controller.dialogShowFailed('preview', error),
    });
    const tree = preview ? buildSkillFileTree(preview.files, tr) : [];
    const folderKey = (path: string) => preview ? `${preview.scopeLabel}:${preview.skill.name}:${path}` : '';

    return (
        <dialog
            ref={ref}
            className="ttas-file-dialog ttas-skill-preview-dialog"
            data-tt-mobile-surface="fullscreen-window"
            onCancel={(event) => { event.preventDefault(); controller.previewCancelled(); }}
            onClose={controller.previewClosed}
        >
            {preview && (
                <div className="ttas-root ttas-file-viewer ttas-skill-preview-viewer">
                    <header className="ttas-titlebar ttas-file-viewer-titlebar">
                        <div><div className="ttas-eyebrow">{preview.scopeLabel}</div><h3>{preview.skill.displayName || preview.skill.name}</h3></div>
                        <button type="button" className="menu_button menu_button_icon ttas-close-button" title={tr('close')} aria-label={tr('close')} onClick={controller.closePreview}>
                            <i className="fa-solid fa-xmark"></i>
                        </button>
                    </header>
                    <div className={`ttas-file-viewport${preview.loading ? ' loading' : ''}`}>
                        {preview.loading ? (
                            <div className="ttas-file-loading" role="status">
                                <span>{tr('loadingSkillFiles')}</span><div className="ttas-file-loading-lines"><i></i><i></i><i></i></div>
                            </div>
                        ) : tree.length === 0 ? (
                            <div className="ttas-empty ttas-file-empty">{tr('noFilesFoundForSkill')}</div>
                        ) : (
                            <SkillFileTree
                                nodes={tree}
                                isFolderOpen={path => preview.expandedFolders[folderKey(path)] === true}
                                onToggleFolder={controller.togglePreviewFolder}
                                onOpenFile={controller.openPreviewFile}
                                tr={tr}
                            />
                        )}
                    </div>
                    {snapshot.fileViewer && (
                        <div
                            className="ttas-file-overlay"
                            role="presentation"
                            onMouseDown={(event) => { if (event.target === event.currentTarget) controller.closeFileViewer(); }}
                        >
                            <div className="ttas-file-overlay-panel">
                                <SkillFileViewer
                                    key={snapshot.fileViewer.id}
                                    file={snapshot.fileViewer.file}
                                    onSave={controller.saveOpenFile}
                                    onClose={controller.closeFileViewer}
                                    tr={tr}
                                />
                            </div>
                        </div>
                    )}
                </div>
            )}
        </dialog>
    );
}
