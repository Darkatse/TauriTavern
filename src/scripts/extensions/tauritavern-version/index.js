import { DOMPurify } from '../../../lib.js';
import { CLIENT_VERSION, converter, displayVersion, eventSource, event_types, reloadMarkdownProcessor } from '../../../script.js';
import {
    checkForUpdate,
    getClientVersion as getBridgeClientVersion,
    getTauriTavernSettings,
    openExternalUrl,
    updateTauriTavernSettings,
} from '../../../tauri-bridge.js';
import { renderExtensionTemplateAsync } from '../../extensions.js';
import { POPUP_RESULT, POPUP_TYPE, Popup } from '../../popup.js';

const MODULE_NAME = 'tauritavern-version';
const LINKS = Object.freeze({
    authorName: 'Darkatse',
    repositoryUrl: 'https://github.com/Darkatse/TauriTavern',
    discordUrl: 'https://discord.com/channels/1134557553011998840/1472415443078742188',
});

const COPY_SUCCESS_TEXT = '\u7248\u672c\u4fe1\u606f\u5df2\u590d\u5236\u5230\u526a\u8d34\u677f';
const COPY_FAILURE_TEXT = '\u590d\u5236\u5931\u8d25\uff0c\u8bf7\u624b\u52a8\u590d\u5236\u7248\u672c\u4fe1\u606f';
const UNKNOWN_VALUE = 'UNKNOWN';
const UPDATE_CHECKING_TEXT = '\u68c0\u67e5\u4e2d...';
const UPDATE_NO_UPDATE_TEXT = '\u5f53\u524d\u5df2\u662f\u6700\u65b0\u7248\u672c';
const OPEN_LINK_FAILURE_PREFIX = '\u6253\u5f00\u94fe\u63a5\u5931\u8d25\uff1a';

let latestUpdateResult = null;
let startupUpdateCheckPromise = null;
let startupUpdatePopupShown = false;
let tauriTavernSettingsCache = null;
let tauriTavernSettingsPromise = null;

function extractCompatVersion(agent) {
    const segments = String(agent || '')
        .split(':')
        .map(segment => segment.trim())
        .filter(Boolean);

    return segments.length >= 2 ? segments[1] : UNKNOWN_VALUE;
}

function getFallbackVersion() {
    const normalized = String(displayVersion || '')
        .replace(/^TauriTavern\s*/i, '')
        .trim();

    return normalized || UNKNOWN_VALUE;
}

