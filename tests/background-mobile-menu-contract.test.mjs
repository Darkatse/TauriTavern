import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('mobile background menu close contract avoids focusout/click races', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/backgrounds.js'), 'utf8');

    assert.match(source, /function\s+isInsideOpenMobileBackgroundMenu\(target\)\s*\{/);
    assert.match(source, /\.on\('pointerdown\.backgroundsMobileMenu',\s*onMobileBackgroundDocumentPointerDown\)/);
    assert.match(source, /\.on\('focusin\.backgroundsMobileMenu',\s*onMobileBackgroundDocumentFocusIn\)/);
    assert.doesNotMatch(source, /function\s+onMobileBackgroundMenuFocusOut\b/);
    assert.doesNotMatch(source, /\.on\('focusout',\s*'\.bg_example\.mobile-menu-open'/);
});
