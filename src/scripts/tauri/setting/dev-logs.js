import { callGenericPopup, POPUP_TYPE } from '../../popup.js';
import { t, translate } from '../../i18n.js';
import { getTauriTavernSettings, invoke, listen, updateTauriTavernSettings } from '../../../tauri-bridge.js';
import {
    getFrontendLogEntries,
    isFrontendConsoleCaptureEnabled,
    subscribeFrontendLogs,
    setFrontendConsoleCaptureEnabled,
} from '../../../tauri/main/services/dev-logging/frontend-log-capture.js';

const CONSOLE_CAPTURE_STORAGE_KEY = 'tt:devConsoleCapture';

function formatTimestamp(ms) {
    const date = new Date(Number(ms) || 0);
    if (Number.isNaN(date.getTime())) {
        return 'Invalid time';
    }
    return date.toLocaleString();
}

function normalizeLevel(level) {
    const value = String(level || '').trim().toUpperCase();
    if (!value) {
        return 'INFO';
    }
    return value === 'WARNING' ? 'WARN' : value;
}

function entryMatchesLevel(entry, filter) {
    if (!filter || filter === 'ALL') {
        return true;
    }
    return normalizeLevel(entry.level) === filter;
}

function levelColor(level) {
    switch (level) {
        case 'ERROR':
            return 'var(--fullred)';
        case 'WARN':
            return 'var(--golden)';
        case 'INFO':
            return 'var(--SmartThemeUnderlineColor)';
        case 'DEBUG':
            return 'var(--grey70)';
        default:
            return 'var(--SmartThemeEmColor)';
    }
}

function buildLogRow(entry, getTarget) {
    const level = normalizeLevel(entry.level);
    const target = getTarget(entry);

    const row = document.createElement('div');
    row.style.display = 'flex';
    row.style.flexDirection = 'column';
    row.style.gap = '2px';
    row.style.padding = '6px 8px';
    row.style.borderBottom = '1px solid rgba(255,255,255,0.06)';
    row.style.borderLeft = `3px solid ${levelColor(level)}`;

    const prefixLine = document.createElement('div');
    prefixLine.style.display = 'flex';
    prefixLine.style.alignItems = 'baseline';
    prefixLine.style.gap = '8px';
    prefixLine.style.flexWrap = 'wrap';

    const time = document.createElement('span');
    time.textContent = formatTimestamp(entry.timestampMs);
    time.style.opacity = '0.75';
    time.style.whiteSpace = 'nowrap';
    time.style.fontVariantNumeric = 'tabular-nums';

    const badge = document.createElement('span');
    badge.textContent = level;
    badge.style.fontWeight = '700';
    badge.style.fontSize = '11px';
    badge.style.padding = '1px 6px';
    badge.style.borderRadius = '999px';
    badge.style.border = `1px solid ${levelColor(level)}`;
    badge.style.color = levelColor(level);
    badge.style.whiteSpace = 'nowrap';
    badge.style.userSelect = 'none';

    prefixLine.appendChild(time);
    prefixLine.appendChild(badge);

    if (target) {
        const targetEl = document.createElement('span');
        targetEl.textContent = target;
        targetEl.style.opacity = '0.75';
        targetEl.style.wordBreak = 'break-word';
        targetEl.style.overflowWrap = 'anywhere';
        prefixLine.appendChild(targetEl);
    }

    const message = document.createElement('span');
    message.textContent = entry.message;
    message.style.whiteSpace = 'pre-wrap';
    message.style.wordBreak = 'break-word';
    message.style.overflowWrap = 'anywhere';

    row.appendChild(prefixLine);
    row.appendChild(message);

    return row;
}

function isNearBottom(container) {
    return container.scrollHeight - container.scrollTop - container.clientHeight < 24;
}

function formatEntryLine(entry, getTarget) {
    const target = getTarget(entry);
    const targetSuffix = target ? ` [${target}]` : '';
    return `[${formatTimestamp(entry.timestampMs)}] [${normalizeLevel(entry.level)}]${targetSuffix} ${entry.message}`;
}

