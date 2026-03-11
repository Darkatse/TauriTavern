import fs from 'node:fs/promises';
import path from 'node:path';
import { init, parse } from 'es-module-lexer';

const DEFAULT_BASELINE_PATH = 'scripts/guardrails/frontend-lines-baseline.json';
const DEFAULT_MAX_FILE_LINES = 500;

const OVER_MAX_WHITELIST = new Set([
    'src/tauri/main/perf/perf-hud.js',
]);

function toPosixPath(value) {
    return String(value).replace(/\\/g, '/');
}

function isFilePathImport(specifier) {
    return typeof specifier === 'string' && (specifier.startsWith('.') || specifier.startsWith('/'));
}

async function pathExists(candidate) {
    try {
        await fs.access(candidate);
        return true;
    } catch {
        return false;
    }
}

async function resolveImportPath({ importerPath, specifier, repoRoot }) {
    if (!specifier || !isFilePathImport(specifier) || specifier.startsWith('/')) {
        return null;
    }

    const baseDir = path.dirname(importerPath);
    const rawResolved = path.resolve(baseDir, specifier);

    const candidates = [];
    if (path.extname(rawResolved)) {
        candidates.push(rawResolved);
    } else {
        candidates.push(`${rawResolved}.js`, `${rawResolved}.mjs`, `${rawResolved}.ts`, `${rawResolved}.mts`);
        candidates.push(path.join(rawResolved, 'index.js'), path.join(rawResolved, 'index.mjs'), path.join(rawResolved, 'index.ts'));
    }

    for (const candidate of candidates) {
        const rel = path.relative(repoRoot, candidate);
        if (rel.startsWith('..') || path.isAbsolute(rel)) {
            continue;
        }

        if (await pathExists(candidate)) {
            return candidate;
        }
    }

    return null;
}

function layerOf(posixRepoRelativePath) {
    const normalized = `/${posixRepoRelativePath}`;
    if (normalized.includes('/src/tauri/main/kernel/')) return 'kernel';
    if (normalized.includes('/src/tauri/main/ports/')) return 'ports';
    if (normalized.includes('/src/tauri/main/adapters/')) return 'adapters';
    if (normalized.includes('/src/tauri/main/services/')) return 'services';
    if (normalized.includes('/src/tauri/main/routes/')) return 'routes';
    return null;
}

function isInsideHostScope(posixRepoRelativePath) {
    return posixRepoRelativePath.startsWith('src/tauri/main/')
        || posixRepoRelativePath === 'src/init.js'
        || posixRepoRelativePath === 'src/tauri-main.js'
        || posixRepoRelativePath === 'src/tauri-bridge.js';
}

function parseArgs(argv) {
    const args = {
        baselinePath: DEFAULT_BASELINE_PATH,
        updateBaseline: false,
        verbose: false,
    };

    for (let index = 0; index < argv.length; index += 1) {
        const token = argv[index];
        if (token === '--baseline') {
            args.baselinePath = String(argv[index + 1] || '').trim() || args.baselinePath;
            index += 1;
            continue;
        }
        if (token === '--update-baseline') {
            args.updateBaseline = true;
            continue;
        }
        if (token === '--verbose') {
            args.verbose = true;
            continue;
        }
    }

    return args;
}

async function readTextFile(filePath) {
    return fs.readFile(filePath, 'utf8');
}

function countLines(text) {
    if (text.length === 0) {
        return 0;
    }

    // Count '\n' characters; if the file doesn't end with a newline, add 1.
    // This matches common tooling expectations and avoids counting a trailing
    // newline as an extra empty line.
    let newlines = 0;
    for (let index = 0; index < text.length; index += 1) {
        if (text.charCodeAt(index) === 10) {
            newlines += 1;
        }
    }

    return text.charCodeAt(text.length - 1) === 10 ? newlines : newlines + 1;
}

async function listHostFiles(repoRoot) {
    const targets = [
        'src/init.js',
        'src/tauri-main.js',
        'src/tauri-bridge.js',
    ];

    const mainDir = path.join(repoRoot, 'src', 'tauri', 'main');
    const stack = [mainDir];

    while (stack.length > 0) {
        const current = stack.pop();
        let entries;
        try {
            entries = await fs.readdir(current, { withFileTypes: true });
        } catch {
            continue;
        }

        for (const entry of entries) {
            const fullPath = path.join(current, entry.name);
            if (entry.isDirectory()) {
                stack.push(fullPath);
                continue;
            }

            const ext = path.extname(entry.name).toLowerCase();
            if (ext !== '.js' && ext !== '.mjs' && ext !== '.ts' && ext !== '.mts') {
                continue;
            }

            targets.push(toPosixPath(path.relative(repoRoot, fullPath)));
        }
    }

    return Array.from(new Set(targets))
        .filter((p) => isInsideHostScope(p))
        .sort();
}

async function loadBaseline(repoRoot, baselinePath) {
    const baselineAbsPath = path.isAbsolute(baselinePath)
        ? baselinePath
        : path.join(repoRoot, baselinePath);

    const raw = await readTextFile(baselineAbsPath);
    const parsed = JSON.parse(raw);

    const maxLinesByPath = parsed?.maxLinesByPath && typeof parsed.maxLinesByPath === 'object'
        ? parsed.maxLinesByPath
        : {};

    return {
        absPath: baselineAbsPath,
        raw: parsed,
        maxLinesByPath: Object.fromEntries(Object.entries(maxLinesByPath).map(([k, v]) => [toPosixPath(k), Number(v)])),
    };
}

