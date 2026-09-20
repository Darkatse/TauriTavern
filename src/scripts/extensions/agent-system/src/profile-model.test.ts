import { expect, test } from '@rstest/core';

import { defaultProfile, normalizeProfileForSave, profileForEdit } from './profile-model';

test('profileForEdit migrates legacy tools without widening permissions', () => {
    for (const version of [1, 2, 3]) {
        const profile = defaultProfile('legacy-profile');
        profile.schemaVersion = version;
        const id = (name: string) => version < 3 ? name : `builtin:${name}`;
        profile.tools.allow = [id('workspace.read_file'), id('agent.list'), id('skill.read'), id('skill.run_script')];
        profile.tools.deny = [id('workspace.shell'), id('agent.list'), id('skill.search')];
        profile.tools.toolDescriptions = {
            [id('workspace.read_file')]: { description: 'Read' },
            [id('skill.read')]: { description: 'Old Skill read' },
            [id('agent.list')]: { description: 'Old Agent list' },
        };
        profile.tools.maxCallsPerTool = { [id('workspace.read_file')]: 4, [id('agent.list')]: 1, [id('skill.list')]: 2 };
        Object.assign(profile.skills, { maxReadCharsPerCall: 20_000, maxReadCharsPerRun: 80_000 });
        // Older persisted profiles can predate this field.
        Reflect.deleteProperty(profile.tools, 'mcpResultInlineCharLimit');

        const migrated = profileForEdit(profile);
        expect(migrated.schemaVersion).toBe(4);
        expect(migrated.tools.allow).toEqual(['builtin:workspace.read_file']);
        expect(migrated.tools.deny).toEqual(['builtin:workspace.shell']);
        expect(Object.keys(migrated.tools.toolDescriptions ?? {})).toEqual(['builtin:workspace.read_file']);
        expect(Object.keys(migrated.tools.maxCallsPerTool ?? {})).toEqual(['builtin:workspace.read_file']);
        expect(migrated.tools.mcpResultInlineCharLimit).toBe(50_000);
        expect('maxReadCharsPerCall' in migrated.skills).toBe(false);
        expect('maxReadCharsPerRun' in migrated.skills).toBe(false);

        profile.schemaVersion = 5;
        expect(() => profileForEdit(profile)).toThrow(/profile\.schemaVersion is unsupported: 5/);
    }
});

test('profileForEdit keeps CSV drafts separate and normalizeProfileForSave restores lists', () => {
    const profile = defaultProfile('writer');
    profile.run.stream = false;
    profile.skills.visible = ['lore', 'tools'];
    profile.delegation.allowedCallers = ['main', 'reviewer'];

    const draft = profileForEdit(profile);
    expect(draft.skills.visibleCsv).toBe('lore, tools');
    expect(draft.delegation.allowedCallersCsv).toBe('main, reviewer');
    draft.skills.visibleCsv = 'research, tools';
    draft.delegation.allowedCallersCsv = 'editor';

    const saved = normalizeProfileForSave(draft);
    expect(saved.run.stream).toBe(false);
    expect(saved.skills.visible).toEqual(['research', 'tools']);
    expect(saved.delegation.allowedCallers).toEqual(['editor']);
    expect('visibleCsv' in saved.skills).toBe(false);
    expect('allowedCallersCsv' in saved.delegation).toBe(false);
});

test('run streaming defaults missing fields and rejects non-booleans', () => {
    const profile = defaultProfile('streaming');
    Reflect.deleteProperty(profile.run, 'stream');
    expect(profileForEdit(profile).run.stream).toBe(false);

    Reflect.set(profile.run, 'stream', 'true');
    expect(() => normalizeProfileForSave(profile)).toThrow(/run\.stream must be a boolean/);
});

test('tool description overrides preserve user text and reject invalid values', () => {
    const profile = defaultProfile('descriptions');
    profile.tools.toolDescriptions = {
        'builtin:workspace.read_file': {
            description: '  Read exactly this way.  ',
            properties: { path: '  Use the supplied path.  ' },
        },
    };

    expect(normalizeProfileForSave(profile).tools.toolDescriptions).toEqual(profile.tools.toolDescriptions);

    Reflect.set(profile.tools, 'toolDescriptions', {
        'builtin:workspace.read_file': { description: 42 },
    });
    expect(() => normalizeProfileForSave(profile)).toThrow(/description must be a string/);
});