function runOrPopup(task) {
    void (async () => {
        try {
            await task();
        } catch (error) {
            const message = error?.message ? String(error.message) : String(error);
            await callGenericPopup(translate(message), POPUP_TYPE.TEXT, '', {
                okButton: translate('OK'),
                allowVerticalScrolling: true,
                wide: false,
                large: false,
            });
        }
    })();
}

async function openLiveLogPanel({
    title,
    initialEntries,
    subscribe,
    extraControls = [],
    getTarget = () => null,
}) {
    let filter = 'ALL';
    let paused = false;

    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.style.gap = '10px';

    const header = document.createElement('div');
    header.className = 'flex-container alignItemsCenter';
    header.style.gap = '10px';
    header.style.flexWrap = 'wrap';

    const titleEl = document.createElement('b');
    titleEl.textContent = translate(title);
    header.appendChild(titleEl);

    for (const el of extraControls) {
        header.appendChild(el);
    }

    const pauseLabel = document.createElement('label');
    pauseLabel.className = 'flex-container alignItemsCenter';
    pauseLabel.style.gap = '6px';
    const pauseToggle = document.createElement('input');
    pauseToggle.type = 'checkbox';
    pauseToggle.style.margin = '0';
    const pauseText = document.createElement('span');
    pauseText.textContent = translate('Pause');
    pauseLabel.appendChild(pauseToggle);
    pauseLabel.appendChild(pauseText);
    header.appendChild(pauseLabel);

    const copyButton = document.createElement('div');
    copyButton.className = 'menu_button';
    copyButton.textContent = translate('Copy');
    header.appendChild(copyButton);

    const clearButton = document.createElement('div');
    clearButton.className = 'menu_button';
    clearButton.textContent = translate('Clear');
    header.appendChild(clearButton);

    root.appendChild(header);

    const filterRow = document.createElement('div');
    filterRow.className = 'flex-container alignItemsCenter';
    filterRow.style.gap = '6px';
    filterRow.style.flexWrap = 'wrap';

    /** @type {Map<string, HTMLElement>} */
    const chipMap = new Map();

    const setFilter = (next) => {
        filter = next;
        for (const [level, el] of chipMap.entries()) {
            const active = level === filter;
            el.style.background = active ? 'rgba(255,255,255,0.10)' : 'transparent';
            el.style.borderColor = active ? 'rgba(255,255,255,0.24)' : 'rgba(255,255,255,0.10)';
        }
    };

    for (const option of ['ALL', 'DEBUG', 'INFO', 'WARN', 'ERROR']) {
        const chip = document.createElement('div');
        chip.textContent = option;
        chip.style.cursor = 'pointer';
        chip.style.userSelect = 'none';
        chip.style.padding = '4px 10px';
        chip.style.borderRadius = '999px';
        chip.style.border = '1px solid rgba(255,255,255,0.10)';
        chip.style.fontWeight = '700';
        chip.style.fontSize = '12px';
        chip.style.color = levelColor(option);
        chip.addEventListener('click', () => {
            setFilter(option);
            renderAll();
        });
        chipMap.set(option, chip);
        filterRow.appendChild(chip);
    }
    setFilter(filter);
    root.appendChild(filterRow);

    const logContainer = document.createElement('div');
    logContainer.style.height = '60vh';
    logContainer.style.overflow = 'auto';
    logContainer.style.padding = '8px';
    logContainer.style.border = '1px solid rgba(255,255,255,0.10)';
    logContainer.style.borderRadius = '10px';
    logContainer.style.background = 'rgba(0,0,0,0.12)';
    logContainer.style.fontFamily = 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace';
    logContainer.style.fontSize = '12px';
    root.appendChild(logContainer);

    /** @type {any[]} */
    let entries = initialEntries.slice();

    const renderAll = () => {
        logContainer.textContent = '';
        const fragment = document.createDocumentFragment();
        for (const entry of entries) {
            if (!entryMatchesLevel(entry, filter)) {
                continue;
            }
            fragment.appendChild(buildLogRow(entry, getTarget));
        }
        logContainer.appendChild(fragment);
        logContainer.scrollTop = logContainer.scrollHeight;
    };

    renderAll();

    const unsubscribe = subscribe((entry) => {
        entries.push(entry);
        if (paused || !entryMatchesLevel(entry, filter)) {
            return;
        }

        const shouldStick = isNearBottom(logContainer);
        logContainer.appendChild(buildLogRow(entry, getTarget));
        if (shouldStick) {
            logContainer.scrollTop = logContainer.scrollHeight;
        }
    });

    pauseToggle.addEventListener('change', () => {
        paused = pauseToggle.checked;
        if (!paused) {
            renderAll();
        }
    });

    copyButton.addEventListener('click', async () => {
        const text = entries
            .filter((entry) => entryMatchesLevel(entry, filter))
            .map((entry) => formatEntryLine(entry, getTarget))
            .join('\n');
        await navigator.clipboard.writeText(text);
    });

    clearButton.addEventListener('click', () => {
        entries = [];
        logContainer.textContent = '';
    });

    await callGenericPopup(root, POPUP_TYPE.TEXT, '', {
        okButton: translate('Close'),
        allowVerticalScrolling: true,
        wide: true,
        large: true,
        onClose: () => {
            unsubscribe();
        },
    });
}

