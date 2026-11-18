import { StrictMode, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createRoot } from 'react-dom/client';

import { formatBytes } from '../format-bytes.js';

export type WebStorageTranslate = (key: string) => string;

export type WebStorageSnapshot = {
    localStorage: {
        usedBytes: number;
        quotaBytes: number;
        nearlyFull: boolean;
        entries: { key: string; bytes: number }[];
    };
    databases: string[];
};

export type WebStorageSelection = {
    localStorageKeys: string[];
    databases: string[];
};

export type WebStorageMountOptions = {
    snapshot: WebStorageSnapshot;
    actions: {
        cleanUp: (selection: WebStorageSelection) => Promise<unknown>;
        copyText: (text: string) => Promise<unknown>;
    };
    tr: WebStorageTranslate;
};

export type WebStorageHandle = {
    unmount: () => void;
};

type ListItem = { id: string; label: string; size?: string };

const EXTENSION_STORE_DOCS_URL = 'https://github.com/Darkatse/TauriTavern/blob/main/docs/API/Extension.md';

const EXTENSION_STORE_EXAMPLE = `const tt = window.__TAURITAVERN__;
const store = tt?.api?.extension?.store;

async function loadSettings() {
    if (!store) {
        return JSON.parse(localStorage.getItem('my-extension') ?? 'null');
    }
    await tt.ready;
    const saved = await store.tryGetJson({ namespace: 'my-extension', key: 'settings' });
    return saved.found ? saved.value : null;
}

async function saveSettings(settings) {
    if (!store) {
        localStorage.setItem('my-extension', JSON.stringify(settings));
        return;
    }
    await tt.ready;
    await store.setJson({ namespace: 'my-extension', key: 'settings', value: settings });
}`;

/** The request a user forwards to an extension author, written for the author or their coding assistant. */
function authorRequest(tr: WebStorageTranslate): string {
    const notes = [
        'Detect window.__TAURITAVERN__?.api?.extension?.store first and keep the current storage when it is absent.',
        'Every method is asynchronous. JSON: getJson, tryGetJson, setJson, updateJson, deleteJson, listKeys. Binary: setBlob, getBlob.',
        'namespace, table and key may only use letters, digits, _, . and -. Each key is a separate file, so split data that keeps growing into several keys and read them when needed.',
        'State that only belongs to this device\'s interface (window positions, per-device toggles) can stay in localStorage. Put caches that can be rebuilt in IndexedDB; keeping them in localStorage is not recommended.',
        'On the first run in TauriTavern, copy the data already in localStorage and IndexedDB into the extension store, and delete the old copies only after the writes succeed.',
    ];
    return [
        tr('Your extension (or character card script) keeps its data in the WebView\'s localStorage or IndexedDB in TauriTavern. TauriTavern is a native SillyTavern client: all extensions share about 5 MB of localStorage, and once it is full every extension fails to save. This data also stays on the current device only and cannot move with TauriTavern sync or backups.'),
        tr('In TauriTavern, please save the data that should be kept with the native extension store, window.__TAURITAVERN__.api.extension.store, instead. It has a much higher capacity limit, keeps data in the user\'s data directory, and moves to other devices with sync. Keep the current behavior unchanged in upstream SillyTavern.'),
        EXTENSION_STORE_EXAMPLE,
        [tr('Notes:'), ...notes.map(note => `- ${tr(note)}`)].join('\n'),
        tr('Full reference: {url}').replace('{url}', EXTENSION_STORE_DOCS_URL),
    ].join('\n\n');
}

function AuthorRequest({ copyText, tr }: { copyText: (text: string) => Promise<unknown>; tr: WebStorageTranslate }) {
    const text = authorRequest(tr);
    return (
        <details className="tt-web-storage-author">
            <summary>{tr('Note for extension authors')}</summary>
            <small>
                {tr('Please copy this and send it to the extension or character card author. Once they make the changes, their data no longer uses the limited web storage and moves to other devices with sync.')}
            </small>
            <textarea className="text_pole" readOnly rows={10} value={text} aria-label={tr('Note for extension authors')}></textarea>
            <button
                type="button"
                className="menu_button menu_button_icon tt-web-storage-copy"
                onClick={() => void copyText(text)}
            >
                <i className="fa-solid fa-copy" aria-hidden="true"></i>
                <span>{tr('Copy')}</span>
            </button>
        </details>
    );
}

function toggled(set: ReadonlySet<string>, id: string, checked: boolean): ReadonlySet<string> {
    const next = new Set(set);
    if (checked) {
        next.add(id);
    } else {
        next.delete(id);
    }
    return next;
}

type SelectableListProps = {
    items: ListItem[];
    selected: ReadonlySet<string>;
    tr: WebStorageTranslate;
    onChange: (selected: ReadonlySet<string>) => void;
};

function SelectableList({ items, selected, tr, onChange }: SelectableListProps) {
    const selectAllRef = useRef<HTMLInputElement>(null);
    const allSelected = items.length > 0 && selected.size === items.length;
    const someSelected = selected.size > 0 && !allSelected;

    useLayoutEffect(() => {
        if (selectAllRef.current) {
            selectAllRef.current.indeterminate = someSelected;
        }
    }, [someSelected]);

    if (items.length === 0) {
        return <small className="tt-web-storage-empty">{tr('Nothing stored.')}</small>;
    }

    return (
        <div className="tt-web-storage-list">
            <label className="tt-web-storage-item tt-web-storage-select-all">
                <input
                    ref={selectAllRef}
                    type="checkbox"
                    checked={allSelected}
                    onChange={event => onChange(new Set(event.target.checked ? items.map(item => item.id) : []))}
                />
                <span className="tt-web-storage-item-name">{tr('Select All')}</span>
            </label>
            {items.map(item => (
                <label key={item.id} className="tt-web-storage-item">
                    <input
                        type="checkbox"
                        checked={selected.has(item.id)}
                        onChange={event => onChange(toggled(selected, item.id, event.target.checked))}
                    />
                    <span className="tt-web-storage-item-name">{item.label}</span>
                    {item.size && <span className="tt-web-storage-item-size">{item.size}</span>}
                </label>
            ))}
        </div>
    );
}

