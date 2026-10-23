// Runs every *.perf.mjs scenario in one frontend session and compares its engine counters with
// baseline.json. Any change past the slack fails, including improvements, so every performance change
// is recorded on purpose: `pnpm run test:perf -- --update` writes the current counters.
import { readdir, readFile, writeFile } from 'node:fs/promises';
import { openFrontend } from './frontend.mjs';

const directory = new URL('./', import.meta.url);
const baselineFile = new URL('baseline.json', directory);
const update = process.argv.includes('--update');
// Across runs and CPU speeds, counters stay within a frame or two and a few events of each other; the slack
// absorbs that and keeps small UI additions from needing a new baseline.
const slack = expected => Math.max(2, Math.ceil(expected * 0.1));

const app = await openFrontend();
try {
    for (const file of (await readdir(directory)).filter(name => name.endsWith('.perf.mjs')).sort()) {
        const { default: scenario } = await import(new URL(file, directory));
        await scenario(app);
    }
} finally {
    await app.close();
}
if (app.problems.length > 0) {
    throw new Error(`The frontend reported problems, so its counters are not comparable:\n${app.problems.join('\n')}`);
}

const current = { engine: app.engine, steps: app.results };
if (update) {
    await writeFile(baselineFile, `${JSON.stringify(current, null, 4)}\n`);
    console.log(`perf baseline recorded: ${Object.keys(current.steps).length} steps on ${current.engine}`);
    process.exit(0);
}

const baseline = JSON.parse(await readFile(baselineFile, 'utf8'));
if (baseline.engine !== current.engine) {
    throw new Error(`The baseline was recorded on ${baseline.engine}, but this run used ${current.engine}. Install the pinned browser or record a new baseline.`);
}
const changes = [];
for (const step of new Set([...Object.keys(baseline.steps), ...Object.keys(current.steps)])) {
    const expected = baseline.steps[step], actual = current.steps[step];
    if (!expected || !actual) {
        changes.push(`${step}: ${expected ? 'no longer measured' : 'not in the baseline'}`);
        continue;
    }
    for (const counter of new Set([...Object.keys(expected), ...Object.keys(actual)])) {
        const before = expected[counter], after = actual[counter];
        if (before === undefined || after === undefined || Math.abs(after - before) > slack(before)) {
            changes.push(`${step} ${counter}: ${before} -> ${after}`);
        }
    }
}
if (changes.length > 0) {
    console.error(`Performance counters moved past the baseline. Fix the regression, or record an intended change with \`pnpm run test:perf -- --update\`:\n  ${changes.join('\n  ')}`);
    process.exit(1);
}
console.log(`perf baseline: ${Object.keys(current.steps).length} steps match on ${current.engine}`);
