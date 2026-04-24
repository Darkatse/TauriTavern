import { getHostAbi } from './layout-kit.js';

export function getIosPolicyActivationReport() {
    return getHostAbi()?.iosPolicy ?? null;
}

export function getActiveIosPolicyActivationReport() {
    const report = getIosPolicyActivationReport();
    if (!report || report.scope !== 'ios') {
        return null;
    }

    return report;
}

export function getActiveIosPolicyCapabilities() {
    return getActiveIosPolicyActivationReport()?.capabilities ?? null;
}

export function allowlistSettingAllows(allowlist, value) {
    if (allowlist === 'all') {
        return true;
    }

    if (Array.isArray(allowlist)) {
        return allowlist.includes(value);
    }

    throw new Error(`[TauriTavern][iOSPolicy] Unsupported allowlist setting: ${JSON.stringify(allowlist)}`);
}

