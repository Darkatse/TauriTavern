import fs from 'node:fs/promises';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const RUST_CRATES_ROOT = path.join(REPO_ROOT, 'src-tauri', 'crates');
const WORKSPACE_MANIFEST = path.join(REPO_ROOT, 'src-tauri', 'Cargo.toml');

const DOMAIN_FORBIDDEN_PACKAGES = new Set([
    'async-trait',
    'axum',
    'image',
    'miktik',
    'reqwest',
    'tar',
    'tauri',
    'tauritavern',
    'tokio',
    'ttsync-core',
    'zip',
]);

const DOMAIN_FORBIDDEN_SOURCE_PATTERNS = [
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

const CONTRACTS_FORBIDDEN_PACKAGES = new Set([
    'async-trait',
    'axum',
    'image',
    'miktik',
    'reqwest',
    'tar',
    'tauri',
    'tauritavern',
    'tokio',
    'tt-ports',
    'ttsync-core',
    'zip',
]);

const CONTRACTS_FORBIDDEN_SOURCE_PATTERNS = [
    ['async-trait', /\basync_trait\b/],
    ['axum', /\baxum::/],
    ['image', /\bimage::/],
    ['miktik', /\bmiktik::/],
    ['reqwest', /\breqwest::/],
    ['tauri', /\btauri::/],
    ['tokio', /\btokio::/],
    ['tt-ports', /\btt_ports::/],
    ['ttsync-core', /\bttsync_core::/],
    ['filesystem IO', /\bstd::fs::/],
    ['network IO', /\bstd::net::/],
];

const PORTS_FORBIDDEN_PACKAGES = new Set([
    'axum',
    'image',
    'miktik',
    'reqwest',
    'tar',
    'tauri',
    'tauritavern',
    'ttsync-core',
    'zip',
]);

const PORTS_FORBIDDEN_SOURCE_PATTERNS = [
    ['axum', /\baxum::/],
    ['image', /\bimage::/],
    ['miktik', /\bmiktik::/],
    ['reqwest', /\breqwest::/],
    ['tauri', /\btauri::/],
    ['ttsync-core', /\bttsync_core::/],
    ['filesystem IO', /\bstd::fs::/],
    ['network IO', /\bstd::net::/],
];

const ADAPTER_HTTP_FORBIDDEN_PACKAGES = new Set([
    'axum',
    'image',
    'miktik',
    'tar',
    'tauri',
    'tauritavern',
    'tt-application',
    'ttsync-core',
    'zip',
]);

const ADAPTER_FORBIDDEN_SOURCE_PATTERNS = [
    ['application path', /\bcrate::application::/],
    ['app host path', /\bcrate::app::/],
    ['infrastructure path', /\bcrate::infrastructure::/],
    ['presentation path', /\bcrate::presentation::/],
    ['tauri', /\btauri::/],
    ['tt-application', /\btt_application::/],
    ['main crate', /\btauritavern(_lib)?::/],
];

const ADAPTER_TOKENIZATION_FORBIDDEN_PACKAGES = new Set([
    'axum',
    'image',
    'tar',
    'tauri',
    'tauritavern',
    'tt-application',
    'ttsync-core',
    'zip',
]);

const ADAPTER_TOKENIZATION_FORBIDDEN_SOURCE_PATTERNS = [
    ...ADAPTER_FORBIDDEN_SOURCE_PATTERNS,
    ['axum', /\baxum::/],
    ['image', /\bimage::/],
    ['reqwest', /\breqwest::/],
    ['ttsync-core', /\bttsync_core::/],
    ['zip', /\bzip::/],
];

const ADAPTER_SYNC_FORBIDDEN_PACKAGES = new Set([
    'image',
    'miktik',
    'qrcode',
    'tar',
    'tauri',
    'tauritavern',
    'tt-adapter-http',
    'tt-adapter-tokenization',
    'tt-application',
    'zip',
]);

const ADAPTER_SYNC_FORBIDDEN_SOURCE_PATTERNS = [
    ...ADAPTER_FORBIDDEN_SOURCE_PATTERNS,
    ['image', /\bimage::/],
    ['qrcode', /\bqrcode::/],
    ['tt-adapter-http', /\btt_adapter_http::/],
    ['tt-adapter-tokenization', /\btt_adapter_tokenization::/],
    ['zip', /\bzip::/],
];

const ADAPTER_ARCHIVE_FORBIDDEN_PACKAGES = new Set([
    'axum',
    'image',
    'miktik',
    'qrcode',
    'reqwest',
    'tauri',
    'tauritavern',
    'tt-adapter-http',
    'tt-adapter-tokenization',
    'tt-adapter-sync',
    'tt-application',
    'ttsync-core',
]);

const ADAPTER_ARCHIVE_FORBIDDEN_SOURCE_PATTERNS = [
    ...ADAPTER_FORBIDDEN_SOURCE_PATTERNS,
    ['axum', /\baxum::/],
    ['image', /\bimage::/],
    ['miktik', /\bmiktik::/],
    ['qrcode', /\bqrcode::/],
    ['reqwest', /\breqwest::/],
    ['tt-adapter-http', /\btt_adapter_http::/],
    ['tt-adapter-tokenization', /\btt_adapter_tokenization::/],
    ['tt-adapter-sync', /\btt_adapter_sync::/],
    ['ttsync-core', /\bttsync_core::/],
];

const CRATES = [
    crateConfig('tt-domain', DOMAIN_FORBIDDEN_PACKAGES, DOMAIN_FORBIDDEN_SOURCE_PATTERNS),
    crateConfig('tt-contracts', CONTRACTS_FORBIDDEN_PACKAGES, CONTRACTS_FORBIDDEN_SOURCE_PATTERNS),
    crateConfig('tt-ports', PORTS_FORBIDDEN_PACKAGES, PORTS_FORBIDDEN_SOURCE_PATTERNS),
    crateConfig('tt-adapter-http', ADAPTER_HTTP_FORBIDDEN_PACKAGES, ADAPTER_FORBIDDEN_SOURCE_PATTERNS),
    crateConfig('tt-adapter-tokenization', ADAPTER_TOKENIZATION_FORBIDDEN_PACKAGES, ADAPTER_TOKENIZATION_FORBIDDEN_SOURCE_PATTERNS),
    crateConfig('tt-adapter-sync', ADAPTER_SYNC_FORBIDDEN_PACKAGES, ADAPTER_SYNC_FORBIDDEN_SOURCE_PATTERNS),
    crateConfig('tt-adapter-archive', ADAPTER_ARCHIVE_FORBIDDEN_PACKAGES, ADAPTER_ARCHIVE_FORBIDDEN_SOURCE_PATTERNS),
];

const MAIN_CRATE_SOURCE_RULES = [
    sourceRule('app composition', path.join(REPO_ROOT, 'src-tauri', 'src', 'app', 'composition'), [
        ['repository facade', /\bcrate::domain::repositories::/],
        ['sync contract facade', /\bcrate::domain::models::sync(_automation)?::/],
        ['data-change port facade', /\bcrate::application::services::data_change_reconciler::/],
    ]),
    sourceRule('web resource adapter', path.join(REPO_ROOT, 'src-tauri', 'src', 'presentation', 'web_resources'), [
        ['client asset contract facade', /\bcrate::application::client_asset_paths::/],
        ['host resource facade', /\bcrate::application::services::host_resource_service::(contract|ports|range)::/],
    ]),
];

function crateConfig(name, forbiddenPackages, forbiddenSourcePatterns) {
    const root = path.join(RUST_CRATES_ROOT, name);
    return {
        name,
        root,
        src: path.join(root, 'src'),
        manifest: path.join(root, 'Cargo.toml'),
        forbiddenPackages,
        forbiddenSourcePatterns,
    };
}

function sourceRule(name, root, forbiddenSourcePatterns) {
    return {
        name,
        root,
        forbiddenSourcePatterns,
    };
}

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

async function fileExists(filePath) {
    try {
        await fs.access(filePath);
        return true;
    } catch {
        return false;
    }
}

async function crateFiles(config) {
    const files = [
        ...(await listFiles(config.src)).filter((file) => file.endsWith('.rs')),
        config.manifest,
    ];
    const buildScript = path.join(config.root, 'build.rs');
    if (await fileExists(buildScript)) {
        files.push(buildScript);
    }
    return files;
}

async function sourceRuleFiles(config) {
    return (await listFiles(config.root)).filter((file) => file.endsWith('.rs'));
}

async function checkSourceBoundaries(config) {
    const violations = [];

    for (const filePath of await crateFiles(config)) {
        const relPath = toPosixPath(path.relative(REPO_ROOT, filePath));
        const text = await fs.readFile(filePath, 'utf8');
        const lines = text.split(/\r?\n/);
        lines.forEach((line, index) => {
            for (const [kind, pattern] of config.forbiddenSourcePatterns) {
                if (pattern.test(line)) {
                    violations.push(`${relPath}:${index + 1}: ${config.name} ${kind}: ${line.trim()}`);
                }
            }
        });
    }

    return violations;
}

async function checkMainCrateSourceRule(config) {
    const violations = [];

    for (const filePath of await sourceRuleFiles(config)) {
        const relPath = toPosixPath(path.relative(REPO_ROOT, filePath));
        const text = await fs.readFile(filePath, 'utf8');
        const lines = text.split(/\r?\n/);
        lines.forEach((line, index) => {
            for (const [kind, pattern] of config.forbiddenSourcePatterns) {
                if (pattern.test(line)) {
                    violations.push(`${relPath}:${index + 1}: ${config.name} ${kind}: ${line.trim()}`);
                }
            }
        });
    }

    return violations;
}

function checkDependencyTree(config) {
    const result = spawnSync('cargo', [
        'tree',
        '--manifest-path',
        WORKSPACE_MANIFEST,
        '-p',
        config.name,
        '--no-default-features',
        '--prefix',
        'none',
    ], {
        cwd: REPO_ROOT,
        encoding: 'utf8',
    });

    if (result.status !== 0) {
        return [`${config.name} cargo tree failed:\n${result.stderr || result.stdout}`];
    }

    const packages = new Set(
        result.stdout
            .split(/\r?\n/)
            .map((line) => line.trim().split(/\s+/)[0])
            .filter(Boolean),
    );

    return [...config.forbiddenPackages]
        .filter((name) => packages.has(name))
        .map((name) => `${config.name} dependency tree includes forbidden package: ${name}`);
}

async function main() {
    const violations = [];
    for (const config of CRATES) {
        violations.push(
            ...(await checkSourceBoundaries(config)),
            ...checkDependencyTree(config),
        );
    }
    for (const config of MAIN_CRATE_SOURCE_RULES) {
        violations.push(...await checkMainCrateSourceRule(config));
    }

    if (violations.length > 0) {
        console.error(`[rust-crate-boundaries] FAILED\n${violations.join('\n')}`);
        process.exitCode = 1;
        return;
    }

    console.log(`[rust-crate-boundaries] clean (${CRATES.map((config) => config.name).join(', ')})`);
}

main().catch((error) => {
    console.error('[rust-crate-boundaries] failed:', error);
    process.exitCode = 1;
});
