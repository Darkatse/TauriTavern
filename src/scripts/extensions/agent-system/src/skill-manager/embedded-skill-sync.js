import {
    embedSkillForScope,
    removeEmbeddedSkillForScope,
} from '../embedded-assets.js';

const PORTABLE_SCOPE_KINDS = new Set(['preset', 'character']);
const COMMITTED_SKILL_ACTIONS = new Set(['installed', 'replaced', 'already_installed']);

function isPortableSkillScope(scope) {
    return PORTABLE_SCOPE_KINDS.has(String(scope?.kind || '').trim());
}

function requireSkillName(value, label = 'skill name') {
    const name = String(value || '').trim();
    if (!name) {
        throw new Error(`${label} is required`);
    }
    return name;
}

function requireScope(value, label) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
        throw new Error(`${label} is required`);
    }
    return value;
}

function skillMutationCommitted(result) {
    const action = String(result?.action || '').trim();
    if (action === 'skipped') {
        return false;
    }
    if (!COMMITTED_SKILL_ACTIONS.has(action)) {
        throw new Error(`Unsupported Skill install action: ${action || '(empty)'}`);
    }
    return true;
}

export async function syncSkillInstallPortability(result) {
    if (!skillMutationCommitted(result)) {
        return;
    }

    const scope = requireScope(result.scope, 'result.scope');
    if (!isPortableSkillScope(scope)) {
        return;
    }

    await embedSkillForScope(scope, requireSkillName(result.name, 'result.name'));
}

export async function syncSkillMovePortability(request, result) {
    if (!skillMutationCommitted(result)) {
        return;
    }

    const name = requireSkillName(result?.name, 'result.name');
    const fromScope = requireScope(request?.fromScope, 'request.fromScope');
    const toScope = requireScope(result?.scope, 'result.scope');

    if (isPortableSkillScope(toScope)) {
        await embedSkillForScope(toScope, name);
    }
    if (isPortableSkillScope(fromScope)) {
        await removeEmbeddedSkillForScope(fromScope, name);
    }
}

export async function syncSkillWritePortability({ scope, name }) {
    const portableScope = requireScope(scope, 'scope');
    if (!isPortableSkillScope(portableScope)) {
        return;
    }

    await embedSkillForScope(portableScope, requireSkillName(name));
}

export async function syncSkillDeletePortability({ scope, name }) {
    const portableScope = requireScope(scope, 'scope');
    if (!isPortableSkillScope(portableScope)) {
        return;
    }

    await removeEmbeddedSkillForScope(portableScope, requireSkillName(name));
}
