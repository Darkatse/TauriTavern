import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
    getExtensionMenuShortcutIds,
    setExtensionMenuShortcutIds,
} from '../src/scripts/tauri/setting/extension-menu-shortcuts.js';

test('quick access selection defaults to Sync and preserves an explicit empty selection', () => {
    const values = new Map();
    const originalStorage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
    Object.defineProperty(globalThis, 'localStorage', {
        configurable: true,
        value: {
            getItem: key => values.get(key) ?? null,
            setItem: (key, value) => values.set(key, String(value)),
        },
    });

    try {
        assert.deepEqual(getExtensionMenuShortcutIds(), ['sync']);

        setExtensionMenuShortcutIds([]);
        assert.deepEqual(getExtensionMenuShortcutIds(), []);

        assert.throws(() => setExtensionMenuShortcutIds(['unknown']), /unsupported shortcut: unknown/);
    } finally {
        if (originalStorage) {
            Object.defineProperty(globalThis, 'localStorage', originalStorage);
        } else {
            delete globalThis.localStorage;
        }
    }
});
