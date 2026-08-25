import { expect, test } from '@rstest/core';

import { previewSkillImports } from './SkillImportOperation';
import type { SkillImportItem } from './SkillManagerContract';

const scope: TauriTavernSkillScope = { kind: 'global' };

function input(name: string): TauriTavernSkillImportInput {
    return { kind: 'archiveFile', path: `/tmp/${name}.zip` };
}

function skill(name: string): TauriTavernSkillIndexEntry {
    return {
        scope,
        name,
        description: '',
        tags: [],
        installedHash: `${name}-hash`,
        fileCount: 1,
        totalBytes: 1,
        hasScripts: false,
        hasBinary: false,
        installedAt: '2026-01-01T00:00:00Z',
    };
}

function preview(name: string, conflict: TauriTavernSkillImportConflictKind = 'new'): TauriTavernSkillImportPreview {
    return { skill: skill(name), files: [], conflict: { kind: conflict }, warnings: [], source: null };
}

function item(name: string, conflict: TauriTavernSkillImportConflictKind = 'new'): SkillImportItem {
    return { input: input(name), preview: preview(name, conflict), error: '', conflictStrategy: 'skip' };
}

test('previews in selection order and isolates failures in a batch', async () => {
    const events: string[] = [];
    const previews: string[] = [];
    const failures: string[] = [];
    const items = ['one', 'bad', 'last'].map(name => ({ ...item(name), preview: null }));

    await previewSkillImports({
        items,
        targetScope: scope,
        preview: async ({ input: selected }) => {
            const name = 'path' in selected ? selected.path.split('/').at(-1)?.replace('.zip', '') ?? '' : '';
            events.push(`start:${name}`);
            await Promise.resolve();
            events.push(`end:${name}`);
            if (name === 'bad') throw new Error('invalid archive');
            return preview(name);
        },
        isActive: () => true,
        onPreview: (_index, result) => previews.push(result.skill.name),
        onError: (_index, error) => failures.push(error instanceof Error ? error.message : ''),
    });

    expect(events).toEqual(['start:one', 'end:one', 'start:bad', 'end:bad', 'start:last', 'end:last']);
    expect(previews).toEqual(['one', 'last']);
    expect(failures).toEqual(['invalid archive']);
});
