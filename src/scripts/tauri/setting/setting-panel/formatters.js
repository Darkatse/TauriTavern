import { translate } from '../../../i18n.js';

export { formatBytes } from '../format-bytes.js';

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

