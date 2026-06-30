import fs from 'node:fs/promises';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DOMAIN_ROOT = path.join(REPO_ROOT, 'src-tauri', 'crates', 'tt-domain');
const DOMAIN_SRC = path.join(DOMAIN_ROOT, 'src');
const DOMAIN_MANIFEST = path.join(DOMAIN_ROOT, 'Cargo.toml');
const WORKSPACE_MANIFEST = path.join(REPO_ROOT, 'src-tauri', 'Cargo.toml');

const FORBIDDEN_PACKAGES = new Set([
    'async-trait',
    'axum',
    'image',
    'miktik',
    'reqwest',
    'tar',
    'tauri',
    'tokio',
    'ttsync-core',
    'zip',
]);

const FORBIDDEN_SOURCE_PATTERNS = [
    ['main crate domain path', /\bcrate::domain::/],
    ['repository path', /\bcrate::repositories::/],
    ['async-trait', /\basync_trait\b/],
    ['axum', /\baxum::/],
    ['image', /\bimage::/],
    ['miktik', /\bmiktik::/],
    ['reqwest', /\breqwest::/],
    ['tauri', /\btauri::/],
    ['tokio', /\btokio::/],
    ['filesystem IO', /\bstd::fs::/],
    ['network IO', /\bstd::net::/],
];

function toPosixPath(value) {
    return String(value).replace(/\\/g, '/');
}

async function listFiles(dir) {
    const entries = await fs.readdir(dir, { withFileTypes: true });
    const files = [];

    for (const entry of entries) {
        const fullPath = path.join(dir, entry.name);
        if (entry.isDirectory()) {
            files.push(...await listFiles(fullPath));
            continue;
        }
        if (entry.isFile()) {
            files.push(fullPath);
        }
    }

    return files;
}

async function checkSourceBoundaries() {
    const files = [
        ...(await listFiles(DOMAIN_SRC)).filter((file) => file.endsWith('.rs')),
        path.join(DOMAIN_ROOT, 'build.rs'),
        DOMAIN_MANIFEST,
    ];
    const violations = [];

    for (const filePath of files) {
        const relPath = toPosixPath(path.relative(REPO_ROOT, filePath));
        const text = await fs.readFile(filePath, 'utf8');
        const lines = text.split(/\r?\n/);
        lines.forEach((line, index) => {
            for (const [kind, pattern] of FORBIDDEN_SOURCE_PATTERNS) {
                if (pattern.test(line)) {
                    violations.push(`${relPath}:${index + 1}: ${kind}: ${line.trim()}`);
                }
            }
        });
    }

    return violations;
}

function checkDependencyTree() {
    const result = spawnSync('cargo', [
        'tree',
        '--manifest-path',
        WORKSPACE_MANIFEST,
        '-p',
        'tt-domain',
        '--no-default-features',
        '--prefix',
        'none',
    ], {
        cwd: REPO_ROOT,
        encoding: 'utf8',
    });

    if (result.status !== 0) {
        return [`cargo tree failed:\n${result.stderr || result.stdout}`];
    }

    const packages = new Set(
        result.stdout
            .split(/\r?\n/)
            .map((line) => line.trim().split(/\s+/)[0])
            .filter(Boolean),
    );

    return [...FORBIDDEN_PACKAGES]
        .filter((name) => packages.has(name))
        .map((name) => `tt-domain dependency tree includes forbidden package: ${name}`);
}

async function main() {
    const violations = [
        ...(await checkSourceBoundaries()),
        ...checkDependencyTree(),
    ];

    if (violations.length > 0) {
        console.error(`[rust-crate-boundaries] FAILED\n${violations.join('\n')}`);
        process.exitCode = 1;
        return;
    }

    console.log('[rust-crate-boundaries] clean');
}

main().catch((error) => {
    console.error('[rust-crate-boundaries] failed:', error);
    process.exitCode = 1;
});
