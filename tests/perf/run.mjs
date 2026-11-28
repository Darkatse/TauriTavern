// `pnpm run test:perf`: compares the counters measure.mjs takes in both layers. They are exact only on Linux, where
// the harness issues the frontend's frames and the backend runs under callgrind, so Linux compares them with
// baseline.json and fails on any change past the slack, improvements included, so that every change is recorded
// on purpose: `pnpm run test:perf -- --update`, or commit the measurement a failing run prints. macOS and Windows
// leave the comparison to Linux unless asked: `pnpm run test:perf -- --head` compares the working tree with HEAD on
// this machine when sources changed, and fails on growth; a change is recorded on Linux once committed.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { measure } from './measure.mjs';

const repo = fileURLToPath(new URL('../../', import.meta.url));
const baselineFile = fileURLToPath(new URL('baseline.json', import.meta.url));
const update = process.argv.includes('--update');
const againstHead = process.argv.includes('--head');
// Counters differ by a frame or two between runs on one machine; the slack absorbs that and keeps small UI
// additions from needing a new baseline.
const slack = expected => Math.max(2, Math.ceil(expected * 0.1));
const run = (command, args, options = {}) => execFileSync(command, args, { stdio: 'inherit', ...options });
const output = (command, args, options = {}) => execFileSync(command, args, { encoding: 'utf8', ...options }).trim();
const keys = (...objects) => [...new Set(objects.flatMap(object => Object.keys(object ?? {})))];

/**
 * Lists every counter that moved past the slack. Against the baseline, anything measured on one side only is a
 * change too; against HEAD, only counters both sides measured compare, and only growth counts.
 */
function compare(expected, actual, { growthOnly }) {
    const changes = [];
    if (!growthOnly && expected.chromium !== actual.chromium) changes.push(`Chromium: ${expected.chromium} -> ${actual.chromium}`);
    for (const step of keys(expected.steps, actual.steps)) {
        for (const layer of keys(expected.steps[step], actual.steps[step])) {
            const before = expected.steps[step]?.[layer], after = actual.steps[step]?.[layer];
            if (!before || !after) {
                if (!growthOnly) changes.push(`${step} ${layer}: ${before ? 'no longer measured' : 'not in the baseline'}`);
                continue;
            }
            for (const counter of keys(before, after)) {
                const was = before[counter], is = after[counter];
                if (was === undefined || is === undefined) {
                    if (!growthOnly) changes.push(`${step} ${layer} ${counter}: ${was} -> ${is}`);
                } else if (is - was > slack(was) || (!growthOnly && was - is > slack(was))) {
                    changes.push(`${step} ${layer} ${counter}: ${was} -> ${is}`);
                }
            }
        }
    }
    return changes;
}

/** HEAD's measurement on this machine, from a worktree in which HEAD measures itself; null before measure.mjs. */
function measureHead(sha) {
    // An earlier version measured HEAD in perf-base and built it into the working tree's target directory; the
    // artifacts it built stay fresh for the working tree until that tree is gone.
    const sharedTree = path.join(repo, 'src-tauri/target/perf-base');
    if (existsSync(sharedTree)) {
        rmSync(sharedTree, { recursive: true, force: true });
        run('git', ['-C', repo, 'worktree', 'prune']);
    }
    const headRoot = path.join(repo, 'src-tauri/target/perf-head');
    const rust = output('rustc', ['--version']).split(' ')[1];
    const cache = path.join(headRoot, `${sha}.${process.platform}.${rust}.json`);
    if (existsSync(cache)) return JSON.parse(readFileSync(cache, 'utf8'));
    const tree = path.join(headRoot, 'tree');
    if (existsSync(tree)) run('git', ['-C', tree, 'checkout', '--quiet', '--detach', '--force', sha]);
    else run('git', ['-C', repo, 'worktree', 'add', '--quiet', '--detach', tree, sha]);
    if (!existsSync(path.join(tree, 'tests/perf/measure.mjs'))) return null;
    run('pnpm', ['install', '--frozen-lockfile', '--prefer-offline'], { cwd: tree, shell: process.platform === 'win32' });
    run(process.execPath, [path.join(tree, 'tests/perf/measure.mjs'), cache], { cwd: tree });
    return JSON.parse(readFileSync(cache, 'utf8'));
}

if (process.platform === 'linux') {
    const baseline = existsSync(baselineFile) ? JSON.parse(readFileSync(baselineFile, 'utf8')) : null;
    const rust = update || !baseline ? output('rustc', ['--version']).split(' ')[1] : baseline.rust;
    run('rustup', ['toolchain', 'install', rust, '--profile', 'minimal']);
    const current = await measure({ rust });
    const recorded = `${JSON.stringify(current, null, 4)}\n`;
    if (update) {
        writeFileSync(baselineFile, recorded);
        console.log(`perf baseline recorded: ${Object.keys(current.steps).length} steps with Chromium ${current.chromium} and Rust ${rust}`);
        process.exit(0);
    }
    const changes = baseline ? compare(baseline, current, { growthOnly: false }) : ['no baseline recorded'];
    if (changes.length > 0) {
        console.error(`Performance counters moved past the baseline. Fix the regression, or record an intended change with \`pnpm run test:perf -- --update\` on Linux or by committing this measurement as tests/perf/baseline.json:\n  ${changes.join('\n  ')}\n${recorded}`);
        process.exit(1);
    }
    console.log(`perf baseline: ${Object.keys(current.steps).length} steps match with Chromium ${current.chromium} and Rust ${rust}`);
} else if (update) {
    throw new Error('The baseline is recorded on Linux, where the counters are exact: run `pnpm run test:perf -- --update` there, or commit the measurement a failing Linux run prints.');
} else if (!againstHead) {
    console.log('perf counters: compared on Linux; `pnpm run test:perf -- --head` compares the working tree with HEAD on this machine');
} else if (!output('git', ['-C', repo, 'status', '--porcelain', '--', 'src', 'src-tauri', 'tests/perf'])) {
    console.log('perf counters: no source changes against HEAD');
} else {
    const head = measureHead(output('git', ['-C', repo, 'rev-parse', 'HEAD']));
    if (!head) {
        console.log('perf counters: HEAD predates measure.mjs, so there is nothing to compare with');
        process.exit(0);
    }
    const current = await measure();
    if (head.chromium !== current.chromium) {
        console.log(`perf counters: HEAD ran Chromium ${head.chromium} and the working tree ${current.chromium}, so their counters do not compare`);
        process.exit(0);
    }
    const changes = compare(head, current, { growthOnly: true });
    if (changes.length > 0) {
        console.error(`Performance counters grew past HEAD's on this machine. Fix the regression; growth that is intended is measured once committed, and Linux records it in the baseline:\n  ${changes.join('\n  ')}`);
        process.exit(1);
    }
    console.log(`perf counters: ${Object.keys(current.steps).length} steps within HEAD's on this machine`);
}
