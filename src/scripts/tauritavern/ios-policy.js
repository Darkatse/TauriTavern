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