export async function openFrontendLogsPanel() {
    const settings = await getTauriTavernSettings();
    const currentCapture = settings.dev.frontend_console_capture;

    const captureLabel = document.createElement('label');
    captureLabel.className = 'flex-container alignItemsCenter';
    captureLabel.style.gap = '6px';
    captureLabel.style.marginLeft = 'auto';

    const captureToggle = document.createElement('input');
    captureToggle.type = 'checkbox';
    captureToggle.style.margin = '0';
    captureToggle.checked = isFrontendConsoleCaptureEnabled();

    const captureText = document.createElement('span');
    captureText.textContent = translate('Capture full console logs');

    captureLabel.appendChild(captureToggle);
    captureLabel.appendChild(captureText);

    captureToggle.addEventListener('change', () => runOrPopup(async () => {
        const enabled = captureToggle.checked;
        localStorage.setItem(CONSOLE_CAPTURE_STORAGE_KEY, enabled ? '1' : '0');
        setFrontendConsoleCaptureEnabled(enabled);
        await updateTauriTavernSettings({
            dev: {
                frontend_console_capture: enabled,
            },
        });
    }));

    if (currentCapture !== captureToggle.checked) {
        localStorage.setItem(CONSOLE_CAPTURE_STORAGE_KEY, currentCapture ? '1' : '0');
        captureToggle.checked = currentCapture;
        setFrontendConsoleCaptureEnabled(currentCapture);
    }

    const initial = getFrontendLogEntries();
    await openLiveLogPanel({
        title: t`Frontend Logs`,
        initialEntries: initial,
        subscribe: subscribeFrontendLogs,
        extraControls: [captureLabel],
        getTarget: (entry) => entry.target,
    });
}

export async function openBackendLogsPanel() {
    await invoke('devlog_set_backend_log_stream_enabled', { enabled: true });
    const initial = await invoke('devlog_get_backend_log_tail', { limit: 800 });

    /** @type {Set<(entry: any) => void>} */
    const handlers = new Set();

    const unlisten = await listen('tauritavern-backend-log', (event) => {
        const entry = event?.payload;
        for (const handler of handlers) {
            handler(entry);
        }
    });

    try {
        await openLiveLogPanel({
            title: t`Backend Logs`,
            initialEntries: initial,
            subscribe: (handler) => {
                handlers.add(handler);
                return () => handlers.delete(handler);
            },
            getTarget: (entry) => entry.target,
        });
    } finally {
        await invoke('devlog_set_backend_log_stream_enabled', { enabled: false });
        await unlisten();
    }
}

