import { callGenericPopup, POPUP_TYPE } from '../../popup.js';
import { t, translate } from '../../i18n.js';
import { openFullscreenTextViewer } from './text-viewer-popup.js';
import { trimFrontendLogEntriesInPlace } from '../../../tauri/main/services/dev-logging/frontend-log-retention.js';

const MONOSPACE_FONT_FAMILY = 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace';

const LIVE_LOG_PANEL_BUFFER_LIMIT = 800;
const LIVE_LOG_PANEL_DEFAULT_WINDOW_SIZE = 300;
const LIVE_LOG_PANEL_WINDOW_GROW_STEP = 200;
const LIVE_LOG_PANEL_MAX_WINDOW_SIZE = 500;

function getDevApi() {
    const api = window.__TAURITAVERN__?.api?.dev;
    if (!api) {
        throw new Error('TauriTavern host dev API is unavailable');
    }
    return api;
}

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

function createExpandableTextSection({
    title,
    viewerTitle = title,
    rows,
    placeholder,
    beforeExpand = null,
    viewerWrap = 'soft',
}) {
    const section = document.createElement('div');
    section.className = 'flex-container flexFlowColumn';
    section.style.gap = '6px';

    const header = document.createElement('div');
    header.className = 'flex-container alignItemsCenter';
    header.style.justifyContent = 'space-between';
    header.style.gap = '8px';

    const titleEl = document.createElement('span');
    titleEl.textContent = title;
    titleEl.style.opacity = '0.85';
    header.appendChild(titleEl);

    const expandButton = document.createElement('div');
    expandButton.className = 'menu_button menu_button_icon';
    expandButton.title = translate('Expand view');
    expandButton.setAttribute('aria-label', expandButton.title);

    const expandIcon = document.createElement('i');
    expandIcon.className = 'fa-solid fa-expand';
    expandButton.appendChild(expandIcon);
    header.appendChild(expandButton);

    const textarea = document.createElement('textarea');
    textarea.rows = rows;
    textarea.readOnly = true;
    textarea.spellcheck = false;
    textarea.style.width = '100%';
    textarea.style.resize = 'vertical';
    textarea.style.fontFamily = MONOSPACE_FONT_FAMILY;
    textarea.placeholder = placeholder;

    expandButton.addEventListener('click', () => runOrPopup(async () => {
        if (beforeExpand) {
            await beforeExpand();
        }

        await openFullscreenTextViewer({
            title: viewerTitle,
            text: textarea.value,
            wrap: viewerWrap,
        });
    }));

    section.appendChild(header);
    section.appendChild(textarea);

    return { section, textarea };
}

