import { CLIENT_VERSION, displayVersion } from '../../../script.js';
import { getClientVersion as getBridgeClientVersion } from '../../../tauri-bridge.js';
import { renderExtensionTemplateAsync } from '../../extensions.js';

const MODULE_NAME = 'tauritavern-version';
const LINKS = Object.freeze({
    authorName: 'Darkatse',
    repositoryUrl: 'https://github.com/Darkatse/TauriTavern',
    discordUrl: 'https://discord.com/channels/1134557553011998840/1472415443078742188',
});

const COPY_SUCCESS_TEXT = '版本信息已复制到剪贴板';
const COPY_FAILURE_TEXT = '复制失败，请手动复制版本信息';

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

    return normalized || 'UNKNOWN';
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
    const summary = [
        `TauriTavern ${packageVersion}`,
        `Compat ${compatBaseline}`,
        `Git ${gitInfo}`,
    ].join(' | ');

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

jQuery(async () => {
    const container = $('#tauritavern_version_container');
    if (!container.length) {
        return;
    }

    const html = await renderExtensionTemplateAsync(MODULE_NAME, 'settings', LINKS);
    container.append(html);
    $('#tauritavern_version_copy').on('click', onCopyVersionClick);

    const versionInfo = await resolveVersionInfo();
    renderVersionInfo(versionInfo);
});
