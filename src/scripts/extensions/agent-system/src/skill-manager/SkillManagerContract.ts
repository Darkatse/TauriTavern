export type SkillManagerTr = (key: string, params?: Record<string, unknown>) => string;

export type SkillSectionId = 'global' | 'preset' | 'profile' | 'character';
export type SkillImportKind = 'archive' | 'directory' | 'manual' | 'download';

export type SkillHostCharacter = {
    avatar?: string | null;
    name?: string | null;
};

export type SkillPresetManager = {
    getSelectedPreset?: () => string | null | undefined;
    getSelectedPresetName?: () => string | null | undefined;
};

export type SkillHostEventSource = {
    on: (eventName: string, listener: () => void) => void;
    removeListener: (eventName: string, listener: () => void) => void;
};

export type SkillHostContext = {
    mainApi?: string | null;
    getPresetManager?: (apiId: string) => SkillPresetManager | null | undefined;
    characterId?: string | number | null;
    characters?: SkillHostCharacter[] | Record<string, SkillHostCharacter | undefined>;
    eventSource?: SkillHostEventSource;
    eventTypes?: Partial<Record<SkillHostEventKey, string>>;
};

export const SKILL_HOST_EVENT_KEYS = [
    'CHAT_CHANGED',
    'CHAT_LOADED',
    'CHARACTER_EDITED',
    'CHARACTER_DELETED',
    'CHARACTER_RENAMED',
    'PRESET_CHANGED',
    'PRESET_DELETED',
    'PRESET_RENAMED',
    'MAIN_API_CHANGED',
] as const;

export type SkillHostEventKey = typeof SKILL_HOST_EVENT_KEYS[number];

export type SkillScopeSection = {
    id: SkillSectionId;
    icon: string;
    labelKey: string;
    available: boolean;
    subtitle: string;
    unavailableKey?: string;
    scope: TauriTavernSkillScope | null;
};

export type SkillSection = SkillScopeSection & {
    skills: readonly TauriTavernSkillIndexEntry[];
    loading: boolean;
};

export type SkillImportItem = {
    input: TauriTavernSkillImportInput;
    preview: TauriTavernSkillImportPreview | null;
    error: string;
    conflictStrategy: TauriTavernSkillInstallConflictStrategy;
};

export type SkillImportDraft = {
    id: number;
    items: readonly SkillImportItem[];
    installing: boolean;
    sectionId: SkillSectionId | '';
};

export type SkillScopeDialog =
    | { mode: '' }
    | {
        mode: 'import';
        importKind: SkillImportKind;
        selectedSectionId: SkillSectionId;
    }
    | {
        mode: 'move';
        selectedSectionId: SkillSectionId;
        sourceSectionId: SkillSectionId;
        skill: TauriTavernSkillIndexEntry;
    };

export type SkillSourceDialog =
    | { mode: '' }
    | {
        id: number;
        mode: 'manual' | 'download';
        sectionId: SkillSectionId;
        content: string;
        url: string;
        loading: boolean;
    };

export type SkillPreview = {
    id: number;
    sectionId: SkillSectionId;
    scope: TauriTavernSkillScope;
    scopeLabel: string;
    skill: TauriTavernSkillIndexEntry;
    files: readonly TauriTavernSkillFileRef[];
    loading: boolean;
    expandedFolders: Readonly<Record<string, boolean>>;
};

export type SkillFileViewerState = {
    id: number;
    file: TauriTavernSkillReadResult;
};

export type SkillManagerSnapshot = {
    initialized: boolean;
    loading: boolean;
    error: string;
    profiles: readonly TauriTavernAgentProfileSummary[];
    selectedProfileId: string;
    sections: readonly SkillSection[];
    importDraft: SkillImportDraft;
    scopeDialog: SkillScopeDialog;
    sourceDialog: SkillSourceDialog;
    searchQuery: string;
    preview: SkillPreview | null;
    fileViewer: SkillFileViewerState | null;
    supportsDirectoryImport: boolean;
};

export type SkillDownloadResult = {
    mode: string;
    completed?: boolean;
};

