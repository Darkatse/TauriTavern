import { translate } from '../../../i18n.js';

export function formatTimestamp(ms) {
    if (!ms) {
        return translate('N/A');
    }

    const date = new Date(Number(ms));
    if (Number.isNaN(date.getTime())) {
        return translate('Invalid time');
    }

    return date.toLocaleString();
}

export function formatBytes(value) {
    const bytes = Number(value) || 0;
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let size = bytes;
    let unitIndex = 0;

    while (size >= 1024 && unitIndex < units.length - 1) {
        size /= 1024;
        unitIndex += 1;
    }

    return `${size.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

