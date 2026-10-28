// Measures the backend's work for the frontend suite's host calls: tt-perf replays out/trace.json through the
// real command layer over a copy of out/data and counts each step. Run `pnpm run test:perf` first.
// Linux counts exactly on any machine (callgrind instructions, /proc/self/io), builds with the Rust release the
// baseline names, and compares with backend-baseline.json; `--update` records it with the current release. macOS
// and Windows counters depend on the OS and machine, so they compare the working tree with HEAD on this machine,
// and only when sources changed.
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = fileURLToPath(new URL('../../', import.meta.url));
const baselineFile = path.join(repo, 'tests/perf/backend-baseline.json');
const baseRoot = path.join(repo, 'src-tauri/target/perf-base');
const update = process.argv.includes('--update');
const slack = expected => Math.max(2, Math.ceil(expected * 0.1));
const run = (command, args, options = {}) => execFileSync(command, args, { stdio: 'inherit', ...options });
const output = (command, args, options = {}) => execFileSync(command, args, { encoding: 'utf8', ...options }).trim();

/** Builds tt-perf from `root` (with `toolchain` when given) and replays that root's suite output; returns each step's counters. */
function measure(root, toolchain) {
    const inputs = path.join(root, 'tests/perf/out');
    if (!existsSync(path.join(inputs, 'trace.json'))) throw new Error(`${inputs} has no trace; run pnpm run test:perf there first`);
    run('cargo', [...(toolchain ? [`+${toolchain}`] : []), 'build', '--manifest-path', path.join(root, 'src-tauri/Cargo.toml'), '-p', 'tauritavern',
        '--profile', 'perf', '--no-default-features', '--features', 'perf', '--bin', 'tt-perf'], { env: { ...process.env, CARGO_TARGET_DIR: path.join(repo, 'src-tauri/target') } });
    const work = mkdtempSync(path.join(os.tmpdir(), 'tt-perf-'));
    try {
        const binary = path.join(work, `tt-perf${process.platform === 'win32' ? '.exe' : ''}`);
        copyFileSync(path.join(repo, 'src-tauri/target/perf', path.basename(binary)), binary);
        const report = path.join(work, 'report.json');
        const replay = [path.join(inputs, 'data'), path.join(inputs, 'trace.json'), report, root];
        if (process.platform === 'linux') {
            run('valgrind', ['--tool=callgrind', '--dump-before=tt_perf_step_boundary', '--dump-instr=no',
                `--callgrind-out-file=${path.join(work, 'callgrind')}`, binary, ...replay], { stdio: ['ignore', 'ignore', 'inherit'] });
            const { steps, boundaries } = JSON.parse(readFileSync(report, 'utf8'));
            addInstructions(steps, boundaries, work);
            return steps;
        }
        // OS counters also take in work the system does for the process, which only ever adds; the least of three runs leaves the code's own.
        const runs = [0, 1, 2].map(() => {
            run(binary, replay);
            return JSON.parse(readFileSync(report, 'utf8')).steps;
        });
        return Object.fromEntries(Object.entries(runs[0]).map(([step, counters]) => [step,
            Object.fromEntries(Object.keys(counters).map(counter => [counter, Math.min(...runs.map(steps => steps[step][counter]))]))]));
    } finally {
        rmSync(work, { recursive: true, force: true });
    }
}

/** Callgrind closes dump N at boundary N; a step's instructions are in the dump closed at its end. */
function addInstructions(steps, boundaries, work) {
    const dumps = readdirSync(work).filter(name => /^callgrind\.\d+$/.test(name)).sort((a, b) => a.split('.')[1] - b.split('.')[1]);
    if (dumps.length < boundaries.length) throw new Error(`callgrind wrote ${dumps.length} dumps for ${boundaries.length} boundaries`);
    boundaries.forEach(({ step, edge }, index) => {
        if (edge !== 'end') return;
        const summary = readFileSync(path.join(work, dumps[index]), 'utf8').match(/^(?:summary|totals): (\d+)/m);
        if (!summary) throw new Error(`${dumps[index]} has no instruction total`);
        steps[step].instructions = Number(summary[1]);
    });
}