function getAndroidSystemInfo() {
    const userAgent = String(globalThis?.navigator?.userAgent || '');
    if (!/\bAndroid\b/i.test(userAgent)) {
        return null;
    }

    const androidVersionMatch = userAgent.match(/\bAndroid\s+([0-9.]+)/i);
    const androidVersion = androidVersionMatch?.[1] || UNKNOWN_VALUE;

    const modelFromBuild = userAgent.match(/\bAndroid\s+[0-9.]+;\s*([^;]+?)\s+Build\//i);
    const modelFallback = userAgent.match(/\bAndroid\s+[0-9.]+;\s*([^;]+)/i);
    const model = (modelFromBuild?.[1] || modelFallback?.[1] || UNKNOWN_VALUE).trim();

    const webViewVersionMatch = userAgent.match(/\bChrome\/([0-9.]+)/i);
    const webViewVersion = webViewVersionMatch?.[1] || UNKNOWN_VALUE;

    return {
        androidVersion,
        model,
        webViewVersion,
    };
}

function buildVersionInfo(payload = null) {
    const agent = typeof payload?.agent === 'string' && payload.agent.trim()
        ? payload.agent.trim()
        : (String(CLIENT_VERSION || '').trim() || 'SillyTavern:UNKNOWN:TauriTavern');

    const packageVersion = typeof payload?.tauriVersion === 'string' && payload.tauriVersion.trim()
        ? payload.tauriVersion.trim()
        : (typeof payload?.pkgVersion === 'string' && payload.pkgVersion.trim()
            ? payload.pkgVersion.trim()
            : getFallbackVersion());

    const gitBranch = typeof payload?.gitBranch === 'string' ? payload.gitBranch.trim() : '';
    const gitRevision = typeof payload?.gitRevision === 'string' ? payload.gitRevision.trim() : '';
    const gitInfo = gitBranch && gitRevision
        ? `${gitBranch} (${gitRevision})`
        : (gitBranch || gitRevision || 'N/A');

    const compatVersion = extractCompatVersion(agent);
    const compatBaseline = `SillyTavern ${compatVersion}`;
    const summaryParts = [
        `TauriTavern ${packageVersion}`,
        `Compat ${compatBaseline}`,
        `Git ${gitInfo}`,
    ];

    const androidInfo = getAndroidSystemInfo();
    if (androidInfo) {
        summaryParts.push(`Android ${androidInfo.androidVersion}`);
        summaryParts.push(`Model ${androidInfo.model}`);
        summaryParts.push(`WebView ${androidInfo.webViewVersion}`);
    }

    return {
        packageVersion,
        compatBaseline,
        gitInfo,
        summary: summaryParts.join(' | '),
    };
}

async function resolveVersionInfo() {
    try {
        const payload = await getBridgeClientVersion();
        return buildVersionInfo(payload);
    } catch (error) {
        console.warn('TauriTavern version extension fallback:', error);
        return buildVersionInfo();
    }
}

function renderVersionInfo(info) {
    $('#tauritavern_version_number').text(info.packageVersion);
    $('#tauritavern_compat_version').text(info.compatBaseline);
    $('#tauritavern_git_info').text(info.gitInfo);
    $('#tauritavern_version_copy').data('summary', info.summary);
}

async function onCopyVersionClick() {
    const summary = String($('#tauritavern_version_copy').data('summary') || '').trim();
    if (!summary) {
        return;
    }

    const clipboard = globalThis?.navigator?.clipboard;
    if (!clipboard || typeof clipboard.writeText !== 'function') {
        globalThis.toastr?.warning?.(COPY_FAILURE_TEXT);
        return;
    }

    try {
        await clipboard.writeText(summary);
        globalThis.toastr?.success?.(COPY_SUCCESS_TEXT);
    } catch {
        globalThis.toastr?.error?.(COPY_FAILURE_TEXT);
    }
}

async function openVersionUrl(url) {
    try {
        await openExternalUrl(url);
    } catch (error) {
        globalThis.toastr?.error?.(`${OPEN_LINK_FAILURE_PREFIX}${error}`);
        throw error;
    }
}

function shouldInterceptExternalLink(event) {
    return event.button === 0
        && !event.metaKey
        && !event.ctrlKey
        && !event.shiftKey
        && !event.altKey;
}

function onExternalLinkClick(event) {
    if (!shouldInterceptExternalLink(event)) {
        return;
    }

    const href = String(event.currentTarget?.href || '').trim();
    if (!href) {
        return;
    }

    event.preventDefault();
    void openVersionUrl(href);
}

function ensureMarkdownConverter() {
    return converter || reloadMarkdownProcessor();
}

function renderChangelogHtml(markdown) {
    const normalized = String(markdown || '').trim();
    if (!normalized) {
        return '<p>\u65e0\u53d8\u66f4\u65e5\u5fd7</p>';
    }

    const html = ensureMarkdownConverter().makeHtml(normalized);
    return DOMPurify.sanitize(html);
}

function showUpdateResult(result) {
    const release = result?.latest_release;
    if (!release) {
        return;
    }

    latestUpdateResult = result;

    const $result = $('#tauritavern_update_result');
    if (!$result.length) {
        return;
    }

    $('#tauritavern_update_version').text(release.version || release.tag_name || UNKNOWN_VALUE);
    $('#tauritavern_update_changelog').html(renderChangelogHtml(release.body));
    $('#tauritavern_update_download').attr('href', release.html_url);

    if ($result.is(':hidden')) {
        $result.slideDown(200);
    }
}

function hideUpdateResult() {
    const $result = $('#tauritavern_update_result');
    if ($result.length && $result.is(':visible')) {
        $result.slideUp(200);
    }
}

async function onCheckUpdateClick() {
    const $btn = $('#tauritavern_check_update');
    const $icon = $btn.find('i');
    const $text = $btn.find('span');
    const defaultText = String($text.data('defaultLabel') || $text.text()).trim();

    $text.data('defaultLabel', defaultText);
    $icon.addClass('fa-spin');
    $text.text(UPDATE_CHECKING_TEXT);
    $btn.prop('disabled', true);

    try {
        const result = await checkForUpdate();
        if (result?.has_update && result?.latest_release) {
            showUpdateResult(result);
        } else {
            latestUpdateResult = null;
            globalThis.toastr?.info?.(UPDATE_NO_UPDATE_TEXT);
            hideUpdateResult();
        }
    } catch (error) {
        globalThis.toastr?.error?.(`\u68c0\u67e5\u66f4\u65b0\u5931\u8d25\uff1a${error}`);
    } finally {
        $icon.removeClass('fa-spin');
        $text.text(defaultText);
        $btn.prop('disabled', false);
    }
}

async function getTauriTavernSettingsState() {
    if (tauriTavernSettingsCache) {
        return tauriTavernSettingsCache;
    }

    if (!tauriTavernSettingsPromise) {
        tauriTavernSettingsPromise = getTauriTavernSettings()
            .then((settings) => {
                tauriTavernSettingsCache = settings;
                return settings;
            })
            .finally(() => {
                tauriTavernSettingsPromise = null;
            });
    }

    return tauriTavernSettingsPromise;
}

function getStartupUpdatePopupToken(result) {
    const releaseVersion = String(result?.latest_release?.version || result?.latest_release?.tag_name || '').trim();
    const currentVersion = String(result?.current_version || '').trim();

    return releaseVersion ? `${currentVersion}->${releaseVersion}` : '';
}

async function hasSeenStartupUpdate(result) {
    const token = getStartupUpdatePopupToken(result);
    if (token === '') {
        return false;
    }

    const settings = await getTauriTavernSettingsState();
    return settings.updates.startup_popup.dismissed_release_token === token;
}

async function rememberStartupUpdate(result) {
    const token = getStartupUpdatePopupToken(result);
    if (token === '') {
        return;
    }

    tauriTavernSettingsCache = await updateTauriTavernSettings({
        updates: {
            startup_popup: {
                dismissed_release_token: token,
            },
        },
    });
}

function buildStartupUpdatePopupContent(result) {
    const release = result.latest_release;
    const root = document.createElement('div');
    root.className = 'ttv-update-popup';

    const header = document.createElement('div');
    header.className = 'ttv-update-popup-header';

    const title = document.createElement('h3');
    title.className = 'ttv-update-popup-title';
    title.textContent = `\u53d1\u73b0\u65b0\u7248 TauriTavern ${release.version}`;
    header.appendChild(title);

    const meta = document.createElement('p');
    meta.className = 'ttv-update-popup-meta';
    meta.textContent = `TauriTavern ${result.current_version}  \u2192  ${release.version}`;
    header.appendChild(meta);

    const note = document.createElement('p');
    note.className = 'ttv-update-popup-note';
    note.textContent = '\u53ef\u4ee5\u7a0d\u540e\u518d\u66f4\u65b0\uff0c\u624b\u52a8\u68c0\u67e5\u5165\u53e3\u4ecd\u7136\u4f1a\u4fdd\u7559\u3002';
    header.appendChild(note);

    root.appendChild(header);

    const body = document.createElement('div');
    body.className = 'ttv-update-popup-body';
    body.innerHTML = renderChangelogHtml(release.body);
    root.appendChild(body);

    return root;
}

async function showStartupUpdatePopup(result) {
    const popup = new Popup(buildStartupUpdatePopupContent(result), POPUP_TYPE.CONFIRM, '', {
        okButton: '\u524d\u5f80\u4e0b\u8f7d',
        cancelButton: '\u7a0d\u540e',
        allowVerticalScrolling: true,
        wide: true,
        wider: true,
    });

    const popupResult = await popup.show();
    await rememberStartupUpdate(result);

    if (popupResult === POPUP_RESULT.AFFIRMATIVE) {
        await openVersionUrl(result.latest_release.html_url);
    }
}

async function runStartupUpdateCheck() {
    if (startupUpdateCheckPromise) {
        return startupUpdateCheckPromise;
    }

    startupUpdateCheckPromise = (async () => {
        let result;

        try {
            result = await checkForUpdate();
        } catch (error) {
            console.warn('Startup update check failed:', error);
            return;
        }

        if (!result?.has_update || !result?.latest_release) {
            return;
        }

        showUpdateResult(result);

        if (startupUpdatePopupShown || await hasSeenStartupUpdate(result)) {
            return;
        }

        startupUpdatePopupShown = true;
        await showStartupUpdatePopup(result);
    })();

    try {
        await startupUpdateCheckPromise;
    } finally {
        startupUpdateCheckPromise = null;
    }
}

eventSource.once(event_types.APP_READY, () => {
    void runStartupUpdateCheck();
});

jQuery(async () => {
    const container = $('#tauritavern_version_container');
    if (!container.length) {
        return;
    }

    const html = await renderExtensionTemplateAsync(MODULE_NAME, 'settings', LINKS);
    container.append(html);
    $('#tauritavern_version_copy').on('click', onCopyVersionClick);
    $('#tauritavern_check_update').on('click', onCheckUpdateClick);
    $('#tauritavern_update_dismiss').on('click', hideUpdateResult);
    container.on('click', 'a[target="_blank"]', onExternalLinkClick);

    const versionInfo = await resolveVersionInfo();
    renderVersionInfo(versionInfo);

    if (latestUpdateResult?.has_update && latestUpdateResult?.latest_release) {
        showUpdateResult(latestUpdateResult);
    }
});
