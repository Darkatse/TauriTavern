import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('power user startup applies persisted SmartTheme colors without relying on color picker events', async () => {
    const source = await readFile(path.join(REPO_ROOT, 'src/scripts/power-user.js'), 'utf8');
    const colorTypes = source.match(/const THEME_COLOR_TYPES = \[([\s\S]*?)\];/);

    assert.ok(colorTypes);
    for (const type of ['main', 'blurTint', 'chatTint', 'userMesBlurTint', 'botMesBlurTint', 'shadow', 'border']) {
        assert.match(colorTypes[1], new RegExp(`'${type}'`));
    }
    assert.match(source, /function applyThemeColor\(type\) \{\s*if \(type === undefined\) \{\s*for \(const themeColorType of THEME_COLOR_TYPES\) \{\s*applyThemeColor\(themeColorType\);/);
    assert.match(source, /export function applyPowerUserSettings\(\) \{[\s\S]*applyThemeColor\(\);/);
    assert.match(source, /const \[red, green, blue, alpha = '1'\] = color\.split/);
});
