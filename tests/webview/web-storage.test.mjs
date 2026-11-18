import { expect, onTestFinished, test } from '@rstest/core';
import { importFrontend } from '../helpers/frontend.mjs';
import { HOSTS, installHostIdentity } from '../helpers/host-identity.mjs';

const {
    readLocalStorageUsage,
    runScheduledWebStorageCleanup,
    scheduleWebStorageCleanup,
} = await importFrontend('tauri/main/services/web-storage/web-storage.js');

function openDatabase(name) {
    return new Promise((resolve, reject) => {
        const request = indexedDB.open(name, 1);
        request.onupgradeneeded = () => request.result.createObjectStore('records');
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
    });
}

test('a scheduled cleanup deletes only the selection on the next load and reports what the list showed', async () => {
    onTestFinished(installHostIdentity(HOSTS.android));
    localStorage.setItem('tt-test:keep', 'kept');
    localStorage.setItem('tt-test:drop', 'x'.repeat(1000));
    (await openDatabase('tt-test-drop')).close();
    // A connection that ignores versionchange keeps its database from being deleted.
    const busy = await openDatabase('tt-test-busy');
    onTestFinished(() => {
        busy.close();
        localStorage.removeItem('tt-test:keep');
        return new Promise(resolve => { indexedDB.deleteDatabase('tt-test-busy').onsuccess = resolve; });
    });
    const shownBytes = readLocalStorageUsage().entries.find(entry => entry.key === 'tt-test:drop').bytes;

    scheduleWebStorageCleanup({
        localStorageKeys: ['tt-test:drop', 'tt-test:already-gone'],
        databases: ['tt-test-drop', 'tt-test-busy'],
    });
    const report = await runScheduledWebStorageCleanup();

    expect(report).toEqual({
        removedKeys: 1,
        removedBytes: shownBytes,
        deletedDatabases: 1,
        failedDatabases: [{ name: 'tt-test-busy', message: expect.any(String) }],
    });
    expect(localStorage.getItem('tt-test:drop')).toBeNull();
    expect(localStorage.getItem('tt-test:keep')).toBe('kept');
    const names = (await indexedDB.databases()).map(database => database.name);
    expect(names).not.toContain('tt-test-drop');
    expect(await runScheduledWebStorageCleanup(), 'A schedule runs once').toBeNull();
});
