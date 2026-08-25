import fs from 'node:fs/promises';
import path from 'node:path';

const OWNED_UI_ROOTS = [
    'src/scripts/extensions/agent-system/src',
    'src/scripts/extensions/mcp-manager/src',
    'src/scripts/tauri/setting',
];

// Ratchet these limits down as each complete Vue root is migrated.
const LIMITS = {
    runtimeTemplates: 0,
    vueImports: 0,
    vueRoots: 0,
};

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

const files = (await Promise.all(OWNED_UI_ROOTS.map(listFiles))).flat();
const vueFiles = files.filter(file => path.extname(file) === '.vue');
const sourceFiles = files.filter(file => ['.js', '.ts', '.tsx'].includes(path.extname(file)));
const counts = {
    runtimeTemplates: 0,
    vueImports: 0,
    vueRoots: 0,
};

for (const file of sourceFiles) {
    const source = await fs.readFile(file, 'utf8');
    counts.runtimeTemplates += source.match(/\btemplate\s*:/gu)?.length ?? 0;
    counts.vueImports += source.match(/\bfrom\s+['"]vue(?:\/[^'"]*)?['"]|\bimport\s*\(\s*['"]vue(?:\/[^'"]*)?['"]\s*\)/gu)?.length ?? 0;
    counts.vueRoots += source.match(/\bcreateApp\s*\(/gu)?.length ?? 0;
}

const errors = vueFiles.map(file => `${file}: first-party UI must not add Vue SFCs`);
for (const [name, limit] of Object.entries(LIMITS)) {
    if (counts[name] > limit) {
        errors.push(`${name}: ${counts[name]} exceeds migration baseline ${limit}`);
    }
}

if (errors.length > 0) {
    console.error(`[first-party-ui] FAILED\n${errors.map(error => `- ${error}`).join('\n')}`);
    process.exitCode = 1;
} else {
    console.log(`[first-party-ui] clean (templates ${counts.runtimeTemplates}, Vue imports ${counts.vueImports}, roots ${counts.vueRoots})`);
}
