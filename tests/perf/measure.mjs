// Measures this tree: builds the frontend, runs the frontend layer, and replays its host calls through the backend
// layer. A measurement has the shape of baseline.json. `node tests/perf/measure.mjs <file>` writes one, which is
// how a worktree of another revision measures itself.
import { execFileSync } from 'node:child_process';
import { realpathSync } from 'node:fs';
import { mkdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { measureBackend } from './backend.mjs';
import { writeDataRoot } from './dataset.mjs';
import { measureFrontend } from './frontend.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));

/** Measures both layers; the backend builds with the Rust release `rust`, or the default toolchain. */
export async function measure({ rust } = {}) {
    execFileSync('pnpm', ['run', 'web:build'], { cwd: root, stdio: 'inherit', shell: process.platform === 'win32' });
    const frontend = await measureFrontend();
    // The backend replays the frontend's host calls over the data they were made against; both stay in out/.
    const inputs = path.join(root, 'tests/perf/out');
    await rm(inputs, { recursive: true, force: true });
    await mkdir(inputs, { recursive: true });
    await writeDataRoot(path.join(inputs, 'data'));
    await writeFile(path.join(inputs, 'trace.json'), JSON.stringify({ calls: frontend.calls }));
    const backend = measureBackend({ root, inputs, rust });
    const steps = {};
    for (const step of new Set([...Object.keys(frontend.steps), ...Object.keys(backend.steps)])) {
        steps[step] = {};
        if (frontend.steps[step]) steps[step].frontend = frontend.steps[step];
        if (backend.steps[step]) steps[step].backend = backend.steps[step];
    }
    return { chromium: frontend.chromium, rust: backend.rust, steps };
}

if (realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
    const file = process.argv[2];
    if (!file) throw new Error('Usage: node tests/perf/measure.mjs <file>');
    await writeFile(file, JSON.stringify(await measure()));
}