function compare(expected, actual, { failOnDecrease }) {
    const changes = [];
    for (const step of new Set([...Object.keys(expected), ...Object.keys(actual)])) {
        for (const counter of new Set([...Object.keys(expected[step] ?? {}), ...Object.keys(actual[step] ?? {})])) {
            const before = expected[step]?.[counter], after = actual[step]?.[counter];
            if (before === undefined || after === undefined) changes.push(`${step} ${counter}: ${before} -> ${after}`);
            else if (after - before > slack(before) || (failOnDecrease && before - after > slack(before))) changes.push(`${step} ${counter}: ${before} -> ${after}`);
        }
    }
    return changes;
}

/** HEAD's counters on this machine, from a worktree that runs HEAD's own frontend suite and replay. */
function measureHead(sha) {
    const rust = output('rustc', ['--version']).split(' ')[1];
    const cache = path.join(baseRoot, `${sha}.${process.platform}.${rust}.json`);
    if (existsSync(cache)) return JSON.parse(readFileSync(cache, 'utf8'));
    const tree = path.join(baseRoot, 'tree');
    if (existsSync(tree)) run('git', ['-C', tree, 'checkout', '--quiet', '--detach', '--force', sha]);
    else run('git', ['-C', repo, 'worktree', 'add', '--quiet', '--detach', tree, sha]);
    if (!existsSync(path.join(tree, 'tests/perf/backend.mjs'))) return null;
    const pnpm = (...args) => run('pnpm', args, { cwd: tree, shell: process.platform === 'win32' });
    pnpm('install', '--frozen-lockfile', '--prefer-offline');
    pnpm('run', 'test:perf');
    const steps = measure(tree);
    mkdirSync(baseRoot, { recursive: true });
    writeFileSync(cache, JSON.stringify(steps));
    return steps;
}

if (process.platform === 'linux') {
    const baseline = existsSync(baselineFile) ? JSON.parse(readFileSync(baselineFile, 'utf8')) : null;
    const toolchain = update || !baseline ? output('rustc', ['--version']).split(' ')[1] : baseline.toolchain;
    run('rustup', ['toolchain', 'install', toolchain, '--profile', 'minimal']);
    const current = { toolchain, steps: measure(repo, toolchain) };
    const recorded = `${JSON.stringify(current, null, 4)}\n`;
    if (update) {
        writeFileSync(baselineFile, recorded);
        console.log(`backend baseline recorded: ${Object.keys(current.steps).length} steps with Rust ${toolchain}`);
        process.exit(0);
    }
    const changes = baseline ? compare(baseline.steps, current.steps, { failOnDecrease: true }) : ['no baseline recorded'];
    if (changes.length > 0) {
        console.error(`Backend counters moved past the baseline. Fix the regression, or record an intended change with \`pnpm run test:perf:backend -- --update\` on Linux or by committing this run's counters as tests/perf/backend-baseline.json:\n  ${changes.join('\n  ')}\n${recorded}`);
        process.exit(1);
    }
    console.log(`backend baseline: ${Object.keys(current.steps).length} steps match with Rust ${toolchain}`);
} else if (process.env.CI) {
    console.log('backend counters: compared on Linux only in CI');
} else if (!output('git', ['-C', repo, 'status', '--porcelain', '--', 'src', 'src-tauri', 'tests/perf'])) {
    console.log('backend counters: no source changes against HEAD');
} else {
    const sha = output('git', ['-C', repo, 'rev-parse', 'HEAD']);
    const head = measureHead(sha);
    if (!head) {
        console.log('backend counters: HEAD predates the backend perf suite');
        process.exit(0);
    }
    const changes = compare(head, measure(repo), { failOnDecrease: false });
    if (changes.length > 0) {
        console.error(`Backend counters grew past HEAD on this machine. Fix the regression; growth that is intended is measured once committed, and CI records it in the Linux baseline:\n  ${changes.join('\n  ')}`);
        process.exit(1);
    }
    console.log(`backend counters: ${Object.keys(head).length} steps within HEAD's on this machine`);
}
