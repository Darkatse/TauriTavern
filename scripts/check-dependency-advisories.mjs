#!/usr/bin/env node
// Audits shipped dependencies: src-tauri/Cargo.lock against RustSec, and the
// frontend production dependencies against the npm advisory database.
//
//   node scripts/check-dependency-advisories.mjs [--fix-rust] [--report <file>]
//
// --fix-rust updates each vulnerable crate to its newest semver-compatible
// version and audits again. --report writes the result as Markdown once both
// audits have completed. The exit code is 1 while known advisories remain.

import { spawnSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const RUST_ROOT = path.join(REPO_ROOT, 'src-tauri');
// Registry mirrors usually lack the audit endpoint.
const NPM_REGISTRY = 'https://registry.npmjs.org/';

function parseArgs(argv) {
    const options = { fixRust: false, reportPath: null };
    for (let index = 0; index < argv.length; index += 1) {
        const arg = argv[index];
        if (arg === '--fix-rust') {
            options.fixRust = true;
        } else if (arg === '--report') {
            if (!argv[index + 1]) {
                throw new Error('--report needs a file path');
            }
            options.reportPath = path.resolve(argv[index + 1]);
            index += 1;
        } else {
            throw new Error(`Unknown argument: ${arg}`);
        }
    }
    return options;
}

// Audit tools exit with 1 when they find advisories, so only a missing or
// unreadable JSON report counts as a failure.
function runAudit(label, command, args, cwd) {
    const result = spawnSync(command, args, {
        cwd,
        encoding: 'utf8',
        maxBuffer: 64 * 1024 * 1024,
        shell: process.platform === 'win32',
    });
    if (result.error) {
        throw new Error(`${label} could not start: ${result.error.message}`);
    }
    let report;
    try {
        report = JSON.parse(result.stdout);
    } catch {
        throw new Error(`${label} failed:\n${result.stderr || result.stdout}`);
    }
    if (report.error) {
        throw new Error(`${label} failed: ${report.error.message ?? JSON.stringify(report.error)}`);
    }
    return report;
}

// src-tauri/.cargo/audit.toml is read from the working directory.
function auditRust() {
    const report = runAudit('cargo audit', 'cargo', ['audit', '--json'], RUST_ROOT);
    return report.vulnerabilities.list.map((entry) => ({
        name: entry.package.name,
        version: entry.package.version,
        id: entry.advisory.id,
        title: entry.advisory.title,
        url: `https://rustsec.org/advisories/${entry.advisory.id}.html`,
        patched: entry.versions.patched.join(', ') || 'none',
    }));
}

function auditFrontend() {
    const report = runAudit(
        'pnpm audit',
        'pnpm',
        ['audit', '--prod', '--json', `--registry=${NPM_REGISTRY}`],
        REPO_ROOT,
    );
    return Object.values(report.advisories).map((advisory) => ({
        name: advisory.module_name,
        version: [...new Set(advisory.findings.map((finding) => finding.version))].join(', '),
        id: advisory.github_advisory_id,
        severity: advisory.severity,
        title: advisory.title,
        url: advisory.url,
        patched: advisory.patched_versions,
    }));
}

function updateRustCrates(findings) {
    const specs = [...new Set(findings.map((finding) => `${finding.name}@${finding.version}`))];
    if (specs.length === 0) {
        return [];
    }
    const result = spawnSync('cargo', ['update', ...specs.flatMap((spec) => ['-p', spec])], {
        cwd: RUST_ROOT,
        encoding: 'utf8',
    });
    if (result.status !== 0) {
        throw new Error(`cargo update failed:\n${result.stderr || result.stdout}`);
    }
    return result.stderr
        .split('\n')
        .map((line) => line.trim())
        .filter((line) => /^(Updating|Adding|Removing) /.test(line) && !line.endsWith(' index'));
}

function table(findings, withSeverity) {
    const header = withSeverity
        ? '| Package | Version | Severity | Advisory | Patched |\n| --- | --- | --- | --- | --- |'
        : '| Crate | Version | Advisory | Patched |\n| --- | --- | --- | --- |';
    const rows = findings.map((finding) => {
        const advisory = `[${finding.id}](${finding.url}) ${finding.title.replaceAll('|', '\\|')}`;
        return withSeverity
            ? `| ${finding.name} | ${finding.version} | ${finding.severity} | ${advisory} | ${finding.patched} |`
            : `| ${finding.name} | ${finding.version} | ${advisory} | ${finding.patched} |`;
    });
    return [header, ...rows].join('\n');
}

function renderReport({ rustUpdates, rust, frontend }) {
    const lines = ['## Rust (RustSec, `src-tauri/Cargo.lock`)', ''];
    if (rustUpdates.length > 0) {
        lines.push('This PR applies the semver-compatible updates:', '', '```text', ...rustUpdates, '```', '');
    }
    if (rust.length > 0) {
        lines.push(
            'Still affected. Upgrade by hand, or record why the advisory is unreachable in `src-tauri/.cargo/audit.toml`:',
            '',
            table(rust, false),
            '',
        );
    } else if (rustUpdates.length === 0) {
        lines.push('No known vulnerabilities.', '');
    }

    lines.push('## Frontend (npm advisories, production dependencies)', '');
    if (frontend.length > 0) {
        lines.push(
            'Update by hand after checking SillyTavern ecosystem compatibility, or record why the advisory is unreachable under `auditConfig.ignoreGhsas` in `pnpm-workspace.yaml`:',
            '',
            table(frontend, true),
            '',
        );
    } else {
        lines.push('No known vulnerabilities.', '');
    }
    return lines.join('\n');
}

const options = parseArgs(process.argv.slice(2));
let rust = auditRust();
const rustUpdates = options.fixRust ? updateRustCrates(rust) : [];
if (rustUpdates.length > 0) {
    rust = auditRust();
}
const frontend = auditFrontend();

const report = renderReport({ rustUpdates, rust, frontend });
console.log(report);
if (options.reportPath) {
    writeFileSync(options.reportPath, report);
}
process.exitCode = rust.length + frontend.length > 0 ? 1 : 0;
