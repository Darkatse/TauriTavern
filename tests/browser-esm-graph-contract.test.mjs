import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, statSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { init, parse } from 'es-module-lexer';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SRC_ROOT = path.join(REPO_ROOT, 'src');
const ENTRY_HTML = path.join(SRC_ROOT, 'index.html');
const STARTUP_ENTRY_MODULES = [
    'lib.js',
    'tauri-main.js',
    'script.js',
];

await init;

function stripUrlSuffix(specifier) {
    return specifier.split(/[?#]/, 1)[0];
}

function isLocalSpecifier(specifier) {
    return specifier.startsWith('/')
        || specifier.startsWith('./')
        || specifier.startsWith('../');
}

function resolveLocalModule(fromFile, specifier) {
    if (!isLocalSpecifier(specifier)) {
        return null;
    }

    const cleanSpecifier = stripUrlSuffix(specifier);
    const basePath = cleanSpecifier.startsWith('/')
        ? path.join(SRC_ROOT, cleanSpecifier.slice(1))
        : path.resolve(path.dirname(fromFile), cleanSpecifier);

    const candidates = [
        basePath,
        `${basePath}.js`,
        `${basePath}.mjs`,
        `${basePath}.json`,
        path.join(basePath, 'index.js'),
        path.join(basePath, 'index.mjs'),
    ];

    for (const candidate of candidates) {
        if (existsSync(candidate) && statSync(candidate).isFile()) {
            return candidate;
        }
    }

    return { missing: basePath };
}

function extractHtmlModuleScripts(html) {
    const scripts = [];
    const scriptTagPattern = /<script\b[^>]*>/gi;

    for (const match of html.matchAll(scriptTagPattern)) {
        const tag = match[0];
        if (!/\btype\s*=\s*["']module["']/i.test(tag)) {
            continue;
        }

        const src = tag.match(/\bsrc\s*=\s*["']([^"']+)["']/i)?.[1];
        if (src) {
            scripts.push(src);
        }
    }

    return scripts;
}

function parseSpecifierRecords(filePath, source) {
    const [imports] = parse(source);
    return imports
        .filter((record) => record.n)
        .map((record) => ({
            specifier: record.n,
            statement: source.slice(record.ss, record.se),
            dynamic: record.d !== -1,
            from: filePath,
        }));
}

function splitNamedBindings(bindings) {
    return bindings
        .split(',')
        .map((binding) => binding.trim())
        .filter(Boolean)
        .map((binding) => binding.replace(/^type\s+/, '').trim())
        .map((binding) => binding.match(/^([^\s]+)\s+as\s+[^\s]+$/)?.[1] ?? binding)
        .filter(Boolean);
}

function requiredExportsForStatement(statement) {
    const required = new Set();
    const trimmed = statement.trim();

    const reExportMatch = trimmed.match(/^export\s*\{([\s\S]*?)\}\s*from\s*["']/);
    if (reExportMatch) {
        for (const name of splitNamedBindings(reExportMatch[1])) {
            required.add(name);
        }
        return required;
    }

    const importMatch = trimmed.match(/^import\s+([\s\S]*?)\s+from\s*["']/);
    if (!importMatch) {
        return required;
    }

    const clause = importMatch[1].trim();
    if (!clause || clause.startsWith('*')) {
        return required;
    }

    const namedMatch = clause.match(/\{([\s\S]*?)\}/);
    if (namedMatch) {
        for (const name of splitNamedBindings(namedMatch[1])) {
            required.add(name);
        }
    }

    const defaultPart = clause.split(',', 1)[0].trim();
    if (defaultPart && !defaultPart.startsWith('{') && !defaultPart.startsWith('*')) {
        required.add('default');
    }

    return required;
}

const sourceCache = new Map();
const exportCache = new Map();

async function readSource(filePath) {
    if (!sourceCache.has(filePath)) {
        sourceCache.set(filePath, await readFile(filePath, 'utf8'));
    }
    return sourceCache.get(filePath);
}

async function collectExports(filePath) {
    if (exportCache.has(filePath)) {
        return exportCache.get(filePath);
    }

    const names = new Set();
    exportCache.set(filePath, names);

    if (!/\.(?:mjs|js)$/.test(filePath)) {
        return names;
    }

    const source = await readSource(filePath);
    const [, exports] = parse(source);
    for (const exported of exports) {
        if (exported.n) {
            names.add(exported.n);
        }
    }

    for (const record of parseSpecifierRecords(filePath, source)) {
        if (!record.statement.trim().startsWith('export *')) {
            continue;
        }

        const target = resolveLocalModule(filePath, record.specifier);
        if (!target || target.missing) {
            continue;
        }

        for (const name of await collectExports(target)) {
            if (name !== 'default') {
                names.add(name);
            }
        }
    }

    return names;
}

async function collectDiagnostics(entryFiles) {
    const diagnostics = [];
    const visited = new Set();

    async function visit(filePath) {
        if (visited.has(filePath)) {
            return;
        }
        visited.add(filePath);

        const source = await readSource(filePath);
        for (const record of parseSpecifierRecords(filePath, source)) {
            if (record.dynamic) {
                continue;
            }

            const target = resolveLocalModule(filePath, record.specifier);
            if (!target) {
                continue;
            }

            if (target.missing) {
                diagnostics.push({
                    type: 'missing-module',
                    from: path.relative(REPO_ROOT, filePath),
                    specifier: record.specifier,
                    target: path.relative(REPO_ROOT, target.missing),
                });
                continue;
            }

            const requiredExports = requiredExportsForStatement(record.statement);
            if (requiredExports.size > 0) {
                const availableExports = await collectExports(target);
                for (const name of requiredExports) {
                    if (!availableExports.has(name)) {
                        diagnostics.push({
                            type: 'missing-export',
                            from: path.relative(REPO_ROOT, filePath),
                            specifier: record.specifier,
                            imported: name,
                            target: path.relative(REPO_ROOT, target),
                        });
                    }
                }
            }

            await visit(target);
        }
    }

    for (const entryFile of entryFiles) {
        await visit(entryFile);
    }

    return diagnostics;
}

function formatDiagnostics(diagnostics) {
    return diagnostics
        .map((diagnostic) => {
            if (diagnostic.type === 'missing-module') {
                return `${diagnostic.from}: cannot resolve ${diagnostic.specifier} -> ${diagnostic.target}`;
            }
            return `${diagnostic.from}: ${diagnostic.specifier} does not export ${diagnostic.imported} (${diagnostic.target})`;
        })
        .join('\n');
}

test('browser startup ESM graph links without missing local modules or exports', async () => {
    const html = await readFile(ENTRY_HTML, 'utf8');
    const entryDiagnostics = [];
    const htmlEntries = [];
    for (const specifier of extractHtmlModuleScripts(html)) {
        const htmlSpecifier = isLocalSpecifier(specifier) ? specifier : `./${specifier}`;
        const resolved = resolveLocalModule(ENTRY_HTML, htmlSpecifier);
        if (!resolved || resolved.missing) {
            entryDiagnostics.push({
                type: 'missing-module',
                from: path.relative(REPO_ROOT, ENTRY_HTML),
                specifier,
                target: path.relative(REPO_ROOT, resolved?.missing ?? specifier),
            });
            continue;
        }
        htmlEntries.push(resolved);
    }

    const startupEntries = STARTUP_ENTRY_MODULES.map((specifier) => path.join(SRC_ROOT, specifier));
    const diagnostics = [
        ...entryDiagnostics,
        ...await collectDiagnostics([...new Set([...htmlEntries, ...startupEntries])]),
    ];

    assert.equal(diagnostics.length, 0, formatDiagnostics(diagnostics));
});
