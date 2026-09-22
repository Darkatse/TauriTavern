import { useEffect, useRef, useState } from 'react';
import type { AssistantActions } from './host';
import { ErrorNotice, Icon } from './components';
import { errorText, tr } from './i18n';

type Item = { input: TauriTavernSkillImportInput; preview: TauriTavernSkillImportPreview; replace: boolean };
export function SkillImport({ actions, profileId, onInstalled, onPendingChange }: {
    actions: AssistantActions; profileId: string; onInstalled: (name: string) => void; onPendingChange: (pending: boolean) => void;
}) {
    const [items, setItems] = useState<Item[]>([]);
    const [busy, setBusy] = useState<'reading' | 'installing' | null>(null);
    const [error, setError] = useState<unknown>(null);
    const [notice, setNotice] = useState('');
    const owner = useRef<{ disposed: boolean; busy: boolean; release: (() => Promise<void>) | null }>({ disposed: false, busy: false, release: null });
    useEffect(() => {
        const current = owner.current;
        current.disposed = false;
        return () => {
            current.disposed = true;
            if (!current.busy) void current.release?.().catch(error => console.error('[InAppAssistant] Skill cleanup failed', error));
        };
    }, []);
    const scope: TauriTavernSkillScope = { kind: 'profile', profileId };
    async function release() {
        const current = owner.current;
        try { await current.release?.(); } finally {
            current.release = null;
            if (!current.disposed) onPendingChange(false);
        }
    }
    async function pick(directory: boolean) {
        const current = owner.current;
        current.busy = true; setBusy('reading'); setError(null); setNotice(''); onPendingChange(true);
        try {
            current.release = actions.skill.acquireImport();
            const inputs = directory ? await actions.skill.pickImportDirectories() : await actions.skill.pickImportArchives();
            const prepared: Item[] = [];
            for (const input of inputs ?? []) {
                if (current.disposed) break;
                for (const candidate of await actions.skill.discoverImports({ input })) {
                    if (current.disposed) break;
                    prepared.push({ input: candidate, preview: await actions.skill.previewImport({ input: candidate, targetScope: scope }), replace: false });
                }
            }
            if (!prepared.length || current.disposed) await release();
            else setItems(prepared);
        } catch (failure) {
            await release();
            if (!current.disposed) setError(errorText(failure).includes('skill.import_busy') ? tr('importBusy') : failure);
        } finally { current.busy = false; if (!current.disposed) setBusy(null); }
    }
    async function install() {
        const current = owner.current;
        current.busy = true; setBusy('installing'); setError(null);
        const installed: string[] = [];
        try {
            for (const item of items) {
                const result = await actions.skill.installImport({ input: item.input, targetScope: scope, conflictStrategy: item.replace ? 'replace' : 'skip' });
                if (result.action !== 'skipped') {
                    installed.push(result.name);
                    if (!current.disposed) onInstalled(result.name);
                }
                if (current.disposed) break;
            }
            if (!current.disposed) setNotice(tr(installed.length ? 'installed' : 'skip'));
        } catch (failure) {
            if (!current.disposed) setError(installed.length ? `${installed.join(', ')}: ${tr('installed')}\n${errorText(failure)}` : failure);
        } finally {
            await release(); current.busy = false;
            if (!current.disposed) { setItems([]); setBusy(null); }
        }
    }
    return <div className="ttia-skill-import">
        {items.length ? <div className="ttia-import-preview">
            {items.map((item, index) => <div className="ttia-import-item" key={index}>
                <strong>{item.preview.skill.displayName || item.preview.skill.name}</strong>
                <p>{item.preview.skill.description}</p>
                {item.preview.warnings.map(warning => <p className="ttia-warning" key={warning}>{warning}</p>)}
                {item.preview.conflict.kind === 'same' && <small>{tr('sameSkill')}</small>}
                {item.preview.conflict.kind === 'different' && <label><input type="checkbox" checked={item.replace} disabled={busy !== null}
                    onChange={event => setItems(items.map((value, at) => at === index ? { ...value, replace: event.target.checked } : value))} />{tr('replace')}</label>}
            </div>)}
            <p className="ttia-muted">{tr('installNote')}</p>
            <div className="ttia-actions"><button type="button" disabled={busy !== null} onClick={() => {
                setBusy('reading'); void release().then(() => setItems([])).catch(setError).finally(() => setBusy(null));
            }}>{tr('cancel')}</button><button type="button" className="ttia-primary" disabled={busy !== null} onClick={() => { void install(); }}>
                {busy === 'installing' ? tr('installing') : tr('install')}</button></div>
        </div> : <div className="ttia-actions">
            <button type="button" disabled={busy !== null} onClick={() => { void pick(false); }}><Icon name="plus" />{busy ? tr('importing') : tr('importZip')}</button>
            {!actions.isMobile() && <button type="button" disabled={busy !== null} onClick={() => { void pick(true); }}>{tr('importFolder')}</button>}
        </div>}
        {notice && <p className="ttia-muted" role="status">{notice}</p>}
        {error != null && <ErrorNotice error={error} />}
    </div>;
}
