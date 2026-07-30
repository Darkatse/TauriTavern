import { DOMPurify } from '../../../lib.js';
import { CLIENT_VERSION, converter, displayVersion, eventSource, event_types, reloadMarkdownProcessor } from '../../../script.js';
import {
    checkForUpdate,
    getClientVersion as getBridgeClientVersion,
    getTauriTavernSettings,
    invoke,
    openExternalUrl,
    updateTauriTavernSettings,
} from '../../../tauri-bridge.js';
import { renderExtensionTemplateAsync } from '../../extensions.js';
import { translate } from '../../i18n.js';
import { POPUP_RESULT, POPUP_TYPE, Popup } from '../../popup.js';
import { stripCommandErrorPrefixes } from '../../util/command-error-utils.js';
import { isGitHubRateLimitMessage } from '../../util/github-rate-limit.js';
import { githubRateLimitStopper } from '../../util/github-rate-limit-stopper.js';
import { isIosRuntime } from '../../util/mobile-runtime.js';
import { extractErrorText, toUserFacingErrorText } from '../../util/user-facing-error.js';
import { getActiveIosPolicyCapabilities } from '../../tauritavern/ios-policy.js';

const MODULE_NAME = 'tauritavern-version';
const LINKS = Object.freeze({
    authorName: 'Darkatse',
    repositoryUrl: 'https://github.com/Darkatse/TauriTavern',
    discordUrl: 'https://discord.gg/hn57aFGe8h',
    telegramUrl: 'https://t.me/TauriTavern',
});

const UNKNOWN_VALUE = 'UNKNOWN';

let latestUpdateResult = null;
let startupUpdateCheckPromise = null;
let startupUpdatePopupShown = false;
let tauriTavernSettingsCache = null;
let tauriTavernSettingsPromise = null;
let versionInfoCache = null;
let versionInfoPromise = null;

function resolveIosUpdateCapabilities() {
    return getActiveIosPolicyCapabilities()?.updates ?? null;
}

function resolveIosAboutCapabilities() {
    return getActiveIosPolicyCapabilities()?.about ?? null;
}

function localize(key, fallback) {
    return translate(fallback, key);
}

function localizeTemplate(key, fallback, ...values) {
    const template = localize(key, fallback);
    return template.replace(/\$\{(\d+)\}/g, (_, index) => String(values[Number(index)] ?? ''));
}

function tripGitHubRateLimitIfNeeded(error) {
    const normalized = stripCommandErrorPrefixes(extractErrorText(error));
    if (!isGitHubRateLimitMessage(normalized)) {
        return false;
    }

    githubRateLimitStopper.trip();
    return true;
}

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
    const defaultUpdateChannel = payload?.defaultUpdateChannel === 'canary' ? 'canary' : 'stable';

    return {
        packageVersion,
        compatBaseline,
        gitInfo,
        gitRevision,
        defaultUpdateChannel,
    };
}

async function resolveVersionInfo() {
    if (versionInfoCache) {
        return versionInfoCache;
    }

    if (!versionInfoPromise) {
        versionInfoPromise = getBridgeClientVersion()
            .then(buildVersionInfo)
            .catch((error) => {
                console.warn('TauriTavern version extension fallback:', error);
                return buildVersionInfo();
            })
            .then((info) => {
                versionInfoCache = info;
                return info;
            })
            .finally(() => {
                versionInfoPromise = null;
            });
    }

    return versionInfoPromise;
}

function renderVersionInfo(info) {
    $('#tauritavern_version_number').text(info.packageVersion);
    $('#tauritavern_compat_version').text(info.compatBaseline);
    $('#tauritavern_git_info').text(info.gitInfo);
}

