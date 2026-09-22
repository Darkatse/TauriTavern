import { useId, useState, type Dispatch, type SetStateAction } from 'react';
import { findModelTargetForBinding, modelBindingFromTarget } from '../../../tauritavern/agent/model-target-llm-connection.js';
import type { AssistantActions, SettingsOptions } from './host';
import { Disclosure, ErrorNotice, Icon } from './components';
import { SkillImport } from './SkillImport';
import { toolName, toolDescription } from './Transcript';
import { tr } from './i18n';

type Profile = TauriTavernAgentProfileDefinition;
export function ModelField({ profile, options, onChange, actions }: {
    profile: Profile; options: SettingsOptions; onChange: (profile: Profile) => void; actions: AssistantActions;
}) {
    const id = useId();
    const match = findModelTargetForBinding(options.models, profile.model);
    const missing = profile.model.mode === 'connectionRef' && !match;
    return <div className="ttia-field">
        <label htmlFor={id}>{tr('model')}</label>
        <select id={id} value={match?.id ?? (missing ? '__saved__' : '')} onChange={event => {
            const target = options.models.find(model => model.id === event.target.value);
            if (target) onChange({ ...profile, model: modelBindingFromTarget(target) });
        }}>
            <option value="" disabled>{tr('selectModel')}</option>
            {missing && <option value="__saved__">{profile.model.modelId} · {tr('bindingMissing')}</option>}
            {options.models.map(model => <option key={model.id} value={model.id}>{model.name || model.model}</option>)}
        </select>
        {options.models.length === 0 && <span className="ttia-muted">{tr('noModels')}</span>}
        <button className="ttia-link" type="button" onClick={() => actions.openConnections()}>{tr('connections')} <Icon name="arrow-up-right-from-square" /></button>
    </div>;
}
export function Setup({ options, profile, actions, busy, onSave }: {
    options: SettingsOptions; profile: Profile; actions: AssistantActions; busy: boolean; onSave: (profile: Profile) => Promise<void>;
}) {
    const onlyModel = options.models.length === 1 ? options.models[0] : undefined;
    const [draft, setDraft] = useState(() => onlyModel && profile.model.mode === 'requiresConfiguration'
        ? { ...structuredClone(profile), model: modelBindingFromTarget(onlyModel) } : structuredClone(profile));
    const [error, setError] = useState<unknown>(null);
    return <div className="ttia-onboarding"><div className="ttia-emblem" aria-hidden="true">A<span /></div>
        <h2>{tr('setupTitle')}</h2><p>{tr('setupNote')}</p>
        <form className="ttia-setup-card" onSubmit={event => {
            event.preventDefault(); setError(null); void onSave(draft).catch(setError);
        }}><fieldset disabled={busy}><ModelField profile={draft} options={options} onChange={setDraft} actions={actions} />
            <p className="ttia-muted">{tr('setupDefaults')}</p>
            {error != null && <ErrorNotice error={error} />}
            <button type="submit" className="ttia-primary" disabled={busy || draft.model.mode !== 'connectionRef'}>{tr(busy ? 'saving' : 'start')}<Icon name="arrow-right" /></button>
        </fieldset></form>
    </div>;
}
export function Settings({ draft, setDraft, options, actions, busy, error, dirty, contentWidth, onContentWidthChange, onSave, onCancel, refreshOptions }: {
    draft: Profile; setDraft: Dispatch<SetStateAction<Profile>>; options: SettingsOptions; actions: AssistantActions;
    contentWidth: number; onContentWidthChange: (value: number) => void;
    busy: boolean; error: unknown; dirty: boolean; onSave: () => void; onCancel: () => void; refreshOptions: () => void;
}) {
    const [search, setSearch] = useState('');
    const [installed, setInstalled] = useState<string[]>([]);
    const [importPending, setImportPending] = useState(false);
    function selectTool(id: string, checked: boolean) {
        setDraft(current => ({ ...current, tools: { ...current.tools,
            allow: checked ? [...new Set([...current.tools.allow, id])] : current.tools.allow.filter(value => value !== id),
            deny: checked ? (current.tools.deny ?? []).filter(value => value !== id) : current.tools.deny ?? [],
        } }));
    }
    const selected = (id: string) => draft.tools.allow.includes(id) && !draft.tools.deny?.includes(id);
    const known = new Set(options.tools.map(tool => tool.id));
    const matching = options.tools.filter(tool => `${toolName(tool.id, tool.title)} ${tool.id} ${toolDescription(tool)}`.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()));
    const groups = ['appTools', 'workspace', 'builtin', 'extensions', 'mcp'] as const;
    function group(tool: TauriTavernAgentToolCatalogItem) {
        return tool.extensionId === 'in-app-agent' ? 'appTools' : tool.source === 'builtin'
            ? tool.id.startsWith('builtin:workspace.') ? 'workspace' : 'builtin' : tool.source === 'extension' ? 'extensions' : 'mcp';
    }
    const skillNames = [...new Set([...options.skills.map(skill => skill.name), ...draft.skills.visible, ...installed])];
    function selectSkill(name: string, checked: boolean) {
        setDraft(current => ({ ...current, skills: { ...current.skills,
            visible: checked ? [...new Set([...current.skills.visible, name])] : current.skills.visible.filter(value => value !== name),
            deny: checked ? (current.skills.deny ?? []).filter(value => value !== name) : current.skills.deny ?? [],
        } }));
    }
    return <form className="ttia-settings" onSubmit={event => { event.preventDefault(); onSave(); }}>
        <fieldset className="ttia-settings-scroll" disabled={busy}>
            <ModelField profile={draft} options={options} onChange={setDraft} actions={actions} />
            <label className="ttia-field"><span>{tr('preset')}</span><select value={draft.preset.ref?.name ?? ''}
                onChange={event => setDraft({ ...draft, preset: { mode: 'ref', ref: { apiId: 'openai', name: event.target.value }, required: true } })}>
                {draft.preset.ref?.name && !options.presets.includes(draft.preset.ref.name) && <option value={draft.preset.ref.name}>{draft.preset.ref.name} · {tr('unavailable')}</option>}
                {options.presets.map(name => <option key={name}>{name}</option>)}
            </select></label>
            {!actions.isMobile() && <label className="ttia-field ttia-desktop-width">
                <span>{tr('contentWidth')}<span>{contentWidth}%</span></span>
                <input type="range" min={40} max={100} step={5} value={contentWidth} aria-label={tr('contentWidth')}
                    onChange={event => onContentWidthChange(event.target.valueAsNumber)} />
                <small className="ttia-muted">{tr('contentWidthHelp')}</small>
            </label>}
            <Disclosure className="ttia-settings-section" label={<><span>{tr('tools')}</span><small>{tr('selected', { count: draft.tools.allow.filter(selected).length })}</small></>}>
                <input type="search" aria-label={tr('searchTools')} placeholder={tr('searchTools')} value={search} onChange={event => setSearch(event.target.value)} />
                {groups.map(name => {
                    const tools = matching.filter(tool => group(tool) === name);
                    return tools.length ? <fieldset key={name}><legend>{tr(name)}</legend>{tools.map(tool => <label className="ttia-choice" key={tool.id}>
                        <input type="checkbox" checked={selected(tool.id)} onChange={event => selectTool(tool.id, event.target.checked)} />
                        <span title={tool.id}><strong>{toolName(tool.id, tool.title || tool.nativeName)}</strong>
                            <small>{tool.enabled === false ? tr('disabled') : toolDescription(tool)}</small></span>
                    </label>)}</fieldset> : null;
                })}
                {search && matching.length === 0 && <p className="ttia-muted">{tr('noTools')}</p>}
                {draft.tools.allow.filter(id => !known.has(id)).map(id => <label className="ttia-choice" key={id}>
                    <input type="checkbox" checked={selected(id)} onChange={event => selectTool(id, event.target.checked)} /><span>{id}<small>{tr('unavailable')}</small></span>
                </label>)}
                {options.diagnostics.map((diagnostic, index) => <p className="ttia-warning" key={index}>{diagnostic.message}</p>)}
            </Disclosure>
            <Disclosure className="ttia-settings-section" label={<><span>{tr('skills')}</span><small>{tr('selected', { count: draft.skills.visible.length })}</small></>}>
                {skillNames.length === 0 && <p className="ttia-muted">{tr('noSkills')}</p>}
                {skillNames.map(name => {
                    const skill = options.skills.find(item => item.name === name);
                    return <label className="ttia-choice" key={name}><input type="checkbox" checked={draft.skills.visible.includes(name) && !draft.skills.deny?.includes(name)}
                        onChange={event => selectSkill(name, event.target.checked)} /><span><strong>{skill?.displayName || name}</strong><small>{skill?.description ?? (installed.includes(name) ? '' : tr('unavailable'))}</small></span></label>;
                })}
                {draft.skills.visible.length > 0 && !selected('builtin:workspace.read_file') && <div className="ttia-note"><p>{tr('skillReadNote')}</p>
                    <button type="button" onClick={() => selectTool('builtin:workspace.read_file', true)}>{tr('enableRead')}</button></div>}
                <SkillImport actions={actions} profileId={draft.id} onPendingChange={setImportPending} onInstalled={name => {
                    setInstalled(current => [...new Set([...current, name])]); selectSkill(name, true); refreshOptions();
                }} />
            </Disclosure>
            <Disclosure className="ttia-settings-section" label={tr('advancedSettings')}>
                <label className="ttia-choice"><input type="checkbox" checked={draft.run.stream} onChange={event => setDraft({ ...draft, run: { ...draft.run, stream: event.target.checked } })} /><span>{tr('stream')}</span></label>
                <label className="ttia-field"><span>{tr('instructions')}</span><textarea rows={10} value={draft.instructions.agentSystemPrompt ?? ''}
                    onChange={event => setDraft({ ...draft, instructions: { ...draft.instructions, agentSystemPrompt: event.target.value } })} /></label>
            </Disclosure>
            {error != null && <ErrorNotice error={error} />}
        </fieldset>
        <footer className="ttia-settings-footer"><small>{tr(dirty ? 'unsaved' : 'nextMessage')}</small><div className="ttia-actions">
            <button type="button" disabled={busy} onClick={onCancel}>{tr('cancel')}</button>
            <button type="submit" className="ttia-primary" disabled={busy || importPending || !dirty || draft.model.mode !== 'connectionRef'}>{tr(busy ? 'saving' : 'save')}</button>
        </div></footer>
    </form>;
}
