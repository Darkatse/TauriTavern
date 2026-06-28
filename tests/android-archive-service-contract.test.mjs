import assert from 'node:assert/strict';
import test from 'node:test';

import { createAndroidArchiveService } from '../src/tauri/main/services/android/android-archive-service.js';

async function withWindow(value, callback) {
    const original = Object.getOwnPropertyDescriptor(globalThis, 'window');
    Object.defineProperty(globalThis, 'window', {
        value,
        configurable: true,
    });

    try {
        return await callback();
    } finally {
        if (original) {
            Object.defineProperty(globalThis, 'window', original);
        } else {
            delete globalThis.window;
        }
    }
}

test('Android data archive import asks Rust for fresh staging target paths', async () => {
    const bridgeName = '__TAURITAVERN_TEST_ANDROID_ARCHIVE__';
    const targetPaths = [
        '/cache/.data-archive/imports/incoming/import-one.archive',
        '/cache/.data-archive/imports/incoming/import-two.archive',
    ];
    const safeCalls = [];
    const staged = [];

    await withWindow({
        __TAURI__: { core: { invoke: async () => null } },
        [bridgeName]: {
            stageContentUriToFile(contentUri, targetFilePath) {
                staged.push({ contentUri, targetFilePath });
                return `${targetFilePath}.staged`;
            },
            requestImportArchivePicker() {},
            requestExportArchivePicker() {},
            copyFileToContentUri() {
                return '';
            },
        },
    }, async () => {
        const service = createAndroidArchiveService({
            bridgeName,
            safeInvoke: async (command, args) => {
                safeCalls.push({ command, args });
                assert.equal(command, 'prepare_data_archive_import_target_path');
                return targetPaths.shift();
            },
            removeTempUploadFile: async () => {},
        });

        const first = await service.materializeAndroidContentUriUpload(' content://import-one ');
        const second = await service.materializeAndroidContentUriUpload('content://import-two');

        assert.deepEqual([first, second].map((file) => ({
            filePath: file.filePath,
            isTemporary: file.isTemporary,
        })), [
            {
                filePath: '/cache/.data-archive/imports/incoming/import-one.archive.staged',
                isTemporary: true,
            },
            {
                filePath: '/cache/.data-archive/imports/incoming/import-two.archive.staged',
                isTemporary: true,
            },
        ]);
        assert.deepEqual(safeCalls, [
            { command: 'prepare_data_archive_import_target_path', args: undefined },
            { command: 'prepare_data_archive_import_target_path', args: undefined },
        ]);
        assert.deepEqual(staged, [
            {
                contentUri: 'content://import-one',
                targetFilePath: '/cache/.data-archive/imports/incoming/import-one.archive',
            },
            {
                contentUri: 'content://import-two',
                targetFilePath: '/cache/.data-archive/imports/incoming/import-two.archive',
            },
        ]);
        assert.equal(targetPaths.length, 0);
    });
});
