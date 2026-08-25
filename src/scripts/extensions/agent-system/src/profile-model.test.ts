import { expect, test } from '@rstest/core';

import { defaultProfile, normalizeProfileForSave, profileForEdit } from './profile-model';

test('profileForEdit migrates v2 native tool names to canonical ToolIds', () => {
    const profile = defaultProfile('legacy-profile');
    profile.schemaVersion = 2;
    profile.tools.allow = ['workspace.read_file'];
    profile.tools.deny = ['workspace.write_file'];
    profile.tools.toolDescriptions = { 'workspace.read_file': { description: 'Read' } };
    profile.tools.maxCallsPerTool = { 'workspace.read_file': 4 };
    // Simulate a v2 persisted profile that predates the field.
    Reflect.deleteProperty(profile.tools, 'mcpResultInlineCharLimit');

    const migrated = profileForEdit(profile);
    expect(migrated.schemaVersion).toBe(3);
    expect(migrated.tools.allow).toEqual(['builtin:workspace.read_file']);
    expect(migrated.tools.deny).toEqual(['builtin:workspace.write_file']);
    expect(Object.keys(migrated.tools.toolDescriptions ?? {})).toEqual(['builtin:workspace.read_file']);
    expect(Object.keys(migrated.tools.maxCallsPerTool ?? {})).toEqual(['builtin:workspace.read_file']);
    expect(migrated.tools.mcpResultInlineCharLimit).toBe(50_000);

    profile.schemaVersion = 4;
    expect(() => profileForEdit(profile)).toThrow(/profile\.schemaVersion is unsupported: 4/);
});

test('profileForEdit keeps CSV drafts separate and normalizeProfileForSave restores lists', () => {
    const profile = defaultProfile('writer');
    profile.skills.visible = ['lore', 'tools'];
    profile.delegation.allowedCallers = ['main', 'reviewer'];

    const draft = profileForEdit(profile);
    expect(draft.skills.visibleCsv).toBe('lore, tools');
    expect(draft.delegation.allowedCallersCsv).toBe('main, reviewer');
    draft.skills.visibleCsv = 'research, tools';
    draft.delegation.allowedCallersCsv = 'editor';

    const saved = normalizeProfileForSave(draft);
    expect(saved.skills.visible).toEqual(['research', 'tools']);
    expect(saved.delegation.allowedCallers).toEqual(['editor']);
    expect('visibleCsv' in saved.skills).toBe(false);
    expect('allowedCallersCsv' in saved.delegation).toBe(false);
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
