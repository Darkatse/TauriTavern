const PREFIX = 'agent_system.';

const DEFAULT_MESSAGES = Object.freeze({
    advancedJson: 'Advanced JSON',
    agentModeOff: 'Agent Mode Off',
    agentModeOn: 'Agent Mode On',
    agentProfileBuiltInDelete: 'default-writer cannot be deleted.',
    agentProfileBuiltInEdit: 'default-writer is built in. Copy it before editing.',
    agentProfileNotFound: 'Agent profile not found: {id}',
    agentProfileSaved: 'Agent profile saved.',
    agentRunActive: 'Agent run active',
    agentSystem: 'Agent System',
    agentSystemDialogUnsupported: 'Agent System requires dialog.showModal support',
    agentSystemElementUnsupported: 'Agent System requires HTMLDialogElement support',
    agentSystemPrompt: 'Agent System Prompt',
    applyJson: 'Apply JSON',
    archiveFile: 'Archive File',
    background: 'Background',
    binaryFiles: 'binary files',
    byteCount: '{count} bytes',
    cannotDisplayBinarySkillFile: 'Cannot display binary Skill file: {path}',
    charRangeComplete: '{chars}/{totalChars} chars, complete',
    charRangeTruncated: '{chars}/{totalChars} chars, truncated',
    close: 'Close',
    copy: 'Copy',
    copyDisplayName: '{name} Copy',
    delete: 'Delete',
    deleteAgentProfileConfirm: 'Delete Agent profile "{id}"?',
    deletedProfile: 'Deleted profile: {id}',
    deniedSkills: 'Denied Skills',
    defaultWriter: 'Default Writer',
    defaultWriterDescription: 'General creative writing Agent profile.',
    description: 'Description',
    directory: 'Directory',
    displayName: 'Display Name',
    export: 'Export',
    fileCount: '{count} files',
    foreground: 'Foreground',
    hostAgentProfileApiUnavailable: 'TauriTavern Agent profile API is unavailable',
    hostApiUnavailable: 'TauriTavern host API is unavailable: {name}',
    hostExtensionStoreUnavailable: 'TauriTavern extension store API is unavailable',
    install: 'Install',
    installSkillFromPath: 'Install Skill From Path',
    invalidSkillFilePath: 'Invalid Skill file path: {path}',
    kind: 'Kind',
    loadingAgentSystem: 'Loading Agent system...',
    loadingSkillFiles: 'Loading Skill files...',
    maxCharsPerCall: 'Max Chars / Call',
    maxCharsPerRun: 'Max Chars / Run',
    maxRounds: 'Max Rounds',
    maxToolCalls: 'Max Tool Calls',
    messageBodyPath: 'Message Body Path',
    mountContainerNotFound: 'agent_system_container not found',
    modelRetries: 'Model Retries',
    new: 'New',
    newAgentProfile: 'New Agent Profile',
    noFilesFoundForSkill: 'No files found for this Skill.',
    noSkillsInstalled: 'No skills installed.',
    none: 'None',
    openAgentSystem: 'Open Agent System',
    outputArtifact: 'Output Artifact',
    planMode: 'Plan Mode',
    presentation: 'Presentation',
    preview: 'Preview',
    profileId: 'Profile ID',
    profiles: 'Profiles',
    refresh: 'Refresh',
    refreshJson: 'Refresh JSON',
    replaceConflict: 'Replace Conflict',
    retryIntervalMs: 'Retry Interval Ms',
    save: 'Save',
    scriptsIncluded: 'scripts included',
    selectSkillFirst: 'Select a skill first.',
    sendFormNotFound: 'Chat send form not found',
    skipConflict: 'Skip Conflict',
    skillActionAlreadyInstalled: 'already installed',
    skillActionInstalled: 'installed',
    skillActionReplaced: 'replaced',
    skillActionSkipped: 'skipped',
    skillFilePathRequired: 'Skill file path is required',
    skillFilesMustBeArray: 'Skill files must be an array',
    skillFileViewerDialogUnsupported: 'Skill file viewer requires dialog.showModal support',
    skillFileViewerElementUnsupported: 'Skill file viewer requires HTMLDialogElement support',
    skillInstallToast: 'Skill {action}: {name}',
    skills: 'Skills',
    tauriTavernAgent: 'TauriTavern Agent',
    tools: 'Tools',
    unableToAllocateProfileId: 'Unable to allocate a profile id',
    unknownError: 'Unknown error',
    visible: 'Visible',
    visibleSkills: 'Visible Skills',
    writable: 'Writable',
    workspaceRoots: 'Workspace Roots',
});

const SKILL_ACTION_KEYS = Object.freeze({
    already_installed: 'skillActionAlreadyInstalled',
    installed: 'skillActionInstalled',
    replaced: 'skillActionReplaced',
    skipped: 'skillActionSkipped',
});

function getSillyTavernContext() {
    if (typeof window === 'undefined') {
        return null;
    }
    return window.SillyTavern?.getContext?.() || null;
}

function formatMessage(message, params) {
    return message.replace(/\{(\w+)}/g, (match, name) => (
        Object.hasOwn(params, name) ? String(params[name]) : match
    ));
}

export function translateAgentSystem(key, params = {}) {
    const defaultMessage = DEFAULT_MESSAGES[key];
    if (defaultMessage === undefined) {
        throw new Error(`Missing Agent System i18n key: ${key}`);
    }

    const translate = getSillyTavernContext()?.translate;
    const message = typeof translate === 'function'
        ? translate(defaultMessage, `${PREFIX}${key}`)
        : defaultMessage;

    return formatMessage(message, params);
}

export function translateSkillInstallAction(action) {
    const normalized = String(action || '').trim();
    const key = SKILL_ACTION_KEYS[normalized];
    return key ? translateAgentSystem(key) : normalized;
}
