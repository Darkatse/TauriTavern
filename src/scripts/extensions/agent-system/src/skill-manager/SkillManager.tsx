import { useSyncExternalStore } from 'react';

import type {
    SkillImportItem,
    SkillManagerController,
    SkillManagerTr,
    SkillSection,
} from './SkillManagerContract';
import { SkillPreviewDialog, SkillScopeDialog, SkillSourceDialog } from './SkillManagerDialogs';
import { skillImportItemLabel, skillImportSourceField } from './SkillImportOperation';

function itemLabel(item: SkillImportItem, tr: SkillManagerTr): string {
    return skillImportItemLabel(item, tr);
}

function loadingTitle(item: SkillImportItem, tr: SkillManagerTr): string {
    const sourceKind = skillImportSourceField(item.input, 'kind');
    if (sourceKind === 'manual') return tr('newSkillImport');
    if (sourceKind === 'url') return tr('downloadSkillImport');
    return tr(item.input.kind === 'directory' ? 'importSkillDirectories' : 'importSkillArchive');
}

function importIcon(item: SkillImportItem): string {
    const sourceKind = skillImportSourceField(item.input, 'kind');
    if (sourceKind === 'manual') return 'fa-file-circle-plus';
    if (sourceKind === 'url') return 'fa-cloud-arrow-down';
    return item.input.kind === 'directory' ? 'fa-folder-open' : 'fa-file-import';
}

function conflictText(item: SkillImportItem, tr: SkillManagerTr): string {
    const kind = item.preview?.conflict.kind;
    if (kind === 'new') return tr('conflictNew');
    if (kind === 'same') return tr('conflictSame');
    if (kind === 'different') return tr('conflictDifferent');
    return '';
}

function matchesSearch(skill: TauriTavernSkillIndexEntry, query: string): boolean {
    return [skill.displayName, skill.name, skill.description, skill.version, skill.sourceKind]
        .some(value => String(value || '').toLowerCase().includes(query));
}

function ImportConflictSelect(props: {
    item: SkillImportItem;
    index: number;
    busy: boolean;
    controller: SkillManagerController;
    tr: SkillManagerTr;
}) {
    const { item, index, busy, controller, tr } = props;
    if (item.preview?.conflict.kind !== 'different') return null;
    return (
        <select
            value={item.conflictStrategy}
            disabled={busy}
            aria-label={tr('importConflictAction', { name: itemLabel(item, tr) })}
            onChange={event => controller.setImportConflict(index, event.target.value as TauriTavernSkillInstallConflictStrategy)}
        >
            <option value="skip">{tr('skipConflict')}</option>
            <option value="replace">{tr('replaceConflict')}</option>
        </select>
    );
}

