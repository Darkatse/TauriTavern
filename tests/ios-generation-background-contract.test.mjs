import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PLISTS = [
    'src-tauri/crates/tauritavern/Info.ios.plist',
    'src-tauri/crates/tauritavern/gen/apple/tauritavern_iOS/Info.plist',
];

test('iOS packaging permits continued AI generation tasks', async () => {
    for (const relativePath of PLISTS) {
        const source = await readFile(path.join(REPO_ROOT, relativePath), 'utf8');
        assert.match(source, /<key>BGTaskSchedulerPermittedIdentifiers<\/key>[\s\S]*?<string>\$\(PRODUCT_BUNDLE_IDENTIFIER\)\.ai-generation\.\*<\/string>/);
        assert.match(source, /<key>UIBackgroundModes<\/key>[\s\S]*?<string>processing<\/string>/);
    }
});
