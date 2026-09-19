import fs from 'node:fs/promises';
import path from 'node:path';

const OWNED_UI_ROOTS = [
    'src/scripts/extensions/agent-system/src',
    'src/scripts/extensions/mcp-manager/src',
    'src/scripts/tauri/setting',
];

const COMPILER_OWNED_ROOTS = [
    'src/scripts/extensions/agent-system/src',
    'src/scripts/extensions/mcp-manager/src',
    'src/scripts/tauri/setting/settings-app',
    'src/scripts/tauri/setting/dev-logs-app',
    'src/scripts/tauri/setting/sync-app',
];

async function listFiles(directory) {
    const files = [];
    for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
        if (entry.name === 'dist') {
            continue;
        }

        const filePath = path.join(directory, entry.name);
        if (entry.isDirectory()) {
            files.push(...await listFiles(filePath));
        } else {
            files.push(filePath);
        }
    }
    return files;
}

const ownedFiles = (await Promise.all(OWNED_UI_ROOTS.map(listFiles))).flat();
const compilerOwnedFiles = (await Promise.all(COMPILER_OWNED_ROOTS.map(listFiles))).flat();
const sourceFiles = ownedFiles.filter(file => /\.(?:js|jsx|ts|tsx)$/u.test(file));
const errors = ownedFiles
    .filter(file => file.endsWith('.vue'))
    .map(file => `${file}: Vue SFCs are retired from first-party UI`);
errors.push(
    ...compilerOwnedFiles
        .filter(file => /(?:\.d\.ts|\.(?:js|jsx))$/u.test(file))
        .map(file => `${file}: compiler-owned first-party UI must use co-located TypeScript/TSX`),
);

for (const file of sourceFiles) {
    const source = await fs.readFile(file, 'utf8');
    if (/\btemplate\s*:/u.test(source)) {
        errors.push(`${file}: runtime templates are retired from first-party UI`);
    }
    if (/\bfrom\s+['"]vue(?:\/[^'"]*)?['"]|\bimport\s*\(\s*['"]vue(?:\/[^'"]*)?['"]\s*\)/u.test(source)) {
        errors.push(`${file}: first-party UI must not import Vue`);
    }
    if (/\bcreateApp\s*\(/u.test(source)) {
        errors.push(`${file}: Vue application roots are retired from first-party UI`);
    }
}

if (errors.length > 0) {
    console.error(`[first-party-ui] FAILED\n${errors.map(error => `- ${error}`).join('\n')}`);
    process.exitCode = 1;
} else {
    console.log('[first-party-ui] clean (React + strict TypeScript/TSX; raw Settings host adapters preserved)');
}