async function maybeUpdateBaseline({ repoRoot, baseline, hostFiles, filesByPath }) {
    if (!baseline.raw || !baseline.absPath) {
        throw new Error('Baseline is unavailable');
    }

    const next = { ...baseline.raw, maxLinesByPath: { ...(baseline.raw.maxLinesByPath || {}) } };
    let changed = false;

    for (const [posixPath, maxLines] of Object.entries(baseline.maxLinesByPath)) {
        if (!hostFiles.includes(posixPath)) {
            console.error(`[guardrails] Baseline path missing from host scope: ${posixPath}`);
            console.error(`[guardrails] Remove it from ${toPosixPath(path.relative(repoRoot, baseline.absPath))} or adjust scope.`);
            process.exitCode = 1;
            return;
        }

        const currentLines = filesByPath[posixPath]?.lines;
        if (typeof currentLines !== 'number' || Number.isNaN(currentLines)) {
            continue;
        }

        // Only allow tightening (reducing) baselines automatically.
        if (currentLines < maxLines) {
            next.maxLinesByPath[posixPath] = currentLines;
            changed = true;
        }
    }

    if (!changed) {
        console.log('[guardrails] Baseline unchanged (no reductions detected).');
        return;
    }

    const output = `${JSON.stringify(next, null, 2)}\n`;
    await fs.writeFile(baseline.absPath, output, 'utf8');
    console.log(`[guardrails] Updated baseline (tightened) at ${toPosixPath(path.relative(repoRoot, baseline.absPath))}`);
}

async function main() {
    const repoRoot = process.cwd();

    // Basic sanity check to avoid writing baselines into the wrong directory.
    if (!(await pathExists(path.join(repoRoot, 'package.json')))) {
        throw new Error(`package.json not found in cwd: ${repoRoot}`);
    }

    const args = parseArgs(process.argv.slice(2));
    const baseline = await loadBaseline(repoRoot, args.baselinePath);
    const hostFiles = await listHostFiles(repoRoot);

    const filesByPath = {};
    const lineErrors = [];

    for (const posixPath of hostFiles) {
        const absPath = path.join(repoRoot, posixPath);
        const text = await readTextFile(absPath);
        const lines = countLines(text);

        filesByPath[posixPath] = { absPath, lines, text };

        const baselineMax = baseline.maxLinesByPath[posixPath];
        if (Number.isFinite(baselineMax)) {
            if (lines > baselineMax) {
                lineErrors.push(`${posixPath}: ${lines} lines (baseline max ${baselineMax})`);
            }
            continue;
        }

        if (OVER_MAX_WHITELIST.has(posixPath)) {
            continue;
        }

        if (lines > DEFAULT_MAX_FILE_LINES) {
            lineErrors.push(`${posixPath}: ${lines} lines (max ${DEFAULT_MAX_FILE_LINES})`);
        }
    }

    if (args.updateBaseline) {
        await maybeUpdateBaseline({ repoRoot, baseline, hostFiles, filesByPath });
    }

    await init;

    const importErrors = [];

    const getRepoRelativePosix = (absPath) => toPosixPath(path.relative(repoRoot, absPath));

    for (const posixPath of hostFiles) {
        const { absPath, text } = filesByPath[posixPath] || {};
        if (!absPath || typeof text !== 'string') {
            continue;
        }

        const importerLayer = layerOf(posixPath);
        if (!importerLayer) {
            continue;
        }

        let imports;
        try {
            [imports] = parse(text);
        } catch (error) {
            importErrors.push(`${posixPath}: failed to parse imports (${error?.message || error})`);
            continue;
        }

        for (const entry of imports) {
            const specifier = entry?.n;
            if (!isFilePathImport(specifier)) {
                continue;
            }

            const resolvedAbsPath = await resolveImportPath({ importerPath: absPath, specifier, repoRoot });
            if (!resolvedAbsPath) {
                continue;
            }

            const importeePosix = getRepoRelativePosix(resolvedAbsPath);
            const importeeLayer = layerOf(importeePosix);
            if (!importeeLayer) {
                continue;
            }

            if (importerLayer === 'kernel' && (importeeLayer === 'services' || importeeLayer === 'routes' || importeeLayer === 'adapters')) {
                importErrors.push(`${posixPath}: kernel must not import ${importeeLayer} (${specifier})`);
            }

            if (importerLayer === 'ports' && (importeeLayer === 'services' || importeeLayer === 'routes' || importeeLayer === 'adapters')) {
                importErrors.push(`${posixPath}: ports must not import ${importeeLayer} (${specifier})`);
            }

            if (importerLayer === 'services' && importeeLayer === 'routes') {
                importErrors.push(`${posixPath}: services must not import routes (${specifier})`);
            }
        }
    }

    if (args.verbose) {
        console.log(`[guardrails] Host files checked (${hostFiles.length}):`);
        for (const p of hostFiles) {
            console.log(`- ${p}`);
        }
    }

    const errors = [];
    if (lineErrors.length > 0) {
        errors.push('Line budget violations:');
        for (const message of lineErrors) {
            errors.push(`- ${message}`);
        }
    }

    if (importErrors.length > 0) {
        errors.push('Dependency boundary violations:');
        for (const message of importErrors) {
            errors.push(`- ${message}`);
        }
    }

    if (errors.length > 0) {
        console.error(`[guardrails] FAILED\n${errors.join('\n')}`);
        console.error('[guardrails] Tip: split logic into smaller modules (<= 500 lines) and keep kernel/services/routes boundaries clean.');
        process.exitCode = 1;
        return;
    }

    console.log('[guardrails] OK');
}

main().catch((error) => {
    console.error('[guardrails] ERROR', error);
    process.exitCode = 1;
});