type GroupProps = {
    title: string;
    technicalName: string;
    children: ReactNode;
};

function StorageGroup({ title, technicalName, children }: GroupProps) {
    return (
        <section className="tt-web-storage-group" aria-label={title}>
            <div className="tt-web-storage-group-title">
                <b>{title}</b>
                {technicalName !== title && <small>{technicalName}</small>}
            </div>
            {children}
        </section>
    );
}

function WebStorageApp({ snapshot, actions, tr }: WebStorageMountOptions) {
    const [selectedKeys, setSelectedKeys] = useState<ReadonlySet<string>>(() => new Set());
    const [selectedDatabases, setSelectedDatabases] = useState<ReadonlySet<string>>(() => new Set());
    const [busy, setBusy] = useState(false);
    const usage = snapshot.localStorage;
    const usedRatio = Math.min(usage.usedBytes / usage.quotaBytes, 1);
    const usageText = tr('{used} of {quota} used')
        .replace('{used}', formatBytes(usage.usedBytes))
        .replace('{quota}', formatBytes(usage.quotaBytes));

    const selectedCount = selectedKeys.size + selectedDatabases.size;
    const selectedBytes = usage.entries.reduce(
        (sum, entry) => selectedKeys.has(entry.key) ? sum + entry.bytes : sum,
        0,
    );
    const selectionText = selectedCount === 0
        ? tr('Please select the items to delete.')
        : (selectedBytes > 0
            ? tr('{count} selected, freeing {size} of local storage')
            : tr('{count} selected'))
            .replace('{count}', String(selectedCount))
            .replace('{size}', formatBytes(selectedBytes));

    async function cleanUp(): Promise<void> {
        setBusy(true);
        try {
            await actions.cleanUp({
                localStorageKeys: usage.entries.filter(entry => selectedKeys.has(entry.key)).map(entry => entry.key),
                databases: snapshot.databases.filter(name => selectedDatabases.has(name)),
            });
        } finally {
            setBusy(false);
        }
    }

    return (
        <div className="tt-web-storage">
            <header className="tt-web-storage-header">
                <b>{tr('Web Storage')}</b>
                <small>{tr('Data that extensions or character cards keep on this device.')}</small>
            </header>

            <StorageGroup title={tr('Local Storage')} technicalName="localStorage">
                <div className="tt-web-storage-usage">
                    <span>{usageText}</span>
                    <div
                        className={`tt-web-storage-meter${usage.nearlyFull ? ' tt-web-storage-meter-full' : ''}`}
                        role="meter"
                        aria-label={tr('Local Storage')}
                        aria-valuemin={0}
                        aria-valuemax={usage.quotaBytes}
                        aria-valuenow={usage.usedBytes}
                        aria-valuetext={usageText}
                    >
                        <span style={{ width: `${usedRatio * 100}%` }}></span>
                    </div>
                    {usage.nearlyFull && (
                        <small className="tt-web-storage-warning">
                            {tr('Storage is nearly used up. Extensions may fail to save data.')}
                        </small>
                    )}
                </div>
                <SelectableList
                    items={usage.entries.map(entry => ({ id: entry.key, label: entry.key, size: formatBytes(entry.bytes) }))}
                    selected={selectedKeys}
                    tr={tr}
                    onChange={setSelectedKeys}
                />
            </StorageGroup>

            <StorageGroup title={tr('IndexedDB')} technicalName="IndexedDB">
                <SelectableList
                    items={snapshot.databases.map(name => ({ id: name, label: name }))}
                    selected={selectedDatabases}
                    tr={tr}
                    onChange={setSelectedDatabases}
                />
            </StorageGroup>

            <AuthorRequest copyText={actions.copyText} tr={tr} />

            <footer className="tt-web-storage-footer">
                <span>{selectionText}</span>
                <button
                    type="button"
                    className="menu_button menu_button_icon tt-web-storage-delete"
                    disabled={selectedCount === 0 || busy}
                    onClick={() => void cleanUp()}
                >
                    <i className="fa-solid fa-trash-can" aria-hidden="true"></i>
                    <span>{tr('Delete Selected')}</span>
                </button>
            </footer>
        </div>
    );
}

function validateWebStorageBoundary(options: unknown): asserts options is WebStorageMountOptions {
    const value = options as Partial<WebStorageMountOptions> | null;
    if (!value?.snapshot?.localStorage || !Array.isArray(value.snapshot.databases)) {
        throw new Error('TauriTavern web storage snapshot is required');
    }
    if (typeof value.tr !== 'function') {
        throw new Error('TauriTavern web storage translator is required');
    }
    for (const name of ['cleanUp', 'copyText'] as const) {
        if (typeof value.actions?.[name] !== 'function') {
            throw new Error(`TauriTavern web storage action is unavailable: ${name}`);
        }
    }
}

export function mountTauriTavernWebStorageApp(mount: unknown, options: unknown): WebStorageHandle {
    if (!(mount instanceof HTMLElement)) {
        throw new Error('TauriTavern web storage mount element is required');
    }
    validateWebStorageBoundary(options);

    const root = createRoot(mount);
    root.render(
        <StrictMode>
            <WebStorageApp {...options} />
        </StrictMode>,
    );
    return { unmount: () => root.unmount() };
}
