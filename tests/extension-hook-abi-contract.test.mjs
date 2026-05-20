import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const EXTENSIONS_ROOT = path.join(REPO_ROOT, 'src/scripts/extensions');

function escapeRegExp(value) {
    return String(value).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

async function readBuiltInExtensionManifests() {
    const entries = await readdir(EXTENSIONS_ROOT, { withFileTypes: true });
    const manifests = [];

    for (const entry of entries) {
        if (!entry.isDirectory()) {
            continue;
        }

        const extensionName = entry.name;
        const manifestPath = path.join(EXTENSIONS_ROOT, extensionName, 'manifest.json');

        try {
            const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
            manifests.push({ extensionName, manifest });
        } catch (error) {
            if (error?.code !== 'ENOENT') {
                throw error;
            }
        }
    }

    return manifests.sort((a, b) => a.extensionName.localeCompare(b.extensionName));
}

async function readActivateHookedManifests() {
    const manifests = await readBuiltInExtensionManifests();
    const hookedManifests = manifests.filter(({ manifest }) => Object.hasOwn(manifest.hooks ?? {}, 'activate'));

    assert.ok(hookedManifests.length > 0, 'at least one built-in extension manifest must declare an activate hook');

    return hookedManifests;
}

test('built-in extension activate hooks use explicit init hooks', async () => {
    for (const { extensionName, manifest } of await readActivateHookedManifests()) {
        assert.equal(manifest.hooks.activate, 'init', `${extensionName} manifest activate hook must point to init`);
    }
});

test('activate-hooked built-in extensions export manifest hook without top-level initialization side effects', async () => {
    for (const { extensionName, manifest } of await readActivateHookedManifests()) {
        assert.equal(typeof manifest.js, 'string', `${extensionName} manifest with activate hook must define a JS entry point`);
        assert.notEqual(manifest.js.length, 0, `${extensionName} manifest JS entry point must not be empty`);

        const hookName = manifest.hooks.activate;
        const hookNamePattern = escapeRegExp(hookName);
        const sourcePath = path.join(EXTENSIONS_ROOT, extensionName, manifest.js);
        const source = await readFile(sourcePath, 'utf8');

        assert.match(
            source,
            new RegExp(`export\\s+(?:async\\s+)?function\\s+${hookNamePattern}\\s*\\(`),
            `${extensionName} must export ${hookName} from ${manifest.js}`,
        );
        assert.doesNotMatch(source, /jQuery\s*\(\s*async/, `${extensionName} must not initialize through top-level jQuery async`);
        assert.doesNotMatch(source, /await\s+init\s*\(\s*\)\s*;/, `${extensionName} must not initialize at module top level`);
    }
});

test('manifest hook failures propagate while hook timeouts remain diagnostic', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/extensions.js'), 'utf8');
    const start = source.indexOf('async function callExtensionHook');
    assert.ok(start >= 0, 'callExtensionHook must exist');
    const end = source.indexOf('/**\n * Enables an extension by name.', start);
    assert.ok(end > start, 'callExtensionHook section must end before enableExtension');
    const section = source.slice(start, end);

    assert.match(section, /throw\s+new\s+Error\(`Extension "\$\{name\}" hook "\$\{hookName\}" is not a valid string`\)/);
    assert.match(section, /throw\s+new\s+Error\(`Extension "\$\{name\}" hook "\$\{hookName\}" references "\$\{hookFunctionName\}" which is not an exported function`\)/);
    assert.match(section, /console\.warn\(`callExtensionHook: Hook "\$\{hookName\}" for extension "\$\{name\}" timed out after \$\{HOOK_TIMEOUT\}ms; continuing without waiting for completion`\)/);
    assert.doesNotMatch(section, /throw\s+new\s+Error\(`Hook "\$\{hookName\}" for extension "\$\{name\}" timed out after \$\{HOOK_TIMEOUT\}ms/);
    assert.doesNotMatch(section, /catch\s*\(/, 'callExtensionHook must not swallow hook import or execution failures');
});

test('extensions are marked active only after activation hook call returns without throwing', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/extensions.js'), 'utf8');
    const start = source.indexOf('const activateSingleExtension = async');
    assert.ok(start >= 0, 'activateSingleExtension must exist');
    const end = source.indexOf('let currentGroup = [];', start);
    assert.ok(end > start, 'activateSingleExtension section must end before activation loop state');
    const section = source.slice(start, end);

    const hookIndex = section.indexOf("await callExtensionHook(name, 'activate');");
    const activeIndex = section.indexOf('activeExtensions.add(name);');

    assert.ok(hookIndex >= 0, 'activate hook must be called');
    assert.ok(activeIndex > hookIndex, 'extension must become active after activate hook returns without throwing');
});
