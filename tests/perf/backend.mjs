// The backend layer: tt-perf, the host crate's `perf` feature, replays the frontend's host calls through the real
// command layer and counts each step's work. Each run prepares a fresh copy of the data and measures in the same
// directory: caches record the files' change times, which a copy would renew. OS counters come from native runs.
// Linux's are exact; elsewhere they also take in work the system does for the process, which only ever adds, so the
// least of three runs counts. Callgrind instructions come from a separate run, which keeps Valgrind's I/O out.
import { execFileSync } from 'node:child_process';
import { copyFileSync, cpSync, mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const run = (command, args, options = {}) => execFileSync(command, args, { stdio: 'inherit', ...options });

/**
 * Builds tt-perf from the tree at `root` with the Rust release `rust`, or the default toolchain, and replays the
 * trace and data the frontend layer left in `inputs`. Returns the release that built it and each step's counters.
 */
export function measureBackend({ root, inputs, rust }) {
    const toolchain = rust ? [`+${rust}`] : [];
    // Each tree builds into its own target directory. Cargo names a workspace's artifacts the same wherever it is
    // checked out and judges them fresh by file times, so two checkouts sharing one reuse each other's builds.
    const target = path.join(root, 'src-tauri/target');
    run('cargo', [...toolchain, 'build', '--manifest-path', path.join(root, 'src-tauri/Cargo.toml'), '-p', 'tauritavern',
        '--profile', 'perf', '--no-default-features', '--features', 'perf', '--bin', 'tt-perf'], { env: { ...process.env, CARGO_TARGET_DIR: target } });
    const release = execFileSync('rustc', [...toolchain, '--version'], { encoding: 'utf8' }).split(' ')[1];
    const work = mkdtempSync(path.join(os.tmpdir(), 'tt-perf-'));
    try {
        const binary = path.join(work, `tt-perf${process.platform === 'win32' ? '.exe' : ''}`);
        copyFileSync(path.join(target, 'perf', path.basename(binary)), binary);
        const app = path.join(work, 'app'), trace = path.join(inputs, 'trace.json'), report = path.join(work, 'report.json');
        function replay(measure) {
            rmSync(app, { recursive: true, force: true });
            cpSync(path.join(inputs, 'data'), path.join(app, 'data'), { recursive: true });
            run(binary, ['prepare', app, trace, root]);
            measure(['measure', app, trace, report, root]);
            return JSON.parse(readFileSync(report, 'utf8'));
        }
        const runs = Array.from({ length: process.platform === 'linux' ? 1 : 3 }, () => replay(args => run(binary, args)).steps);
        const steps = Object.fromEntries(Object.entries(runs[0]).map(([step, counters]) => [step,
            Object.fromEntries(Object.keys(counters).map(counter => [counter, Math.min(...runs.map(each => each[step][counter]))]))]));
        if (process.platform === 'linux') {
            const callgrind = ['--tool=callgrind', '--dump-before=tt_perf_step_boundary', '--dump-instr=no',
                `--callgrind-out-file=${path.join(work, 'callgrind')}`];
            const { boundaries } = replay(args => run('valgrind', [...callgrind, binary, ...args], { stdio: ['ignore', 'ignore', 'inherit'] }));
            addInstructions(steps, boundaries, work);
        }
        return { rust: release, steps };
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
