import { DOMPurify } from '../../../lib.js';
import { CLIENT_VERSION, converter, displayVersion, reloadMarkdownProcessor } from '../../../script.js';
import { checkForUpdate, getClientVersion as getBridgeClientVersion, listen } from '../../../tauri-bridge.js';
import { renderExtensionTemplateAsync } from '../../extensions.js';
import { POPUP_RESULT, POPUP_TYPE, Popup } from '../../popup.js';

const MODULE_NAME = 'tauritavern-version';
const LINKS = Object.freeze({
    authorName: 'Darkatse',
    repositoryUrl: 'https://github.com/Darkatse/TauriTavern',
    discordUrl: 'https://discord.com/channels/1134557553011998840/1472415443078742188',
});

const COPY_SUCCESS_TEXT = '版本信息已复制到剪贴板';
const COPY_FAILURE_TEXT = '复制失败，请手动复制版本信息';
const UNKNOWN_VALUE = 'UNKNOWN';
const UPDATE_CHECKING_TEXT = '检查中…';
const UPDATE_NO_UPDATE_TEXT = '当前已是最新版本';

function extractCompatVersion(agent) {
    const segments = String(agent || '')
        .split(':')
        .map(segment => segment.trim())
        .filter(Boolean);

    return segments.length >= 2 ? segments[1] : 'UNKNOWN';
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

    const packageVersion = typeof payload?.pkgVersion === 'string' && payload.pkgVersion.trim()
        ? payload.pkgVersion.trim()
        : getFallbackVersion();

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

    const summary = summaryParts.join(' | ');

    return {
        packageVersion,
        compatBaseline,
        gitInfo,
        summary,
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
        if (globalThis.toastr?.warning) {
            globalThis.toastr.warning(COPY_FAILURE_TEXT);
        }
        return;
    }

    try {
        await clipboard.writeText(summary);
        if (globalThis.toastr?.success) {
            globalThis.toastr.success(COPY_SUCCESS_TEXT);
        }
    } catch {
        if (globalThis.toastr?.error) {
            globalThis.toastr.error(COPY_FAILURE_TEXT);
        }
    }
}

function ensureMarkdownConverter() {
    return converter || reloadMarkdownProcessor();
}

function renderChangelogHtml(markdown) {
    const normalized = String(markdown || '').trim();
    if (!normalized) {
        return '<p>无变更日志</p>';
    }

    const html = ensureMarkdownConverter().makeHtml(normalized);
    return DOMPurify.sanitize(html);
}

function showUpdateResult(result) {
    const release = result?.latest_release;
    if (!release) {
        return;
    }

    $('#tauritavern_update_version').text(release.version || release.tag_name || UNKNOWN_VALUE);
    $('#tauritavern_update_changelog').html(renderChangelogHtml(release.body));
    $('#tauritavern_update_download').attr('href', release.html_url);
    $('#tauritavern_update_result').slideDown(200);
}

function hideUpdateResult() {
    $('#tauritavern_update_result').slideUp(200);
}

async function onCheckUpdateClick() {
    const $btn = $('#tauritavern_check_update');
    const $icon = $btn.find('i');
    const $text = $btn.find('span');

    $icon.addClass('fa-spin');
    $text.text(UPDATE_CHECKING_TEXT);
    $btn.prop('disabled', true);

    try {
        const result = await checkForUpdate();
        if (result?.has_update && result?.latest_release) {
            showUpdateResult(result);
        } else {
            globalThis.toastr?.info(UPDATE_NO_UPDATE_TEXT);
            hideUpdateResult();
        }
    } catch (error) {
        globalThis.toastr?.error(`检查更新失败: ${error}`);
    } finally {
        $icon.removeClass('fa-spin');
        $text.text('检查更新');
        $btn.prop('disabled', false);
    }
}

function buildStartupUpdatePopupContent(release) {
    const root = document.createElement('div');
    root.className = 'ttv-update-popup';

    const title = document.createElement('h3');
    title.textContent = `TauriTavern ${release.version} 已发布`;
    root.appendChild(title);

    const body = document.createElement('div');
    body.className = 'ttv-update-popup-body';
    body.innerHTML = renderChangelogHtml(release.body);
    root.appendChild(body);

    return root;
}

async function showStartupUpdatePopup(release) {
    const popup = new Popup(buildStartupUpdatePopupContent(release), POPUP_TYPE.CONFIRM, '', {
        okButton: '前往下载',
        cancelButton: '稍后',
        allowVerticalScrolling: true,
        wide: true,
        large: true,
    });

    const result = await popup.show();
    if (result === POPUP_RESULT.AFFIRMATIVE) {
        window.open(release.html_url, '_blank', 'noopener,noreferrer');
    }
}

let startupUpdatePopupShown = false;

async function listenForStartupUpdate() {
    try {
        await listen('update-available', (event) => {
            const result = event?.payload;
            if (!result?.has_update || !result?.latest_release) {
                return;
            }

            showUpdateResult(result);

            if (startupUpdatePopupShown) {
                return;
            }
            startupUpdatePopupShown = true;
            void showStartupUpdatePopup(result.latest_release);
        });
    } catch (error) {
        console.warn('Failed to listen for update-available event:', error);
    }
}

jQuery(async () => {
    const container = $('#tauritavern_version_container');
    if (!container.length) {
        return;
    }

    void listenForStartupUpdate();

    const html = await renderExtensionTemplateAsync(MODULE_NAME, 'settings', LINKS);
    container.append(html);
    $('#tauritavern_version_copy').on('click', onCopyVersionClick);
    $('#tauritavern_check_update').on('click', onCheckUpdateClick);
    $('#tauritavern_update_dismiss').on('click', hideUpdateResult);

    const versionInfo = await resolveVersionInfo();
    renderVersionInfo(versionInfo);
});