function ImportDraft(props: {
    controller: SkillManagerController;
    tr: SkillManagerTr;
    section: SkillSection | null;
    items: readonly SkillImportItem[];
    installing: boolean;
}) {
    const { controller, tr, section, items, installing } = props;
    if (items.length === 0) return null;
    const busy = installing || items.some(item => !item.preview && !item.error);
    if (items.length === 1) {
        const item = items[0];
        if (!item) return null;
        return (
            <div className="ttas-skill-import-inline ttas-skill-import-global">
                <div className="ttas-skill-import-inline-main">
                    <i className={`fa-solid ${busy ? 'fa-spinner fa-spin' : importIcon(item)}`}></i>
                    <div>
                        <strong>{item.preview ? itemLabel(item, tr) : loadingTitle(item, tr)}</strong>
                        {section && <small>{tr('importTargetScope')}: {tr(section.labelKey)} / {conflictText(item, tr) || tr('loadingSkillFiles')}</small>}
                    </div>
                </div>
                <ImportConflictSelect item={item} index={0} busy={busy} controller={controller} tr={tr} />
                {item.preview && (
                    <button type="button" className="menu_button menu_button_icon ttas-primary-button" disabled={busy} onClick={() => void controller.installImports()}>
                        <i className={`fa-solid ${installing ? 'fa-spinner fa-spin' : 'fa-check'}`}></i><span>{tr('install')}</span>
                    </button>
                )}
                <button type="button" className="menu_button menu_button_icon" disabled={busy} onClick={controller.clearImportDraft}>
                    <i className="fa-solid fa-xmark"></i><span>{tr('cancel')}</span>
                </button>
                {item.preview && item.preview.warnings.length > 0 && (
                    <ul className="ttas-skill-import-warnings">{item.preview.warnings.map(warning => <li key={warning}>{warning}</li>)}</ul>
                )}
            </div>
        );
    }
    const importable = items.some(item => item.preview && !item.error);
    return (
        <section className="ttas-skill-import-batch ttas-skill-import-global" aria-label={tr('skillImportBatchTitle', { count: items.length })}>
            <header className="ttas-skill-import-batch-head">
                <div><i className={`fa-solid ${busy ? 'fa-spinner fa-spin' : 'fa-layer-group'}`}></i><strong>{tr('skillImportBatchTitle', { count: items.length })}</strong></div>
                {section && <small>{tr('importTargetScope')}: {tr(section.labelKey)}</small>}
            </header>
            <ol className="ttas-skill-import-batch-list">
                {items.map((item, index) => (
                    <li key={'path' in item.input ? item.input.path : `${index}`} className={`ttas-skill-import-batch-item${item.error ? ' has-error' : ''}`}>
                        <i className={`fa-solid ttas-skill-import-batch-status ${item.error ? 'fa-triangle-exclamation' : item.preview ? 'fa-circle-check' : 'fa-spinner fa-spin'}`}></i>
                        <div className="ttas-skill-import-batch-copy">
                            <strong>{itemLabel(item, tr)}</strong>
                            {item.error ? <small className="ttas-skill-import-error">{item.error}</small> : <small>{conflictText(item, tr) || tr('loadingSkillFiles')}</small>}
                            {item.preview && item.preview.warnings.length > 0 && (
                                <ul className="ttas-skill-import-warnings">{item.preview.warnings.map(warning => <li key={warning}>{warning}</li>)}</ul>
                            )}
                        </div>
                        <ImportConflictSelect item={item} index={index} busy={busy} controller={controller} tr={tr} />
                    </li>
                ))}
            </ol>
            <footer className="ttas-skill-import-batch-actions">
                <button type="button" className="menu_button menu_button_icon ttas-primary-button" disabled={busy || !importable} onClick={() => void controller.installImports()}>
                    <i className={`fa-solid ${installing ? 'fa-spinner fa-spin' : 'fa-check-double'}`}></i><span>{tr('install')}</span>
                </button>
                <button type="button" className="menu_button menu_button_icon" disabled={busy} onClick={controller.clearImportDraft}>
                    <i className="fa-solid fa-xmark"></i><span>{tr('cancel')}</span>
                </button>
            </footer>
        </section>
    );
}

function SkillSectionView(props: {
    section: SkillSection;
    query: string;
    controller: SkillManagerController;
    tr: SkillManagerTr;
}) {
    const { section, query, controller, tr } = props;
    const visible = query ? section.skills.filter(skill => matchesSearch(skill, query)) : section.skills;
    return (
        <section className={`ttas-skill-section${section.available ? '' : ' is-unavailable'}`}>
            <header className="ttas-skill-section-head">
                <div className="ttas-skill-section-title">
                    <i className={`fa-solid ${section.icon}`}></i>
                    <div><h4>{tr(section.labelKey)}</h4><small>{section.subtitle}</small></div>
                </div>
                <span className="ttas-skill-count-pill">{tr('skillCount', { count: section.skills.length })}</span>
            </header>
            {!section.available ? (
                <div className="ttas-skill-empty"><i className="fa-solid fa-circle-info"></i><span>{tr(section.unavailableKey || 'skillScopeUnavailable')}</span></div>
            ) : (
                <div className="ttas-skill-section-body">
                    {section.loading ? (
                        <div className="ttas-file-loading ttas-skill-section-loading" role="status">
                            <span>{tr('loadingSkillFiles')}</span><div className="ttas-file-loading-lines"><i></i><i></i><i></i></div>
                        </div>
                    ) : section.skills.length === 0 ? (
                        <div className="ttas-skill-empty"><i className="fa-solid fa-inbox"></i><span>{tr('noSkillsInstalled')}</span></div>
                    ) : visible.length === 0 ? (
                        <div className="ttas-skill-empty"><i className="fa-solid fa-magnifying-glass"></i><span>{tr('noSkillsMatch')}</span></div>
                    ) : (
                        <ul className="ttas-skill-list">
                            {visible.map(skill => (
                                <li key={`${section.id}:${skill.name}:${skill.installedHash}`} className="ttas-skill-list-item">
                                    <article className="ttas-skill-row">
                                        <div className="ttas-skill-row-main">
                                            <i className="fa-solid fa-book-open"></i>
                                            <span className="ttas-skill-row-copy">
                                                <strong>{skill.displayName || skill.name}</strong>
                                                <span>{skill.description.trim() || (skill.displayName !== skill.name ? skill.name : tr('defaultDescription'))}</span>
                                            </span>
                                        </div>
                                        <div className="ttas-skill-row-actions">
                                            <button type="button" className="menu_button menu_button_icon" title={tr('viewSkill')} aria-label={tr('viewSkill')} onClick={() => controller.openSkillPreview(section.id, skill)}><i className="fa-solid fa-eye"></i></button>
                                            <button type="button" className="menu_button menu_button_icon" title={tr('moveToScope')} aria-label={tr('moveToScope')} onClick={() => controller.openMoveScopeDialog(section.id, skill)}><i className="fa-solid fa-arrow-right-arrow-left"></i></button>
                                            <button type="button" className="menu_button menu_button_icon" title={tr('export')} aria-label={tr('export')} onClick={() => controller.exportSkill(section.id, skill)}><i className="fa-solid fa-file-export"></i></button>
                                            <button type="button" className="menu_button menu_button_icon ttas-danger-button" title={tr('delete')} aria-label={tr('delete')} onClick={() => controller.deleteSkill(section.id, skill)}><i className="fa-solid fa-trash-can"></i></button>
                                        </div>
                                    </article>
                                </li>
                            ))}
                        </ul>
                    )}
                </div>
            )}
        </section>
    );
}

