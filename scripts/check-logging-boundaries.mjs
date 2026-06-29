import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SRC_ROOT = path.join(REPO_ROOT, 'src-tauri', 'src');

function toPosixPath(value) {
    return String(value).replace(/\\/g, '/');
}

async function listRustFiles(dir) {
    const entries = await fs.readdir(dir, { withFileTypes: true });
    const files = [];

    for (const entry of entries) {
        const fullPath = path.join(dir, entry.name);
        if (entry.isDirectory()) {
            files.push(...await listRustFiles(fullPath));
            continue;
        }
        if (entry.isFile() && entry.name.endsWith('.rs')) {
            files.push(fullPath);
        }
    }

    return files;
}

function collectLineViolations(relPath, line, lineNumber) {
    const violations = [];
    const stripped = line.trim();

    if (stripped.includes('crate::infrastructure::logging::logger')) {
        violations.push('logger facade import');
    }
    if (/logger::(debug|info|warn|error)\s*\(/.test(stripped)) {
        violations.push('logger facade call');
    }
    if (relPath.startsWith('src-tauri/src/application/') && stripped.includes('crate::infrastructure::logging')) {
        violations.push('application -> infrastructure logging');
    }
    if (relPath.startsWith('src-tauri/src/presentation/') && stripped.includes('crate::infrastructure::logging')) {
        violations.push('presentation -> infrastructure logging');
    }
    if (relPath === 'src-tauri/src/presentation/commands/dev_logging_commands.rs' && stripped.includes('crate::infrastructure::')) {
        violations.push('dev_logging_commands -> infrastructure');
    }
    if (relPath === 'src-tauri/src/presentation/commands/settings_commands.rs' && stripped.includes('LlmApiLogStore')) {
        violations.push('settings_commands -> LlmApiLogStore');
    }

    return violations.map((kind) => ({
        relPath,
        lineNumber,
        kind,
        snippet: stripped,
    }));
}

async function main() {
    try {
        await fs.access(SRC_ROOT);
    } catch {
        console.error('[logging-boundaries] src-tauri/src not found; run from repository root.');
        process.exit(2);
    }

    const violations = [];
    for (const filePath of await listRustFiles(SRC_ROOT)) {
        const relPath = toPosixPath(path.relative(REPO_ROOT, filePath));
        const text = await fs.readFile(filePath, 'utf8');
        const lines = text.split(/\r?\n/);

        lines.forEach((line, index) => {
            violations.push(...collectLineViolations(relPath, line, index + 1));
        });
    }

    if (violations.length > 0) {
        for (const violation of violations) {
            console.error(`${violation.relPath}:${violation.lineNumber}: ${violation.kind}: ${violation.snippet}`);
        }
        console.error(`\n[logging-boundaries] ${violations.length} violation(s) found.`);
        process.exit(1);
    }

    console.log('[logging-boundaries] clean');
}

main().catch((error) => {
    console.error('[logging-boundaries] failed:', error);
    process.exit(1);
});