async function openLiveLogPanel({
    title,
    initialEntries,
    subscribe,
    extraControls = [],
    getTarget = () => null,
    trimEntriesInPlace = null,
}) {
    let filter = 'ALL';
    let paused = false;
    let windowSize = LIVE_LOG_PANEL_DEFAULT_WINDOW_SIZE;

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

    const tailButton = document.createElement('div');
    tailButton.className = 'menu_button';
    tailButton.textContent = translate('Jump to tail');
    header.appendChild(tailButton);

    const moreButton = document.createElement('div');
    moreButton.className = 'menu_button';
    moreButton.textContent = translate('More');
    header.appendChild(moreButton);

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
            renderTail();
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
    logContainer.style.fontFamily = MONOSPACE_FONT_FAMILY;
    logContainer.style.fontSize = '12px';
    root.appendChild(logContainer);

    /** @type {any[]} */
    let entries = initialEntries.slice();
    /** @type {any[]} */
    let renderedEntries = [];
    let wasNearBottom = true;

    const trimEntries = () => {
        if (trimEntriesInPlace) {
            trimEntriesInPlace(entries);
            return;
        }
        if (entries.length > LIVE_LOG_PANEL_BUFFER_LIMIT) {
            entries.splice(0, entries.length - LIVE_LOG_PANEL_BUFFER_LIMIT);
        }
    };

    const updateWindowControls = () => {
        const canGrow = windowSize < LIVE_LOG_PANEL_MAX_WINDOW_SIZE;
        moreButton.style.opacity = canGrow ? '1' : '0.55';
        moreButton.style.pointerEvents = canGrow ? 'auto' : 'none';
    };

    const renderTail = () => {
        trimEntries();
        windowSize = Math.min(windowSize, LIVE_LOG_PANEL_MAX_WINDOW_SIZE);

        const nextRendered = entries
            .filter((entry) => entryMatchesLevel(entry, filter))
            .slice(-windowSize);

        renderedEntries = nextRendered;

        logContainer.textContent = '';
        const fragment = document.createDocumentFragment();
        for (const entry of nextRendered) {
            fragment.appendChild(buildLogRow(entry, getTarget));
        }
        logContainer.appendChild(fragment);
        wasNearBottom = true;
        logContainer.scrollTop = logContainer.scrollHeight;
    };

    updateWindowControls();
    renderTail();

    const unsubscribe = await subscribe((entry) => {
        entries.push(entry);
        trimEntries();

        if (paused) {
            return;
        }

        const shouldFollow = isNearBottom(logContainer);
        wasNearBottom = shouldFollow;
        if (!shouldFollow) {
            return;
        }

        if (!entryMatchesLevel(entry, filter)) {
            return;
        }

        renderedEntries.push(entry);
        logContainer.appendChild(buildLogRow(entry, getTarget));

        while (renderedEntries.length > windowSize) {
            renderedEntries.shift();
            logContainer.firstChild?.remove();
        }

        logContainer.scrollTop = logContainer.scrollHeight;
    });

    pauseToggle.addEventListener('change', () => {
        paused = pauseToggle.checked;
        if (!paused) {
            renderTail();
        }
    });

    logContainer.addEventListener('scroll', () => {
        const nearBottom = isNearBottom(logContainer);
        if (!paused && nearBottom && !wasNearBottom) {
            renderTail();
            return;
        }

        wasNearBottom = nearBottom;
    });

    tailButton.addEventListener('click', () => renderTail());

    moreButton.addEventListener('click', () => {
        windowSize = Math.min(windowSize + LIVE_LOG_PANEL_WINDOW_GROW_STEP, LIVE_LOG_PANEL_MAX_WINDOW_SIZE);
        updateWindowControls();
        renderTail();
    });

    copyButton.addEventListener('click', async () => {
        const text = renderedEntries
            .map((entry) => formatEntryLine(entry, getTarget))
            .join('\n');
        await navigator.clipboard.writeText(text);
    });

    clearButton.addEventListener('click', () => {
        entries = [];
        renderedEntries = [];
        logContainer.textContent = '';
    });

    try {
        await callGenericPopup(root, POPUP_TYPE.TEXT, '', {
            okButton: translate('Close'),
            allowVerticalScrolling: true,
            wide: true,
            large: true,
            onClose: () => {},
        });
    } finally {
        void unsubscribe();
    }
}

export async function openFrontendLogsPanel() {
    const devApi = getDevApi();
    const currentCapture = await devApi.frontendLogs.getConsoleCaptureEnabled();

    const captureLabel = document.createElement('label');
    captureLabel.className = 'flex-container alignItemsCenter';
    captureLabel.style.gap = '6px';
    captureLabel.style.marginLeft = 'auto';

    const captureToggle = document.createElement('input');
    captureToggle.type = 'checkbox';
    captureToggle.style.margin = '0';
    captureToggle.checked = currentCapture;

    const captureText = document.createElement('span');
    captureText.textContent = translate('Capture full console logs');

    captureLabel.appendChild(captureToggle);
    captureLabel.appendChild(captureText);

    captureToggle.addEventListener('change', () => runOrPopup(async () => {
        await devApi.frontendLogs.setConsoleCaptureEnabled(captureToggle.checked);
    }));

    const initial = await devApi.frontendLogs.list();
    await openLiveLogPanel({
        title: t`Frontend Logs`,
        initialEntries: initial,
        subscribe: (handler) => devApi.frontendLogs.subscribe(handler),
        extraControls: [captureLabel],
        getTarget: (entry) => entry.target,
        trimEntriesInPlace: trimFrontendLogEntriesInPlace,
    });
}

export async function openBackendLogsPanel() {
    const devApi = getDevApi();
    const initial = await devApi.backendLogs.tail({ limit: 800 });

    await openLiveLogPanel({
        title: t`Backend Logs`,
        initialEntries: initial,
        subscribe: (handler) => devApi.backendLogs.subscribe(handler),
        getTarget: (entry) => entry.target,
    });
}