export function SkillManager(props: { controller: SkillManagerController; tr: SkillManagerTr }) {
    const { controller, tr } = props;
    const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
    const busy = snapshot.importDraft.installing || snapshot.importDraft.items.some(item => !item.preview && !item.error);
    const importSection = snapshot.sections.find(section => section.id === snapshot.importDraft.sectionId) ?? null;
    const query = snapshot.searchQuery.trim().toLowerCase();

    return (
        <div className="ttas-root ttas-panel-root ttas-skill-manager-root ttas-skill-manager-inline">
            {snapshot.loading && !snapshot.initialized ? <div className="ttas-loading">{tr('loadingSkillExtension')}</div> : (
                <div className="ttas-panel-body ttas-skill-manager-body">
                    {snapshot.error && <div className="ttas-error" role="alert"><i className="fa-solid fa-triangle-exclamation"></i><pre>{snapshot.error}</pre></div>}
                    <div className="ttas-skill-manager-tools">
                        <div className="ttas-skill-toolbar-row">
                            <label className="ttas-field">
                                <span>{tr('selectProfile')}</span>
                                <select value={snapshot.selectedProfileId} onChange={event => controller.selectProfile(event.target.value)}>
                                    {snapshot.profiles.map(profile => <option key={profile.id} value={profile.id}>{profile.displayName || profile.id}</option>)}
                                </select>
                            </label>
                            <div className="ttas-skill-toolbar-actions">
                                <button type="button" className="menu_button menu_button_icon ttas-primary-button" disabled={busy} title={tr('newSkillImport')} aria-label={tr('newSkillImport')} onClick={() => controller.openImportScopeDialog('manual')}><i className="fa-solid fa-file-circle-plus"></i></button>
                                <button type="button" className="menu_button menu_button_icon ttas-primary-button" disabled={busy} title={tr('downloadSkillImport')} aria-label={tr('downloadSkillImport')} onClick={() => controller.openImportScopeDialog('download')}><i className="fa-solid fa-cloud-arrow-down"></i></button>
                                <button type="button" className="menu_button menu_button_icon" disabled={busy} title={tr('importSkill')} aria-label={tr('importSkill')} onClick={() => controller.openImportScopeDialog('archive')}><i className="fa-solid fa-file-import"></i></button>
                                <button type="button" className="menu_button menu_button_icon" title={tr('refresh')} aria-label={tr('refresh')} onClick={controller.refreshAll}><i className="fa-solid fa-rotate"></i></button>
                            </div>
                        </div>
                        <div className="ttas-skill-search"><i className="fa-solid fa-magnifying-glass"></i><input className="text_pole" type="search" aria-label={tr('searchSkills')} placeholder={tr('searchSkills')} value={snapshot.searchQuery} onChange={event => controller.setSearchQuery(event.target.value)} /></div>
                    </div>
                    <ImportDraft controller={controller} tr={tr} section={importSection} items={snapshot.importDraft.items} installing={snapshot.importDraft.installing} />
                    <div className="ttas-skill-section-list">
                        {snapshot.sections.map(section => <SkillSectionView key={section.id} section={section} query={query} controller={controller} tr={tr} />)}
                    </div>
                </div>
            )}
            <SkillScopeDialog snapshot={snapshot} controller={controller} tr={tr} />
            <SkillSourceDialog snapshot={snapshot} controller={controller} tr={tr} />
            <SkillPreviewDialog snapshot={snapshot} controller={controller} tr={tr} />
        </div>
    );
}
