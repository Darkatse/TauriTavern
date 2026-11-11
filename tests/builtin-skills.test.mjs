import test from 'node:test';
import assert from 'node:assert/strict';
import { access, readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { staleTtDocsFiles } from '../scripts/builtin-skills/build-tt-docs.mjs';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SKILLS_ROOT = path.join(REPO_ROOT, 'default/skills');

test('built-in tt-docs matches docs/', async () => {
    assert.deepEqual(await staleTtDocsFiles(), [], 'Run: pnpm run skills:tt-docs');
});

async function authoredMarkdown(dir) {
    const files = [];
    for (const entry of await readdir(dir, { withFileTypes: true })) {
        const target = path.join(dir, entry.name);
        if (entry.isDirectory()) files.push(...await authoredMarkdown(target));
        else if (entry.name.endsWith('.md')) files.push(target);
    }
    return files;
}

// Served paths (`/scripts/...`) map to `src/`. `references/...` resolves inside the Skill,
// or inside tt-docs when the line points there ("... in the tt-docs skill"), matching the
// catalog's rule that a Skill's paths are relative to its own directory. Routes (`/api/...`),
// build output (`dist/`) and placeholders such as `<folder>` are not files in the repository.
function referencedFile(span, line, skillDir) {
    if (/[<*{]/.test(span) || span.startsWith('/api/') || /(^|\/)dist(\/|$)/.test(span)) return null;
    if (span.startsWith('references/')) {
        return path.join(line.includes('tt-docs') ? path.join(SKILLS_ROOT, 'tt-docs') : skillDir, span);
    }
    if (/^\/[\w.-]/.test(span)) return path.join(REPO_ROOT, 'src', span);
    return null;
}

test('built-in Skills reference files that exist', async () => {
    const missing = [];
    for (const entry of await readdir(SKILLS_ROOT, { withFileTypes: true })) {
        if (!entry.isDirectory() || entry.name === 'tt-docs') continue;
        const skillDir = path.join(SKILLS_ROOT, entry.name);
        for (const file of await authoredMarkdown(skillDir)) {
            for (const line of (await readFile(file, 'utf8')).split('\n')) {
                for (const [, span] of line.matchAll(/`([^`\s]+)`/g)) {
                    const target = referencedFile(span.split(/[?#]/)[0], line, skillDir);
                    if (target && await access(target).then(() => false, () => true)) {
                        missing.push(`${path.relative(REPO_ROOT, file)}: ${span}`);
                    }
                }
            }
        }
    }
    assert.deepEqual(missing, []);
});