export async function openLlmApiLogsPanel() {
    await invoke('devlog_set_llm_api_log_stream_enabled', { enabled: true });
    const settings = await getTauriTavernSettings();
    let keep = Math.max(1, settings.dev.llm_api_keep);
    let indexEntries = await invoke('devlog_get_llm_api_log_index', { limit: keep });

    let index = Math.max(0, indexEntries.length - 1);
    let currentId = indexEntries[index]?.id ?? 0;

    const root = document.createElement('div');
    root.className = 'flex-container flexFlowColumn';
    root.style.gap = '10px';

    const header = document.createElement('div');
    header.className = 'flex-container alignItemsCenter';
    header.style.gap = '10px';
    header.style.flexWrap = 'wrap';

    const titleEl = document.createElement('b');
    titleEl.textContent = translate('LLM API Logs');
    header.appendChild(titleEl);

    const prevButton = document.createElement('div');
    prevButton.className = 'menu_button';
    prevButton.textContent = translate('Prev');
    header.appendChild(prevButton);

    const nextButton = document.createElement('div');
    nextButton.className = 'menu_button';
    nextButton.textContent = translate('Next');
    header.appendChild(nextButton);

    const reloadButton = document.createElement('div');
    reloadButton.className = 'menu_button';
    reloadButton.textContent = translate('Reload');
    header.appendChild(reloadButton);

    const positionText = document.createElement('small');
    positionText.style.opacity = '0.85';
    header.appendChild(positionText);

    const copyRequestButton = document.createElement('div');
    copyRequestButton.className = 'menu_button';
    copyRequestButton.textContent = translate('Copy Request');
    header.appendChild(copyRequestButton);

    const copyResponseButton = document.createElement('div');
    copyResponseButton.className = 'menu_button';
    copyResponseButton.textContent = translate('Copy Response');
    header.appendChild(copyResponseButton);

    root.appendChild(header);

    const settingsRow = document.createElement('div');
    settingsRow.className = 'flex-container alignItemsCenter';
    settingsRow.style.gap = '10px';
    settingsRow.style.flexWrap = 'wrap';

    const keepLabel = document.createElement('span');
    keepLabel.textContent = translate('LLM API keep');
    settingsRow.appendChild(keepLabel);

    const keepInput = document.createElement('input');
    keepInput.className = 'text_pole';
    keepInput.type = 'number';
    keepInput.min = '1';
    keepInput.step = '1';
    keepInput.style.margin = '0';
    keepInput.style.width = 'auto';
    keepInput.style.minWidth = '120px';
    keepInput.value = String(keep);
    settingsRow.appendChild(keepInput);

    const applyKeepButton = document.createElement('div');
    applyKeepButton.className = 'menu_button';
    applyKeepButton.textContent = translate('Apply');
    settingsRow.appendChild(applyKeepButton);

    root.appendChild(settingsRow);

    const hint = document.createElement('small');
    hint.style.opacity = '0.85';
    hint.textContent = translate('LLM API logs capture prompt/response bodies.');
    root.appendChild(hint);

    const meta = document.createElement('div');
    meta.style.opacity = '0.92';
    meta.style.whiteSpace = 'pre-wrap';
    root.appendChild(meta);

    const requestReadableBox = document.createElement('textarea');
    requestReadableBox.rows = 10;
    requestReadableBox.readOnly = true;
    requestReadableBox.style.width = '100%';
    requestReadableBox.style.resize = 'vertical';
    requestReadableBox.style.fontFamily = 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace';
    requestReadableBox.placeholder = translate('Request body');
    root.appendChild(requestReadableBox);

    const responseReadableBox = document.createElement('textarea');
    responseReadableBox.rows = 14;
    responseReadableBox.readOnly = true;
    responseReadableBox.style.width = '100%';
    responseReadableBox.style.resize = 'vertical';
    responseReadableBox.style.fontFamily = requestReadableBox.style.fontFamily;
    responseReadableBox.placeholder = translate('Response body');
    root.appendChild(responseReadableBox);

    const rawDetails = document.createElement('details');
    rawDetails.style.border = '1px solid rgba(255,255,255,0.10)';
    rawDetails.style.borderRadius = '10px';
    rawDetails.style.padding = '8px';

    const rawSummary = document.createElement('summary');
    rawSummary.style.cursor = 'pointer';
    rawSummary.textContent = translate('Raw JSON/SSE');
    rawDetails.appendChild(rawSummary);

    const rawControls = document.createElement('div');
    rawControls.className = 'flex-container';
    rawControls.style.gap = '10px';
    rawControls.style.flexWrap = 'wrap';
    rawControls.style.marginTop = '10px';

    const copyRawRequestButton = document.createElement('div');
    copyRawRequestButton.className = 'menu_button';
    copyRawRequestButton.textContent = translate('Copy Raw Request');
    rawControls.appendChild(copyRawRequestButton);

    const copyRawResponseButton = document.createElement('div');
    copyRawResponseButton.className = 'menu_button';
    copyRawResponseButton.textContent = translate('Copy Raw Response');
    rawControls.appendChild(copyRawResponseButton);

    rawDetails.appendChild(rawControls);

    const requestRawBox = document.createElement('textarea');
    requestRawBox.rows = 10;
    requestRawBox.readOnly = true;
    requestRawBox.style.width = '100%';
    requestRawBox.style.resize = 'vertical';
    requestRawBox.style.fontFamily = requestReadableBox.style.fontFamily;
    requestRawBox.placeholder = translate('Request body');
    rawDetails.appendChild(requestRawBox);

    const responseRawBox = document.createElement('textarea');
    responseRawBox.rows = 14;
    responseRawBox.readOnly = true;
    responseRawBox.style.width = '100%';
    responseRawBox.style.resize = 'vertical';
    responseRawBox.style.fontFamily = requestReadableBox.style.fontFamily;
    responseRawBox.placeholder = translate('Response body');
    rawDetails.appendChild(responseRawBox);

    root.appendChild(rawDetails);

    /** @type {any | null} */
    let currentPreview = null;
    /** @type {any | null} */
    let currentRaw = null;

    const loadPreview = async (id) => {
        if (!id) {
            currentPreview = null;
            return;
        }
        currentPreview = null;
        try {
            currentPreview = await invoke('devlog_get_llm_api_log_preview', { id });
        } catch (error) {
            currentPreview = {
                id,
                error: String(error),
            };
        }
    };

    const loadRaw = async (id) => {
        if (!id) {
            currentRaw = null;
            return;
        }
        currentRaw = null;
        try {
            currentRaw = await invoke('devlog_get_llm_api_log_raw', { id });
        } catch (error) {
            currentRaw = {
                id,
                error: String(error),
            };
        }
    };

    const render = () => {
        if (!indexEntries.length) {
            index = 0;
            currentId = 0;
            positionText.textContent = translate('No entries');
            meta.textContent = '';
            requestReadableBox.value = '';
            responseReadableBox.value = '';
            requestRawBox.value = '';
            responseRawBox.value = '';
            return;
        }

        index = Math.max(0, Math.min(index, indexEntries.length - 1));
        const entry = indexEntries[index];
        currentId = entry?.id ?? 0;
        positionText.textContent = `${index + 1}/${indexEntries.length}`;

        const preview = currentPreview && currentPreview.id === currentId ? currentPreview : null;
        if (!preview) {
            meta.textContent = `${entry.source}${entry.model ? ` (${entry.model})` : ''}\n${entry.endpoint}\n${translate('Duration')}: ${entry.durationMs}ms    ok: ${entry.ok}\n${formatTimestamp(entry.timestampMs)}`;
            requestReadableBox.value = translate('Loading...');
            responseReadableBox.value = translate('Loading...');
        } else if (preview.error) {
            meta.textContent = String(preview.error);
            requestReadableBox.value = '';
            responseReadableBox.value = '';
        } else {
            meta.textContent = `${preview.source}${preview.model ? ` (${preview.model})` : ''}\n${preview.endpoint}\n${translate('Duration')}: ${preview.durationMs}ms    ok: ${preview.ok}\n${formatTimestamp(preview.timestampMs)}`;
            requestReadableBox.value = preview.requestReadable || '';
            responseReadableBox.value = preview.responseReadable || '';
        }

        const raw = currentRaw && currentRaw.id === currentId ? currentRaw : null;
        if (!rawDetails.open) {
            requestRawBox.value = '';
            responseRawBox.value = '';
        } else if (!raw) {
            requestRawBox.value = translate('Loading...');
            responseRawBox.value = translate('Loading...');
        } else if (raw.error) {
            requestRawBox.value = String(raw.error);
            responseRawBox.value = '';
        } else {
            requestRawBox.value = raw.requestRaw || '';
            responseRawBox.value = raw.responseRaw || '';
        }
    };

    const setCurrentIndex = async (next) => {
        if (!indexEntries.length) {
            return;
        }

        index = Math.max(0, Math.min(next, indexEntries.length - 1));
        currentRaw = null;
        await loadPreview(indexEntries[index]?.id ?? 0);
        render();
    };

    prevButton.addEventListener('click', () => runOrPopup(async () => setCurrentIndex(index - 1)));
    nextButton.addEventListener('click', () => runOrPopup(async () => setCurrentIndex(index + 1)));
    reloadButton.addEventListener('click', () => runOrPopup(async () => {
        if (!currentId) {
            return;
        }
        await loadPreview(currentId);
        if (rawDetails.open) {
            await loadRaw(currentId);
        }
        render();
    }));

    copyRequestButton.addEventListener('click', async () => {
        await navigator.clipboard.writeText(requestReadableBox.value);
    });
    copyResponseButton.addEventListener('click', async () => {
        await navigator.clipboard.writeText(responseReadableBox.value);
    });
    copyRawRequestButton.addEventListener('click', async () => {
        if (!currentId) {
            return;
        }
        if (!currentRaw || currentRaw.id !== currentId) {
            await loadRaw(currentId);
            render();
        }
        await navigator.clipboard.writeText(requestRawBox.value);
    });
    copyRawResponseButton.addEventListener('click', async () => {
        if (!currentId) {
            return;
        }
        if (!currentRaw || currentRaw.id !== currentId) {
            await loadRaw(currentId);
            render();
        }
        await navigator.clipboard.writeText(responseRawBox.value);
    });

    rawDetails.addEventListener('toggle', () => runOrPopup(async () => {
        if (!rawDetails.open) {
            currentRaw = null;
            render();
            return;
        }

        if (!currentId) {
            return;
        }
        await loadRaw(currentId);
        render();
    }));

    const unlisten = await listen('tauritavern-llm-api-log', (event) => {
        const payload = event.payload;
        const shouldFollowLatest = index >= indexEntries.length - 1;
        indexEntries.push(payload);
        if (indexEntries.length > keep) indexEntries.splice(0, indexEntries.length - keep);

        if (shouldFollowLatest) {
            index = Math.max(0, indexEntries.length - 1);
            void loadPreview(indexEntries[index]?.id ?? 0).then(() => render());
        } else {
            render();
        }
    });

    try {
        if (currentId) {
            await loadPreview(currentId);
        }
        render();

        applyKeepButton.addEventListener('click', () => runOrPopup(async () => {
            const nextKeepRaw = Number(keepInput.value);
            if (!Number.isFinite(nextKeepRaw) || nextKeepRaw <= 0) {
                throw new Error(translate('LLM API keep must be a positive number'));
            }

            keep = Math.floor(nextKeepRaw);
            keepInput.value = String(keep);

            await updateTauriTavernSettings({
                dev: {
                    llm_api_keep: keep,
                },
            });

            indexEntries = await invoke('devlog_get_llm_api_log_index', { limit: keep });
            index = Math.max(0, indexEntries.length - 1);
            currentId = indexEntries[index]?.id ?? 0;
            currentRaw = null;
            if (currentId) {
                await loadPreview(currentId);
            }
            render();
        }));

        await callGenericPopup(root, POPUP_TYPE.TEXT, '', {
            okButton: translate('Close'),
            allowVerticalScrolling: true,
            wide: true,
            large: true,
            onClose: () => {},
        });
    } finally {
        await invoke('devlog_set_llm_api_log_stream_enabled', { enabled: false });
        await unlisten();
    }
}
