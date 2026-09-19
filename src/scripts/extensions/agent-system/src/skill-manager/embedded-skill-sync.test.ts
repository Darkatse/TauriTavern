import { expect, test } from '@rstest/core';

import { EMBEDDED_SKILL_ARCHIVE_FORMAT, readEmbeddedSkillPackage } from '../embedded-asset-packages';
import { syncSkillWritePortability } from './embedded-skill-sync';

type TestCharacter = {
    name: string;
    avatar: string;
    data: {
        extensions: {
            tauritavern: {
                agentProfiles: { version: number; items: Array<{ profile: { id: string } }> };
                skills?: { items: Array<{ contentBase64: string }> };
            };
        };
    };
    json_data: string;
};

test('persisted embedded Skill items are fully validated before use', () => {
    expect(() => readEmbeddedSkillPackage({
        version: 1,
        items: [{
            bundleFormat: EMBEDDED_SKILL_ARCHIVE_FORMAT,
            skillName: 'writer',
            sourceScope: { kind: 'global' },
            sourceScopeLabel: 'Global',
            fileName: 'writer.zip',
            sha256: 'abc123',
        }],
    })).toThrow(/contentBase64/);
});

test('portable Skill sync writes character assets without edit-form coupling', async () => {
    const hostDescriptor = Object.getOwnPropertyDescriptor(window, '__TAURITAVERN__');
    const sillyTavernDescriptor = Object.getOwnPropertyDescriptor(window, 'SillyTavern');
    const previousFetch = globalThis.fetch;
    const fetchCalls: Array<{ url: string; body: Record<string, unknown> }> = [];
    const character: TestCharacter = {
        name: 'Aurelia',
        avatar: 'Aurelia.png',
        data: {
            extensions: {
                tauritavern: {
                    agentProfiles: {
                        version: 1,
                        items: [{ profile: { id: 'stale-local-profile' } }],
                    },
                },
            },
        },
        json_data: JSON.stringify({
            data: {
                extensions: {
                    tauritavern: {
                        agentProfiles: {
                            version: 1,
                            items: [{ profile: { id: 'stale-local-profile' } }],
                        },
                    },
                },
            },
        }),
    };

    Object.defineProperty(window, '__TAURITAVERN__', {
        configurable: true,
        value: {
            api: {
                skill: {
                    export: () => Promise.resolve({
                        fileName: 'writer.zip',
                        contentBase64: 'UEsDBAo=',
                        sha256: 'abc123',
                    }),
                },
            },
        },
    });
    Object.defineProperty(window, 'SillyTavern', {
        configurable: true,
        value: {
            getContext: () => ({
                characters: [character],
                getRequestHeaders: () => ({ 'content-type': 'application/json' }),
            }),
        },
    });
    globalThis.fetch = (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
        if (typeof input !== 'string' || typeof init?.body !== 'string') {
            throw new Error('expected a string request');
        }
        const body = JSON.parse(init.body) as unknown;
        if (!plainObject(body)) {
            throw new Error('expected an object request body');
        }
        fetchCalls.push({ url: input, body });
        return Promise.resolve(new Response('', { status: 200 }));
    };

    try {
        await syncSkillWritePortability({
            scope: { kind: 'character', characterId: 'Aurelia' },
            name: 'writer',
        });

        expect(fetchCalls).toHaveLength(1);
        expect(fetchCalls[0]?.url).toBe('/api/characters/merge-attributes');
        const tauriTavernPatch = nestedObject(fetchCalls[0]?.body, 'data', 'extensions', 'tauritavern');
        expect(Object.keys(tauriTavernPatch)).toEqual(['skills']);
        expect(character.data.extensions.tauritavern.skills?.items[0]?.contentBase64).toBe('UEsDBAo=');
        expect(character.data.extensions.tauritavern.agentProfiles.items[0]?.profile.id).toBe('stale-local-profile');
    } finally {
        globalThis.fetch = previousFetch;
        restoreProperty(window, '__TAURITAVERN__', hostDescriptor);
        restoreProperty(window, 'SillyTavern', sillyTavernDescriptor);
    }
});

function nestedObject(source: unknown, ...path: string[]): Record<string, unknown> {
    let value = source;
    for (const key of path) {
        if (!plainObject(value)) throw new Error(`expected object at ${key}`);
        value = value[key];
    }
    if (!plainObject(value)) throw new Error('expected nested object');
    return value;
}

function plainObject(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function restoreProperty(target: object, key: PropertyKey, descriptor?: PropertyDescriptor): void {
    if (descriptor) Object.defineProperty(target, key, descriptor);
    else Reflect.deleteProperty(target, key);
}