async function onExportDebugBundleClick() {
    const $btn = $('#tauritavern_export_debug_bundle');
    const $icon = $btn.find('i');
    const $text = $btn.find('span');
    const defaultText = String($text.data('defaultLabel') || $text.text()).trim();

    $text.data('defaultLabel', defaultText);
    $icon.addClass('fa-spin');
    $text.text(localize('ttv_version.exporting_bundle', 'Exporting...'));
    $btn.prop('disabled', true);

    try {
        const devApi = window.__TAURITAVERN__?.api?.dev;
        if (!devApi || typeof devApi.exportBundle !== 'function') {
            throw new Error('TauriTavern host dev API exportBundle is unavailable');
        }

        const savedPath = await devApi.exportBundle();

        if (isIosRuntime()) {
            const shareResult = await invoke('ios_share_file', { filePath: savedPath });
            if (shareResult?.completed === true) {
                globalThis.toastr?.success?.(localize('ttv_version.export_success', 'Export completed.'));
            }
            return;
        }

        globalThis.toastr?.success?.(localize('ttv_version.export_success', 'Export completed.'));
        await new Popup(savedPath, POPUP_TYPE.TEXT, localize('ttv_version.export_debug_bundle', 'Export Debug Bundle'), {
            okButton: localize('ttv_version.ok', 'OK'),
            allowVerticalScrolling: true,
            wide: true,
            large: false,
        }).show();
    } catch (error) {
        console.error('TauriTavern debug bundle export failed:', error);
        globalThis.toastr?.error?.(
            localizeTemplate(
                'ttv_version.export_failed',
                'Failed to export debug bundle: ${0}',
                toUserFacingErrorText(error) || extractErrorText(error),
            ),
        );
    } finally {
        $icon.removeClass('fa-spin');
        $text.text(defaultText);
        $btn.prop('disabled', false);
    }
}

