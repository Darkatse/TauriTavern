import type { SkillImportItem, SkillManagerTr } from './SkillManagerContract';

export function skillImportSourceField(input: TauriTavernSkillImportInput, field: 'kind' | 'label'): string {
    if (!input.source || typeof input.source !== 'object') return '';
    const value = (input.source as Record<string, unknown>)[field];
    return typeof value === 'string' ? value.trim() : '';
}

export function skillImportItemLabel(item: SkillImportItem, tr: SkillManagerTr): string {
    if (item.preview) return item.preview.skill.displayName || item.preview.skill.name;
    const path = 'path' in item.input ? item.input.path.replace(/[\\/]+$/, '') : '';
    return path.split(/[\\/]/).pop() || skillImportSourceField(item.input, 'label') || tr('importSkillArchive');
}

export function manualSkillImportInput(content: string, tr: SkillManagerTr): TauriTavernSkillImportInput {
    if (!content.trim()) throw new Error(tr('skillMdContentRequired'));
    return {
        kind: 'inlineFiles',
        files: [{ path: 'SKILL.md', encoding: 'utf8', content, mediaType: 'text/markdown' }],
        source: { kind: 'manual', label: tr('skillImportSourceManual') },
    };
}

export async function previewSkillImports(options: {
    items: readonly SkillImportItem[];
    targetScope: TauriTavernSkillScope;
    preview: TauriTavernSkillApi['previewImport'];
    isActive: () => boolean;
    onPreview: (index: number, preview: TauriTavernSkillImportPreview) => void;
    onError: (index: number, error: unknown) => void;
}): Promise<void> {
    for (const [index, item] of options.items.entries()) {
        try {
            const preview = await options.preview({ input: item.input, targetScope: options.targetScope });
            if (!options.isActive()) {
                return;
            }
            options.onPreview(index, preview);
        } catch (error) {
            if (!options.isActive()) {
                return;
            }
            if (options.items.length === 1) {
                throw error;
            }
            options.onError(index, error);
        }
    }
}

export async function installSkillImports(options: {
    items: readonly SkillImportItem[];
    targetScope: TauriTavernSkillScope;
    install: TauriTavernSkillApi['installImport'];
    onInstalled: (result: TauriTavernSkillInstallResult) => void;
    syncPortability: (result: TauriTavernSkillInstallResult) => Promise<void>;
    onError: (item: SkillImportItem, error: unknown) => void;
}): Promise<TauriTavernSkillInstallResult[]> {
    const results: TauriTavernSkillInstallResult[] = [];
    for (const item of options.items) {
        const request: Parameters<TauriTavernSkillApi['installImport']>[0] = {
            input: item.input,
            targetScope: options.targetScope,
            ...(item.preview?.conflict.kind === 'different'
                ? { conflictStrategy: item.conflictStrategy }
                : {}),
        };
        try {
            const result = await options.install(request);
            options.onInstalled(result);
            await options.syncPortability(result);
            results.push(result);
        } catch (error) {
            if (options.items.length === 1) {
                throw error;
            }
            options.onError(item, error);
        }
    }
    return results;
}
