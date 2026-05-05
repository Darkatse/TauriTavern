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
    background: 'Background',
    binaryFiles: 'binary files',
    byteCount: '{count} bytes',
    cannotDisplayBinarySkillFile: 'Cannot display binary Skill file: {path}',
    capabilityMatrix: 'Capability Matrix',
    charRangeComplete: '{chars}/{totalChars} chars, complete',
    charRangeTruncated: '{chars}/{totalChars} chars, truncated',
    close: 'Close',
    conflict: 'Conflict',
    conflictDifferent: 'Different',
    conflictNew: 'New',
    conflictSame: 'Same',
    contextTools: 'Context',
    controlTools: 'Control',
    copy: 'Copy',
    copyDisplayName: '{name} Copy',
    delete: 'Delete',
    deleteAgentProfileConfirm: 'Delete Agent profile "{id}"?',
    deleteSkillConfirm: 'Delete Skill "{name}"?',
    deletedProfile: 'Deleted profile: {id}',
    deletedSkill: 'Deleted Skill: {name}',
    deniedSkills: 'Denied Skills',
    defaultWriter: 'Default Writer',
    defaultWriterDescription: 'General creative writing Agent profile.',
    description: 'Description',
    displayName: 'Display Name',
    export: 'Export',
    fileCount: '{count} files',
    files: 'Files',
    foreground: 'Foreground',
    hostAgentProfileApiUnavailable: 'TauriTavern Agent profile API is unavailable',
    hostApiUnavailable: 'TauriTavern host API is unavailable: {name}',
    hostExtensionStoreUnavailable: 'TauriTavern extension store API is unavailable',
    hostPopupApiUnavailable: 'SillyTavern Popup API is unavailable',
    identity: 'Identity',
    import: 'Import',
    importDetails: 'Import Details',
    importSkillArchive: 'Import Skill Archive',
    importWarnings: '{count} warnings',
    install: 'Install',
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
    name: 'Name',
    new: 'New',
    newAgentProfile: 'New Agent Profile',
    noFilesFoundForSkill: 'No files found for this Skill.',
    noSkillsInstalled: 'No skills installed.',
    none: 'None',
    openAgentSystem: 'Open Agent System',
    otherTools: 'Other',
    outputArtifact: 'Output Artifact',
    planMode: 'Plan Mode',
    profileCount: '{count} profiles',
    profileSummary: 'Profile Summary',
    presentation: 'Presentation',
    prompt: 'Prompt',
    previewSkillImportFirst: 'Import and preview a Skill archive before installing.',
    profileId: 'Profile ID',
    profiles: 'Profiles',
    refresh: 'Refresh',
    refreshJson: 'Refresh JSON',
    replaceConflict: 'Replace Conflict',
    retryIntervalMs: 'Retry Interval Ms',
    runPolicy: 'Run Policy',
    save: 'Save',
    scriptsIncluded: 'scripts included',
    selectSkillFirst: 'Select a skill first.',
    sendFormNotFound: 'Chat send form not found',
    size: 'Size',
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
    skillAccess: 'Skill Access',
    skillCount: '{count} skills',
    skillTools: 'Skills',
    skills: 'Skills',
    tauriTavernAgent: 'TauriTavern Agent',
    tools: 'Tools',
    unableToAllocateProfileId: 'Unable to allocate a profile id',
    unknownError: 'Unknown error',
    visible: 'Visible',
    visibleSkills: 'Visible Skills',
    writable: 'Writable',
    workspaceAccess: 'Workspace Access',
    workspaceReadTools: 'Workspace Read',
    workspaceRoots: 'Workspace Roots',
    workspaceWriteTools: 'Workspace Write',
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