async function openVersionUrl(url) {
    try {
        await openExternalUrl(url);
    } catch (error) {
        globalThis.toastr?.error?.(
            localizeTemplate(
                'ttv_version.open_link_failed',
                'Failed to open link: ${0}',
                toUserFacingErrorText(error) || extractErrorText(error),
            ),
        );
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
        return `<p>${localize('ttv_version.no_changelog', 'No changelog available.')}</p>`;
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

    $('#tauritavern_update_version').text(getReleaseLabel(result));
    $('#tauritavern_update_changelog').html(renderChangelogHtml(release.body));
    $('#tauritavern_update_download').attr('href', release.html_url);

    if ($result.is(':hidden')) {
        $result.slideDown(200);
    }
}

function getReleaseLabel(result) {
    const release = result?.latest_release;
    if (!release) {
        return UNKNOWN_VALUE;
    }

    return result.channel === 'canary'
        ? (release.name || release.tag_name || UNKNOWN_VALUE)
        : (release.version || release.name || release.tag_name || UNKNOWN_VALUE);
}

async function getEffectiveUpdateChannel() {
    const [settings, versionInfo] = await Promise.all([
        getTauriTavernSettingsState(),
        resolveVersionInfo(),
    ]);
    return settings?.updates?.channel || versionInfo.defaultUpdateChannel;
}

function hideUpdateResult() {
    const $result = $('#tauritavern_update_result');
    if ($result.length && $result.is(':visible')) {
        $result.slideUp(200);
    }
}

async function onCheckUpdateClick() {
    if (githubRateLimitStopper.isTripped()) {
        return;
    }

    const updateCaps = resolveIosUpdateCapabilities();
    if (updateCaps && updateCaps.manual_check === false) {
        globalThis.toastr?.error?.(
            localize(
                'ttv_version.check_update_disabled',
                'Manual update checking is disabled by your iOS policy.',
            ),
        );
        return;
    }

    const $btn = $('#tauritavern_check_update');
    const $icon = $btn.find('i');
    const $text = $btn.find('span');
    const defaultText = String($text.data('defaultLabel') || $text.text()).trim();

    $text.data('defaultLabel', defaultText);
    $icon.addClass('fa-spin');
    $text.text(localize('ttv_version.checking_updates', 'Checking for updates...'));
    $btn.prop('disabled', true);

    try {
        const result = await checkForUpdate(await getEffectiveUpdateChannel());
        if (result?.has_update && result?.latest_release) {
            showUpdateResult(result);
        } else {
            latestUpdateResult = null;
            globalThis.toastr?.info?.(localize('ttv_version.no_update', 'You are already on the latest version.'));
            hideUpdateResult();
        }
    } catch (error) {
        if (tripGitHubRateLimitIfNeeded(error)) {
            return;
        }

        globalThis.toastr?.error?.(
            localizeTemplate(
                'ttv_version.check_update_failed',
                'Failed to check for updates: ${0}',
                toUserFacingErrorText(error) || extractErrorText(error),
            ),
        );
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
    return String(result?.release_token || '').trim();
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
    const releaseLabel = getReleaseLabel(result);
    const root = document.createElement('div');
    root.className = 'ttv-update-popup';

    const header = document.createElement('div');
    header.className = 'ttv-update-popup-header';

    const title = document.createElement('h3');
    title.className = 'ttv-update-popup-title';
    title.textContent = localizeTemplate(
        'ttv_version.popup_title',
        'TauriTavern update available: ${0}',
        releaseLabel,
    );
    header.appendChild(title);

    const meta = document.createElement('p');
    meta.className = 'ttv-update-popup-meta';
    meta.textContent = result.channel === 'canary'
        ? localize('ttv_version.canary_channel', 'Canary channel')
        : `TauriTavern ${result.current_version}  \u2192  ${release.version}`;
    header.appendChild(meta);

    const note = document.createElement('p');
    note.className = 'ttv-update-popup-note';
    note.textContent = localize(
        'ttv_version.popup_note',
        'You can update later. Manual update checking remains available.',
    );
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
        okButton: localize('ttv_version.popup_download', 'Download'),
        cancelButton: localize('ttv_version.popup_later', 'Later'),
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
    if (githubRateLimitStopper.isTripped()) {
        return;
    }

    if (startupUpdateCheckPromise) {
        return startupUpdateCheckPromise;
    }

    startupUpdateCheckPromise = (async () => {
        let result;

        try {
            result = await checkForUpdate(await getEffectiveUpdateChannel());
        } catch (error) {
            if (tripGitHubRateLimitIfNeeded(error)) {
                return;
            }

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
    const updateCaps = resolveIosUpdateCapabilities();
    if (updateCaps && updateCaps.startup_check === false) {
        console.debug('Startup update check skipped: disabled by iOS policy');
        return;
    }

    void runStartupUpdateCheck();
});

function renderChannelHint(channel) {
    const hint = document.getElementById('ttv-channel-hint');
    if (!(hint instanceof HTMLElement)) {
        return;
    }

    const isCanary = channel === 'canary';
    hint.classList.toggle('ttv-hint-canary', isCanary);
    hint.textContent = isCanary
        ? localize('ttv_version.channel_hint_canary', 'Frequent preview builds; may be unstable.')
        : localize('ttv_version.channel_hint_stable', 'Recommended for most users.');
}

function renderChannel(channel) {
    const radio = document.querySelector(`input[name="tauritavern_update_channel"][value="${channel}"]`);
    if (radio instanceof HTMLInputElement) {
        radio.checked = true;
    }

    const isCanary = channel === 'canary';
    const currentName = document.getElementById('ttv-channel-current-name');
    if (currentName instanceof HTMLElement) {
        currentName.classList.toggle('ttv-canary', isCanary);
        currentName.textContent = isCanary
            ? localize('ttv_version.channel_canary', 'Canary')
            : localize('ttv_version.channel_stable', 'Stable');
    }

    renderChannelHint(channel);
}

function setChannelBodyOpen(open) {
    $('#ttv-channel-toggle').attr('aria-expanded', String(open)).toggleClass('open', open);
    const $body = $('#ttv-channel-body');
    $body.stop();
    if (open) {
        $body.slideDown(200);
    } else {
        $body.slideUp(200);
    }
}

function onChannelToggleClick() {
    const open = $('#ttv-channel-toggle').attr('aria-expanded') === 'true';
    setChannelBodyOpen(!open);
}

function setChannelInputsDisabled(disabled) {
    document.querySelectorAll('input[name="tauritavern_update_channel"]').forEach((input) => {
        if (input instanceof HTMLInputElement) {
            input.disabled = disabled;
        }
    });
}

async function onUpdateChannelChange(event) {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) {
        return;
    }

    const nextChannel = input.value;
    const previousChannel = tauriTavernSettingsCache?.updates?.channel
        || versionInfoCache?.defaultUpdateChannel
        || 'stable';
    setChannelInputsDisabled(true);

    try {
        const settings = await getTauriTavernSettingsState();
        tauriTavernSettingsCache = await updateTauriTavernSettings({
            updates: {
                channel: nextChannel,
                startup_popup: settings.updates.startup_popup,
            },
        });
        latestUpdateResult = null;
        hideUpdateResult();
        renderChannel(nextChannel);
        setChannelBodyOpen(false);
    } catch (error) {
        renderChannel(previousChannel);
        globalThis.toastr?.error?.(
            localizeTemplate(
                'ttv_version.channel_save_failed',
                'Failed to save update channel: ${0}',
                toUserFacingErrorText(error) || extractErrorText(error),
            ),
        );
    } finally {
        setChannelInputsDisabled(false);
    }
}

jQuery(async () => {
    const container = $('#tauritavern_version_container');
    if (!container.length) {
        return;
    }

    const html = await renderExtensionTemplateAsync(MODULE_NAME, 'settings', LINKS);
    container.append(html);
    $('#tauritavern_export_debug_bundle').on('click', () => void onExportDebugBundleClick());

    const aboutCaps = resolveIosAboutCapabilities();
    if (aboutCaps && aboutCaps.git_info === false) {
        const gitRow = document.getElementById('ttv-git-row');
        if (!(gitRow instanceof HTMLElement)) {
            throw new Error('[TauriTavern][iOSPolicy] ttv-git-row not found');
        }
        gitRow.hidden = true;
    }

    const updateCaps = resolveIosUpdateCapabilities();
    if (updateCaps && updateCaps.manual_check === false) {
        $('#tauritavern_check_update').remove();

        const channelRow = document.getElementById('ttv-channel-row');
        if (!(channelRow instanceof HTMLElement)) {
            throw new Error('[TauriTavern][iOSPolicy] ttv-channel-row not found');
        }
        channelRow.hidden = true;

        const compatRow = document.getElementById('ttv-compat-row');
        if (!(compatRow instanceof HTMLElement)) {
            throw new Error('[TauriTavern][iOSPolicy] ttv-compat-row not found');
        }
        compatRow.hidden = true;

        const discordLink = document.getElementById('ttv-discord-link');
        if (!(discordLink instanceof HTMLElement)) {
            throw new Error('[TauriTavern][iOSPolicy] ttv-discord-link not found');
        }
        discordLink.hidden = true;

        const telegramLink = document.getElementById('ttv-telegram-link');
        if (!(telegramLink instanceof HTMLElement)) {
            throw new Error('[TauriTavern][iOSPolicy] ttv-telegram-link not found');
        }
        telegramLink.hidden = true;
    } else {
        $('#tauritavern_check_update').on('click', onCheckUpdateClick);
    }
    $('#tauritavern_update_dismiss').on('click', hideUpdateResult);
    container.on('click', 'a[target="_blank"]', onExternalLinkClick);

    const [versionInfo, settings] = await Promise.all([
        resolveVersionInfo(),
        getTauriTavernSettingsState(),
    ]);
    renderVersionInfo(versionInfo);
    renderChannel(settings?.updates?.channel || versionInfo.defaultUpdateChannel);
    $('#ttv-channel-toggle').on('click', onChannelToggleClick);
    container.find('input[name="tauritavern_update_channel"]').on('change', onUpdateChannelChange);

    if (latestUpdateResult?.has_update && latestUpdateResult?.latest_release) {
        showUpdateResult(latestUpdateResult);
    }
});