export async function openLlmApiLogsPanel() {
    const devApi = getDevApi();
    let keep = await devApi.llmApiLogs.getKeep();
    let indexEntries = await devApi.llmApiLogs.index({ limit: keep });

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

    const requestReadableSection = createExpandableTextSection({
        title: translate('Request body'),
        rows: 10,
        placeholder: translate('Request body'),
        beforeExpand: async () => {
            await ensurePreviewLoaded();
        },
    });
    const requestReadableBox = requestReadableSection.textarea;
    root.appendChild(requestReadableSection.section);

    const responseReadableSection = createExpandableTextSection({
        title: translate('Response body'),
        rows: 14,
        placeholder: translate('Response body'),
        beforeExpand: async () => {
            await ensurePreviewLoaded();
        },
    });
    const responseReadableBox = responseReadableSection.textarea;
    root.appendChild(responseReadableSection.section);

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

    const rawSections = document.createElement('div');
    rawSections.className = 'flex-container flexFlowColumn';
    rawSections.style.gap = '10px';
    rawSections.style.marginTop = '10px';

    const requestRawSection = createExpandableTextSection({
        title: translate('Request body'),
        viewerTitle: `${translate('Raw JSON/SSE')} - ${translate('Request body')}`,
        rows: 10,
        placeholder: translate('Request body'),
        beforeExpand: async () => {
            await ensureRawLoaded();
        },
        viewerWrap: 'off',
    });
    const requestRawBox = requestRawSection.textarea;
    rawSections.appendChild(requestRawSection.section);

    const responseRawSection = createExpandableTextSection({
        title: translate('Response body'),
        viewerTitle: `${translate('Raw JSON/SSE')} - ${translate('Response body')}`,
        rows: 14,
        placeholder: translate('Response body'),
        beforeExpand: async () => {
            await ensureRawLoaded();
        },
        viewerWrap: 'off',
    });
    const responseRawBox = responseRawSection.textarea;
    rawSections.appendChild(responseRawSection.section);

    rawDetails.appendChild(rawSections);

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
            currentPreview = await devApi.llmApiLogs.getPreview(id);
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
            currentRaw = await devApi.llmApiLogs.getRaw(id);
        } catch (error) {
            currentRaw = {
                id,
                error: String(error),
            };
        }
    };

    const hasCurrentPreview = () => Boolean(currentPreview && currentPreview.id === currentId);
    const hasCurrentRaw = () => Boolean(currentRaw && currentRaw.id === currentId);

    const ensurePreviewLoaded = async () => {
        if (!currentId || hasCurrentPreview()) {
            return;
        }

        await loadPreview(currentId);
        render();
    };

    const ensureRawLoaded = async () => {
        if (!currentId || hasCurrentRaw()) {
            return;
        }

        await loadRaw(currentId);
        render();
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

    copyRequestButton.addEventListener('click', () => runOrPopup(async () => {
        await ensurePreviewLoaded();
        await navigator.clipboard.writeText(requestReadableBox.value);
    }));
    copyResponseButton.addEventListener('click', () => runOrPopup(async () => {
        await ensurePreviewLoaded();
        await navigator.clipboard.writeText(responseReadableBox.value);
    }));
    copyRawRequestButton.addEventListener('click', () => runOrPopup(async () => {
        await ensureRawLoaded();
        await navigator.clipboard.writeText(requestRawBox.value);
    }));
    copyRawResponseButton.addEventListener('click', () => runOrPopup(async () => {
        await ensureRawLoaded();
        await navigator.clipboard.writeText(responseRawBox.value);
    }));

    rawDetails.addEventListener('toggle', () => runOrPopup(async () => {
        if (!rawDetails.open) {
            currentRaw = null;
            render();
            return;
        }

        if (!currentId) {
            return;
        }
        await ensureRawLoaded();
    }));

    const unsubscribe = await devApi.llmApiLogs.subscribeIndex((payload) => {
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

            await devApi.llmApiLogs.setKeep(keep);

            indexEntries = await devApi.llmApiLogs.index({ limit: keep });
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
        void unsubscribe();
    }
}
