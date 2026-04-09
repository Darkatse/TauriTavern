import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

async function importFresh(modulePath) {
    const url = `${pathToFileURL(modulePath).href}?t=${Date.now()}-${Math.random()}`;
    return import(url);
}

test('trimFrontendLogEntriesInPlace retains per-level quotas and preserves order', async () => {
    const { trimFrontendLogEntriesInPlace, FRONTEND_LOG_RETENTION_LIMITS } = await importFresh(
        path.join(REPO_ROOT, 'src/tauri/main/services/dev-logging/frontend-log-retention.js'),
    );

    /** @type {{ id: number, level: 'debug' | 'info' | 'warn' | 'error' }[]} */
    const entries = [];
    let id = 1;

    for (let i = 0; i < 500; i += 1) entries.push({ id: id++, level: 'debug' });
    for (let i = 0; i < 350; i += 1) entries.push({ id: id++, level: 'info' });
    for (let i = 0; i < 50; i += 1) entries.push({ id: id++, level: 'warn' });
    for (let i = 0; i < 70; i += 1) entries.push({ id: id++, level: 'error' });

    trimFrontendLogEntriesInPlace(entries);

    const counts = entries.reduce(
        (acc, entry) => {
            acc[entry.level] = (acc[entry.level] ?? 0) + 1;
            return acc;
        },
        /** @type {Record<string, number>} */ ({}),
    );

    assert.equal(counts.debug, FRONTEND_LOG_RETENTION_LIMITS.debug);
    assert.equal(counts.info, FRONTEND_LOG_RETENTION_LIMITS.info);
    assert.equal((counts.warn ?? 0) + (counts.error ?? 0), FRONTEND_LOG_RETENTION_LIMITS.warnError);

    assert.equal(entries[0]?.id, 101);
    assert.equal(entries.at(-1)?.id, 970);

    const firstInfo = entries.find((entry) => entry.level === 'info');
    assert.equal(firstInfo?.id, 551);

    const firstWarnError = entries.find((entry) => entry.level === 'warn' || entry.level === 'error');
    assert.equal(firstWarnError?.id, 871);

    for (let i = 1; i < entries.length; i += 1) {
        assert.ok(entries[i - 1].id < entries[i].id, 'entries remain ordered by id');
    }
});

