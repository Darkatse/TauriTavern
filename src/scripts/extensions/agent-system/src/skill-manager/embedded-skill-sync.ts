import {
    embedSkillForScope,
    removeEmbeddedSkillForScope,
} from '../embedded-assets';

const COMMITTED_SKILL_ACTIONS = new Set(['installed', 'replaced', 'already_installed']);

function isPortableSkillScope(
    scope: TauriTavernSkillScope,
): scope is Extract<TauriTavernSkillScope, { kind: 'preset' | 'character' }> {
    return scope.kind === 'preset' || scope.kind === 'character';
}

function requireSkillName(value: string | null | undefined, label = 'skill name'): string {
    const name = String(value ?? '').trim();
    if (!name) {
        throw new Error(`${label} is required`);
    }
    return name;
}

function skillMutationCommitted(result: TauriTavernSkillInstallResult): boolean {
    const action = result.action.trim();
    if (action === 'skipped') {
        return false;
    }
    if (!COMMITTED_SKILL_ACTIONS.has(action)) {
        throw new Error(`Unsupported Skill install action: ${action || '(empty)'}`);
    }
    return true;
}

export async function syncSkillInstallPortability(result: TauriTavernSkillInstallResult): Promise<void> {
    if (!skillMutationCommitted(result)) {
        return;
    }

    const scope = result.scope;
    if (!isPortableSkillScope(scope)) {
        return;
    }

    await embedSkillForScope(scope, requireSkillName(result.name, 'result.name'));
}

export async function syncSkillMovePortability(
    request: Parameters<TauriTavernSkillApi['move']>[0],
    result: TauriTavernSkillInstallResult,
): Promise<void> {
    if (!skillMutationCommitted(result)) {
        return;
    }

    const name = requireSkillName(result?.name, 'result.name');
    const fromScope = request.fromScope;
    const toScope = result.scope;

    if (isPortableSkillScope(toScope)) {
        await embedSkillForScope(toScope, name);
    }
    if (isPortableSkillScope(fromScope)) {
        await removeEmbeddedSkillForScope(fromScope, name);
    }
}

export async function syncSkillWritePortability(
    { scope, name }: { scope: TauriTavernSkillScope; name: string },
): Promise<void> {
    if (!isPortableSkillScope(scope)) {
        return;
    }

    await embedSkillForScope(scope, requireSkillName(name));
}

export async function syncSkillDeletePortability(
    { scope, name }: { scope: TauriTavernSkillScope; name: string },
): Promise<void> {
    if (!isPortableSkillScope(scope)) {
        return;
    }

    await removeEmbeddedSkillForScope(scope, requireSkillName(name));
}