export type SkillManagerDeps = {
    loadSettings: () => Promise<{ editingProfileId?: string }>;
    subscribeSettings: (listener: (settings: { editingProfileId?: string }) => void) => () => void;
    listProfiles: () => Promise<TauriTavernAgentProfileSummary[]>;
    subscribeProfilesChanged: (listener: () => void) => () => void;
    getHostContext: () => SkillHostContext;
    getSkillApi: () => TauriTavernSkillApi;
    confirmAction: (message: string) => Promise<boolean>;
    downloadExport: (blob: Blob, fileName: string, fallbackName: string) => Promise<SkillDownloadResult>;
    syncInstallPortability: (result: TauriTavernSkillInstallResult) => Promise<void>;
    syncMovePortability: (
        request: Parameters<TauriTavernSkillApi['move']>[0],
        result: TauriTavernSkillInstallResult,
    ) => Promise<void>;
    syncWritePortability: (request: { scope: TauriTavernSkillScope; name: string }) => Promise<void>;
    syncDeletePortability: (request: { scope: TauriTavernSkillScope; name: string }) => Promise<void>;
    supportsDirectoryImport: boolean;
    errorText: (error: unknown) => string;
    reportError: (error: unknown) => void;
    logError: (message: string, error: unknown) => void;
    toastSuccess: (message: string) => void;
    toastError: (message: string) => void;
    translateInstallAction: (action: TauriTavernSkillInstallAction) => string;
    tr: SkillManagerTr;
};

export type SkillManagerController = {
    getSnapshot: () => SkillManagerSnapshot;
    subscribe: (listener: () => void) => () => void;
    init: () => Promise<void>;
    dispose: () => void;
    setSearchQuery: (value: string) => void;
    selectProfile: (profileId: string) => void;
    refreshAll: () => void;
    openImportScopeDialog: (kind: Exclude<SkillImportKind, 'directory'>) => void;
    openMoveScopeDialog: (sectionId: SkillSectionId, skill: TauriTavernSkillIndexEntry) => void;
    setScopeDialogTarget: (sectionId: SkillSectionId) => void;
    setScopeImportKind: (kind: 'archive' | 'directory') => void;
    closeScopeDialog: () => void;
    confirmScopeDialog: () => void;
    setSourceContent: (value: string) => void;
    setSourceUrl: (value: string) => void;
    closeSourceDialog: () => void;
    confirmSourceDialog: () => void;
    setImportConflict: (index: number, strategy: TauriTavernSkillInstallConflictStrategy) => void;
    clearImportDraft: () => void;
    installImports: () => Promise<void>;
    openSkillPreview: (sectionId: SkillSectionId, skill: TauriTavernSkillIndexEntry) => void;
    closePreview: () => void;
    previewClosed: () => void;
    previewCancelled: () => void;
    togglePreviewFolder: (path: string) => void;
    openPreviewFile: (file: TauriTavernSkillFileRef) => void;
    closeFileViewer: () => void;
    saveOpenFile: (content: string) => Promise<TauriTavernSkillReadResult>;
    exportSkill: (sectionId: SkillSectionId, skill: TauriTavernSkillIndexEntry) => void;
    deleteSkill: (sectionId: SkillSectionId, skill: TauriTavernSkillIndexEntry) => void;
    dialogShowFailed: (kind: 'scope' | 'source' | 'preview', error: unknown) => void;
};

export function sortSkillEntries(skills: readonly TauriTavernSkillIndexEntry[]): TauriTavernSkillIndexEntry[] {
    return [...skills].sort((left, right) => (
        (left.displayName || left.name).localeCompare(right.displayName || right.name, undefined, { sensitivity: 'base' })
    ));
}

export function skillArchiveBlob(content: string): Blob {
    const binary = atob(content);
    const bytes = Uint8Array.from(binary, character => character.charCodeAt(0));
    return new Blob([bytes], { type: 'application/zip' });
}

export function emptySkillImportDraft(id: number): SkillImportDraft {
    return { id, items: [], installing: false, sectionId: '' };
}
