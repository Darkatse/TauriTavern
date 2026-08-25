import { skillScopeLabel } from '../skill-scope.js';
import { skillArchiveBlob, type SkillManagerDeps, type SkillPreview, type SkillSection } from './SkillManagerContract';

type AvailableSection = SkillSection & { scope: TauriTavernSkillScope };
type CommittedMutation<T> = {
    onCommitted: (result: T) => void;
    reconcile: () => Promise<unknown>;
};

async function finishCommittedMutation(syncPortability: () => Promise<void>, reconcile: () => Promise<unknown>): Promise<void> {
    let portabilityFailure: { error: unknown } | null = null;
    try {
        await syncPortability();
    } catch (error) {
        portabilityFailure = { error };
    }
    await reconcile();
    if (portabilityFailure) throw portabilityFailure.error;
}

export async function writeSkillFile(
    deps: SkillManagerDeps,
    preview: SkillPreview,
    file: TauriTavernSkillReadResult,
    content: string,
    committed: CommittedMutation<TauriTavernSkillReadResult>,
): Promise<TauriTavernSkillReadResult> {
    const result = await deps.getSkillApi().writeFile({
        scope: preview.scope,
        name: preview.skill.name,
        path: file.path,
        content,
        expectedSha256: file.sha256,
    });
    committed.onCommitted(result);
    await finishCommittedMutation(
        () => deps.syncWritePortability({ scope: preview.scope, name: preview.skill.name }),
        committed.reconcile,
    );
    return result;
}

export async function moveSkillMutation(
    deps: SkillManagerDeps,
    source: AvailableSection,
    skill: TauriTavernSkillIndexEntry,
    target: AvailableSection,
    committed: CommittedMutation<TauriTavernSkillInstallResult>,
): Promise<TauriTavernSkillInstallResult | null> {
    let request: Parameters<TauriTavernSkillApi['move']>[0] = {
        name: skill.name,
        fromScope: source.scope,
        toScope: target.scope,
    };
    const existing = target.skills.find(item => item.name === skill.name);
    if (existing && existing.installedHash !== skill.installedHash) {
        const confirmed = await deps.confirmAction(deps.tr('replaceSkillOnMoveConfirm', {
            name: skill.name,
            scope: skillScopeLabel(target.scope),
        }));
        if (!confirmed) return null;
        request = { ...request, conflictStrategy: 'replace' };
    }
    const result = await deps.getSkillApi().move(request);
    committed.onCommitted(result);
    await finishCommittedMutation(() => deps.syncMovePortability(request, result), committed.reconcile);
    return result;
}

export async function exportSkillArchive(
    deps: SkillManagerDeps,
    scope: TauriTavernSkillScope,
    skill: TauriTavernSkillIndexEntry,
): Promise<boolean> {
    const payload = await deps.getSkillApi().export({ scope, name: skill.name });
    const result = await deps.downloadExport(skillArchiveBlob(payload.contentBase64), payload.fileName, `${skill.name}.zip`);
    return result.mode !== 'ios-native-share' || result.completed === true;
}

export async function deleteSkillMutation(
    deps: SkillManagerDeps,
    scope: TauriTavernSkillScope,
    skill: TauriTavernSkillIndexEntry,
    committed: CommittedMutation<void>,
): Promise<boolean> {
    const confirmed = await deps.confirmAction(deps.tr('deleteScopedSkillConfirm', {
        name: skill.name,
        scope: skillScopeLabel(scope),
    }));
    if (!confirmed) return false;
    await deps.getSkillApi().delete({ scope, name: skill.name });
    committed.onCommitted();
    await finishCommittedMutation(
        () => deps.syncDeletePortability({ scope, name: skill.name }),
        committed.reconcile,
    );
    return true;
}
